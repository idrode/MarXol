//! Presupuesto de peticiones RPC por proveedor: ritmo máximo, tope por
//! ejecución y contador de peticiones y unidades (CU o créditos) realmente
//! consumidas. Costes de CLAUDE.md sección 7.

use anyhow::{bail, Result};
use std::cell::Cell;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Provider {
    pub name: &'static str,
    /// Peticiones por segundo por defecto (por debajo del límite del plan).
    pub max_rps: f64,
    /// Unidades por `getTransaction` y por `getSignaturesForAddress`.
    pub cost_get_transaction: u64,
    pub cost_get_signatures: u64,
}

pub const PUBLIC: Provider =
    Provider { name: "public", max_rps: 4.0, cost_get_transaction: 0, cost_get_signatures: 0 };
/// Free tier: 25 req/s; 40 CU por método histórico.
pub const ALCHEMY: Provider =
    Provider { name: "alchemy", max_rps: 20.0, cost_get_transaction: 40, cost_get_signatures: 40 };
/// Free tier: 10 req/s; 10 créditos por método de archivo.
pub const HELIUS: Provider =
    Provider { name: "helius", max_rps: 8.0, cost_get_transaction: 10, cost_get_signatures: 10 };

impl Provider {
    /// `auto` deduce el proveedor de la URL del RPC.
    pub fn parse(name: &str, rpc_url: &str) -> Result<Provider> {
        Ok(match name {
            "public" => PUBLIC,
            "alchemy" => ALCHEMY,
            "helius" => HELIUS,
            "auto" if rpc_url.contains("alchemy.com") => ALCHEMY,
            "auto" if rpc_url.contains("helius") => HELIUS,
            "auto" => PUBLIC,
            _ => bail!("proveedor desconocido: {name} (auto, public, alchemy, helius)"),
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Method {
    GetTransaction,
    GetSignatures,
}

pub struct Budget {
    pub provider: Provider,
    min_interval: Duration,
    max_requests: Option<u64>,
    last: Cell<Option<Instant>>,
    pub get_transaction: Cell<u64>,
    pub get_signatures: Cell<u64>,
}

impl Budget {
    pub fn new(provider: Provider, max_rps: Option<f64>, max_requests: Option<u64>) -> Budget {
        let rps = max_rps.unwrap_or(provider.max_rps).max(0.01);
        Budget {
            provider,
            min_interval: Duration::from_secs_f64(1.0 / rps),
            max_requests,
            last: Cell::new(None),
            get_transaction: Cell::new(0),
            get_signatures: Cell::new(0),
        }
    }

    pub fn requests(&self) -> u64 {
        self.get_transaction.get() + self.get_signatures.get()
    }

    pub fn units(&self) -> u64 {
        self.get_transaction.get() * self.provider.cost_get_transaction
            + self.get_signatures.get() * self.provider.cost_get_signatures
    }

    /// `true` si queda cupo para `n` peticiones más en esta ejecución.
    pub fn has_room(&self, n: u64) -> bool {
        self.max_requests.is_none_or(|m| self.requests() + n <= m)
    }

    /// Llamar justo antes de cada petición: espera lo necesario para no pasar
    /// del ritmo, falla si se agotó el tope y cuenta la petición (también las
    /// que luego fallen: consumen cuota igual).
    pub fn acquire(&self, m: Method) -> Result<()> {
        if !self.has_room(1) {
            bail!("tope de peticiones de esta ejecución alcanzado ({})", self.requests());
        }
        if let Some(last) = self.last.get() {
            let wait = self.min_interval.saturating_sub(last.elapsed());
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }
        self.last.set(Some(Instant::now()));
        let c = match m {
            Method::GetTransaction => &self.get_transaction,
            Method::GetSignatures => &self.get_signatures,
        };
        c.set(c.get() + 1);
        Ok(())
    }

    pub fn summary(&self) -> String {
        format!(
            "{}: {} getTransaction + {} getSignaturesForAddress = {} peticiones, {} unidades",
            self.provider.name,
            self.get_transaction.get(),
            self.get_signatures.get(),
            self.requests(),
            self.units()
        )
    }
}

/// Hace la petición con `acquire` y, si el proveedor responde 429, la
/// reintenta hasta 4 veces con espera creciente (1, 2, 4, 8 s). Cada intento
/// cuenta en el presupuesto: consume cuota igual.
pub fn with_retry<T>(b: &Budget, m: Method, mut f: impl FnMut() -> Result<T>) -> Result<T> {
    let mut wait = Duration::from_secs(1);
    for attempt in 0.. {
        b.acquire(m)?;
        match f() {
            Err(e) if attempt < 4 && is_rate_limited(&e) => {
                std::thread::sleep(wait);
                wait *= 2;
            }
            r => return r,
        }
    }
    unreachable!()
}

fn is_rate_limited(e: &anyhow::Error) -> bool {
    let s = format!("{e:#}");
    s.contains("429") || s.contains("Too Many Requests")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuenta_unidades_por_proveedor_y_respeta_el_tope() {
        let b = Budget::new(ALCHEMY, Some(1000.0), Some(3));
        b.acquire(Method::GetSignatures).unwrap();
        b.acquire(Method::GetTransaction).unwrap();
        b.acquire(Method::GetTransaction).unwrap();
        assert_eq!(b.units(), 120);
        assert!(!b.has_room(1));
        assert!(b.acquire(Method::GetTransaction).is_err());
        assert_eq!(b.requests(), 3, "la petición rechazada no cuenta");
    }

    #[test]
    fn el_ritmo_espacia_las_peticiones() {
        let b = Budget::new(PUBLIC, Some(50.0), None);
        let t = Instant::now();
        for _ in 0..3 {
            b.acquire(Method::GetTransaction).unwrap();
        }
        assert!(t.elapsed() >= Duration::from_millis(38), "{:?}", t.elapsed());
        assert_eq!(b.units(), 0);
    }

    #[test]
    fn reintenta_solo_los_429_y_cuenta_cada_intento() {
        let b = Budget::new(ALCHEMY, Some(1000.0), None);
        let mut n = 0;
        let r = with_retry(&b, Method::GetTransaction, || {
            n += 1;
            if n < 2 { anyhow::bail!("HTTP status client error (429 Too Many Requests)") } else { Ok(n) }
        });
        assert_eq!(r.unwrap(), 2);
        assert_eq!(b.get_transaction.get(), 2);
        let r: Result<()> = with_retry(&b, Method::GetTransaction, || anyhow::bail!("otro error"));
        assert!(r.is_err());
        assert_eq!(b.get_transaction.get(), 3, "un error que no es 429 no se reintenta");
    }

    #[test]
    fn auto_deduce_el_proveedor_de_la_url() {
        assert_eq!(Provider::parse("auto", "https://solana-mainnet.g.alchemy.com/v2/k").unwrap(), ALCHEMY);
        assert_eq!(Provider::parse("auto", "https://mainnet.helius-rpc.com/?api-key=k").unwrap(), HELIUS);
        assert_eq!(Provider::parse("auto", "https://api.mainnet-beta.solana.com").unwrap(), PUBLIC);
    }
}
