//! Presupuesto de peticiones RPC por proveedor: ritmo máximo, tope por
//! ejecución y contador de peticiones y unidades (CU o créditos) realmente
//! consumidas. Costes de CLAUDE.md sección 7.

use crate::ledger::Exhausted;
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
    /// Unidades por `getAccountInfo` y por `getMultipleAccounts`.
    pub cost_get_account: u64,
    pub cost_get_multiple: u64,
}

pub const PUBLIC: Provider =
    Provider { name: "public", max_rps: 4.0, cost_get_transaction: 0, cost_get_signatures: 0, cost_get_account: 0, cost_get_multiple: 0 };
/// Free tier: 25 req/s y 500 CU/s; 40 CU por método histórico. 12 req/s × 40 CU =
/// 480 CU/s, por debajo de los 500 CU/s (con 20 req/s eran 800 CU/s, por encima).
/// Configurable con `--max-rps` / `MARXOL_MAX_RPS`. `getAccountInfo` = 10 CU
/// (tabla de `scripts/madurez_fase0b.py`); `getMultipleAccounts` no figura en
/// la tabla revisada: se cargan 20 CU, por el lado conservador.
pub const ALCHEMY: Provider = Provider {
    name: "alchemy",
    max_rps: 12.0,
    cost_get_transaction: 40,
    cost_get_signatures: 40,
    cost_get_account: 10,
    cost_get_multiple: 20,
};
/// Free tier: 10 req/s; 10 créditos por método de archivo, 1 por el resto.
pub const HELIUS: Provider = Provider {
    name: "helius",
    max_rps: 8.0,
    cost_get_transaction: 10,
    cost_get_signatures: 10,
    cost_get_account: 1,
    cost_get_multiple: 1,
};

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

    pub fn cost(&self, m: Method) -> u64 {
        match m {
            Method::GetTransaction => self.cost_get_transaction,
            Method::GetSignatures => self.cost_get_signatures,
            Method::GetAccountInfo => self.cost_get_account,
            Method::GetMultipleAccounts => self.cost_get_multiple,
        }
    }

    /// Coste de la petición más cara (para saber si cabe una más).
    fn max_cost(&self) -> u64 {
        self.cost_get_transaction.max(self.cost_get_signatures).max(self.cost_get_account).max(self.cost_get_multiple)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Method {
    GetTransaction,
    GetSignatures,
    GetAccountInfo,
    GetMultipleAccounts,
}

impl Method {
    pub fn name(self) -> &'static str {
        match self {
            Method::GetTransaction => "getTransaction",
            Method::GetSignatures => "getSignaturesForAddress",
            Method::GetAccountInfo => "getAccountInfo",
            Method::GetMultipleAccounts => "getMultipleAccounts",
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
    /// `getAccountInfo` y `getMultipleAccounts` (peticiones y unidades).
    pub other_requests: Cell<u64>,
    other_units: Cell<u64>,
    /// Reintentos por 429 de esta ejecución (cada uno cuenta también como petición).
    pub retries_429: Cell<u64>,
    /// Libro de CU diario compartido (`marxol_vivo.db`); `None` en los tests.
    ledger: Option<crate::ledger::Ledger>,
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
            other_requests: Cell::new(0),
            other_units: Cell::new(0),
            retries_429: Cell::new(0),
            ledger: None,
        }
    }

    /// Carga cada petición también en el libro diario compartido.
    pub fn with_ledger(mut self, l: crate::ledger::Ledger) -> Budget {
        self.ledger = Some(l);
        self
    }

    pub fn ledger(&self) -> Option<&crate::ledger::Ledger> {
        self.ledger.as_ref()
    }

    pub fn requests(&self) -> u64 {
        self.get_transaction.get() + self.get_signatures.get() + self.other_requests.get()
    }

    pub fn units(&self) -> u64 {
        self.get_transaction.get() * self.provider.cost_get_transaction
            + self.get_signatures.get() * self.provider.cost_get_signatures
            + self.other_units.get()
    }

    /// Tope de peticiones de esta ejecución (`None`: sin tope).
    pub fn max_requests(&self) -> Option<u64> {
        self.max_requests
    }

    /// `true` si queda cupo para `n` peticiones más en esta ejecución y, con
    /// libro, para `n` de las más caras en el tope diario.
    pub fn has_room(&self, n: u64) -> bool {
        self.max_requests.is_none_or(|m| self.requests() + n <= m)
            && self.ledger.as_ref().is_none_or(|l| {
                l.remaining(unix_now()).is_ok_and(|r| r >= n * self.provider.max_cost())
            })
    }

    /// Llamar justo antes de cada petición: espera lo necesario para no pasar
    /// del ritmo, falla si se agotó el tope y cuenta la petición (también las
    /// que luego fallen: consumen cuota igual). Con libro, la carga en el tope
    /// diario compartido; si no cabe, falla con `ledger::Exhausted`.
    pub fn acquire(&self, m: Method) -> Result<()> {
        if self.max_requests.is_some_and(|max| self.requests() + 1 > max) {
            bail!(Exhausted(format!("tope de peticiones de esta ejecución alcanzado ({})", self.requests())));
        }
        if let Some(last) = self.last.get() {
            let wait = self.min_interval.saturating_sub(last.elapsed());
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }
        if let Some(l) = &self.ledger {
            l.charge(self.provider.cost(m), unix_now())?;
        }
        self.last.set(Some(Instant::now()));
        let c = match m {
            Method::GetTransaction => &self.get_transaction,
            Method::GetSignatures => &self.get_signatures,
            Method::GetAccountInfo | Method::GetMultipleAccounts => {
                self.other_units.set(self.other_units.get() + self.provider.cost(m));
                &self.other_requests
            }
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
        let other = match self.other_requests.get() {
            0 => String::new(),
            n => format!(" + {n} de cuentas"),
        };
        format!(
            "{}: {} getTransaction + {} getSignaturesForAddress{other} = {} peticiones, {} unidades",
            self.provider.name,
            self.get_transaction.get(),
            self.get_signatures.get(),
            self.requests(),
            self.units()
        )
    }

    /// Cierra la fila del proceso en el libro y devuelve la línea de estado
    /// del día (`None` sin libro).
    pub fn finish(&self) -> Result<Option<String>> {
        let Some(l) = &self.ledger else { return Ok(None) };
        let now = unix_now();
        l.finish(now)?;
        Ok(Some(l.summary(now)?))
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
            // El error del cliente HTTP lleva la URL del RPC, y con ella la clave.
            r => return r.map_err(|e| anyhow::anyhow!(redact(&format!("{e:#}")))),
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

/// Quita las claves de API de un texto (mensajes de error del cliente HTTP,
/// que incluyen la URL del RPC): el segmento tras `/v2/` (Alchemy) y el
/// valor de `api-key=` (Helius). Se aplica a todo error que se imprime o se
/// guarda.
pub fn redact(s: &str) -> String {
    let mut out = s.to_string();
    for marker in ["/v2/", "api-key=", "api_key="] {
        let mut from = 0;
        while let Some(i) = out[from..].find(marker) {
            let start = from + i + marker.len();
            let end = out[start..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                .map_or(out.len(), |j| start + j);
            if end > start {
                out.replace_range(start..end, "<redactado>");
            }
            from = start + "<redactado>".len().min(out.len() - start);
        }
    }
    out
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

/// `true` si el error es un tope (diario o de la ejecución) y no un fallo de
/// la petición: la descarga debe parar limpiamente.
pub fn is_exhausted(e: &anyhow::Error) -> bool {
    e.is::<Exhausted>()
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
        let e = b.acquire(Method::GetTransaction).unwrap_err();
        assert!(is_exhausted(&e), "{e:#}");
        assert_eq!(b.requests(), 3, "la petición rechazada no cuenta");
        assert_eq!(b.summary(), "alchemy: 2 getTransaction + 1 getSignaturesForAddress = 3 peticiones, 120 unidades");
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

    #[test]
    fn las_cuentas_tienen_su_coste_y_cargan_el_libro() {
        let p = std::env::temp_dir().join(format!("marxol_test_{}_budget.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let caps = crate::ledger::Caps::new(100, 0).unwrap();
        let l = crate::ledger::Ledger::open(p.to_str().unwrap(), "verify", crate::ledger::Pool::Manual, caps, "alchemy", unix_now())
            .unwrap();
        let b = Budget::new(ALCHEMY, Some(1000.0), None).with_ledger(l);
        b.acquire(Method::GetAccountInfo).unwrap();
        b.acquire(Method::GetMultipleAccounts).unwrap();
        b.acquire(Method::GetTransaction).unwrap();
        assert_eq!((b.requests(), b.units()), (3, 70));
        assert_eq!(b.summary(), "alchemy: 1 getTransaction + 0 getSignaturesForAddress + 2 de cuentas = 3 peticiones, 70 unidades");
        // Quedan 30 CU: no cabe otra de las caras (40) y has_room lo dice antes.
        assert!(!b.has_room(1));
        assert!(is_exhausted(&b.acquire(Method::GetTransaction).unwrap_err()));
        b.acquire(Method::GetAccountInfo).unwrap();
        assert_eq!(b.ledger().unwrap().used(unix_now()).unwrap().total, 80);
    }

    #[test]
    fn los_errores_no_llevan_la_clave() {
        let e = "HTTP status client error (429 Too Many Requests) for url (https://solana-mainnet.g.alchemy.com/v2/alch_p-x2Ab_9)";
        assert_eq!(
            redact(e),
            "HTTP status client error (429 Too Many Requests) for url (https://solana-mainnet.g.alchemy.com/v2/<redactado>)"
        );
        assert_eq!(redact("https://mainnet.helius-rpc.com/?api-key=abc-123&x=1"), "https://mainnet.helius-rpc.com/?api-key=<redactado>&x=1");
        assert_eq!(redact("sin url"), "sin url");
        let b = Budget::new(ALCHEMY, Some(1000.0), None);
        let e = "error sending request for url (https://solana-mainnet.g.alchemy.com/v2/alch_p-x2Ab_9)";
        let r: Result<()> = with_retry(&b, Method::GetTransaction, "tx", || anyhow::bail!("{e}"));
        assert!(!format!("{:#}", r.unwrap_err()).contains("alch_p"));
    }
}
