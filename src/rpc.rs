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
/// Free tier: 25 req/s y 500 CU/s; 40 CU por método histórico. 12 req/s × 40 CU =
/// 480 CU/s, por debajo de los 500 CU/s (con 20 req/s eran 800 CU/s, por encima).
/// Configurable con `--max-rps` / `MARXOL_MAX_RPS`.
pub const ALCHEMY: Provider =
    Provider { name: "alchemy", max_rps: 12.0, cost_get_transaction: 40, cost_get_signatures: 40 };
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

impl Method {
    pub fn name(self) -> &'static str {
        match self {
            Method::GetTransaction => "getTransaction",
            Method::GetSignatures => "getSignaturesForAddress",
        }
    }
}

pub struct Budget {
    pub provider: Provider,
    min_interval: Duration,
    max_requests: Option<u64>,
    last: Cell<Option<Instant>>,
    pub get_transaction: Cell<u64>,
    pub get_signatures: Cell<u64>,
    /// Reintentos por 429 de esta ejecución (cada uno cuenta también como petición).
    pub retries_429: Cell<u64>,
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
            retries_429: Cell::new(0),
        }
    }

    pub fn requests(&self) -> u64 {
        self.get_transaction.get() + self.get_signatures.get()
    }

    pub fn units(&self) -> u64 {
        self.get_transaction.get() * self.provider.cost_get_transaction
            + self.get_signatures.get() * self.provider.cost_get_signatures
    }

    /// Tope de peticiones de esta ejecución (`None`: sin tope).
    pub fn max_requests(&self) -> Option<u64> {
        self.max_requests
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

    /// Línea de resumen de los reintentos por 429 (`None` si no hubo ninguno).
    pub fn retry_summary(&self) -> Option<String> {
        let n = self.retries_429.get();
        (n > 0).then(|| format!("aviso: {n} reintentos por 429 ({}) en esta ejecución", self.provider.name))
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

/// Reintentos por 429 como máximo en cada petición.
const MAX_RETRIES: u32 = 4;

/// Hace la petición con `acquire` y, si el proveedor responde 429, la
/// reintenta hasta 4 veces con espera creciente (1, 2, 4, 8 s). Cada intento
/// cuenta en el presupuesto: consume cuota igual. Cada reintento se avisa en
/// stderr (`what` = qué se pide: token, dirección o firma) y se cuenta en
/// `b.retries_429`.
pub fn with_retry<T>(b: &Budget, m: Method, what: &str, mut f: impl FnMut() -> Result<T>) -> Result<T> {
    let mut wait = Duration::from_secs(1);
    for attempt in 0.. {
        b.acquire(m)?;
        match f() {
            Err(e) if attempt < MAX_RETRIES && is_rate_limited(&e) => {
                b.retries_429.set(b.retries_429.get() + 1);
                eprintln!("{}", retry_line(b.provider.name, m, what, attempt + 1, wait));
                std::thread::sleep(wait);
                wait *= 2;
            }
            r => return r,
        }
    }
    unreachable!()
}

/// Aviso de un reintento por 429. Solo el nombre del proveedor, nunca la URL
/// (lleva la clave).
fn retry_line(provider: &str, m: Method, what: &str, n: u32, wait: Duration) -> String {
    format!(
        "reintento por 429 ({provider}, {} {what}): reintento {n} de {MAX_RETRIES}, espera {} s",
        m.name(),
        wait.as_secs()
    )
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
        assert!(b.retry_summary().is_none());
        let r = with_retry(&b, Method::GetTransaction, "tx X", || {
            n += 1;
            if n < 2 { anyhow::bail!("HTTP status client error (429 Too Many Requests)") } else { Ok(n) }
        });
        assert_eq!(r.unwrap(), 2);
        assert_eq!(b.get_transaction.get(), 2);
        assert_eq!(b.retries_429.get(), 1);
        let r: Result<()> = with_retry(&b, Method::GetTransaction, "tx Y", || anyhow::bail!("otro error"));
        assert!(r.is_err());
        assert_eq!(b.get_transaction.get(), 3, "un error que no es 429 no se reintenta");
        assert_eq!(b.retries_429.get(), 1);
        assert_eq!(b.retry_summary().unwrap(), "aviso: 1 reintentos por 429 (alchemy) en esta ejecución");
    }

    #[test]
    fn aviso_de_reintento_dice_que_se_pide_el_intento_y_la_espera() {
        let l = retry_line("alchemy", Method::GetSignatures, "mint M, firmas de la curva C", 2, Duration::from_secs(2));
        assert_eq!(
            l,
            "reintento por 429 (alchemy, getSignaturesForAddress mint M, firmas de la curva C): reintento 2 de 4, espera 2 s"
        );
    }

    #[test]
    fn alchemy_por_defecto_bajo_500_cu_por_segundo() {
        assert_eq!(ALCHEMY.max_rps, 12.0);
        assert!(ALCHEMY.max_rps * ALCHEMY.cost_get_transaction as f64 <= 500.0);
        // Sigue siendo configurable.
        let b = Budget::new(ALCHEMY, Some(5.0), None);
        assert_eq!(b.min_interval, Duration::from_millis(200));
    }

    #[test]
    fn auto_deduce_el_proveedor_de_la_url() {
        assert_eq!(Provider::parse("auto", "https://solana-mainnet.g.alchemy.com/v2/k").unwrap(), ALCHEMY);
        assert_eq!(Provider::parse("auto", "https://mainnet.helius-rpc.com/?api-key=k").unwrap(), HELIUS);
        assert_eq!(Provider::parse("auto", "https://api.mainnet-beta.solana.com").unwrap(), PUBLIC);
    }
}
