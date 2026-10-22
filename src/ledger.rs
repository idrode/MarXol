//! Libro de CU diario compartido entre procesos, persistido en la base de
//! recolección (`marxol_vivo.db`). Todos los comandos que usan la red cargan
//! aquí cada petición antes de hacerla, así que el tope diario vale para la
//! suma de `collect` y los comandos manuales, aunque corran a la vez.
//!
//! Dos bolsas por día UTC: `collect` (su cuota, por defecto 700 000 CU) y
//! `manual` (el resto hasta el total, por defecto 300 000). Ninguna bolsa
//! puede comerse la de la otra y la suma nunca pasa del total. El día cambia
//! a las 00:00 UTC (`date(t, 'unixepoch')` de SQLite).
//!
//! Solo guarda el nombre del proveedor, nunca la URL del RPC (lleva la clave).

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Total diario por defecto: el free tier de Alchemy (30 M CU/mes) son
/// ~1 M CU/día.
pub const DEFAULT_TOTAL: u64 = 1_000_000;
/// Cuota diaria por defecto de `collect`; el resto queda para los manuales.
pub const DEFAULT_COLLECT: u64 = 700_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pool {
    Collect,
    Manual,
}

impl Pool {
    pub fn name(self) -> &'static str {
        match self {
            Pool::Collect => "collect",
            Pool::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Caps {
    pub total: u64,
    pub collect: u64,
}

impl Caps {
    pub fn new(total: u64, collect: u64) -> Result<Caps> {
        anyhow::ensure!(collect <= total, "la cuota de collect ({collect}) no puede pasar del total diario ({total})");
        Ok(Caps { total, collect })
    }

    pub fn pool(&self, p: Pool) -> u64 {
        match p {
            Pool::Collect => self.collect,
            Pool::Manual => self.total - self.collect,
        }
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS cu_ledger_day (
    day       TEXT NOT NULL,       -- fecha UTC (YYYY-MM-DD)
    pool      TEXT NOT NULL CHECK (pool IN ('collect', 'manual')),
    units     INTEGER NOT NULL,    -- CU (Alchemy) o créditos (Helius); 0 en RPC público
    requests  INTEGER NOT NULL,
    PRIMARY KEY (day, pool)
);
-- Una fila por proceso: comando, bolsa, pid y consumo real.
CREATE TABLE IF NOT EXISTS cu_ledger_runs (
    run_id      INTEGER PRIMARY KEY,
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER,
    command     TEXT NOT NULL,
    pool        TEXT NOT NULL,
    pid         INTEGER NOT NULL,
    provider    TEXT NOT NULL,     -- solo el nombre, nunca la URL
    requests    INTEGER NOT NULL,
    units       INTEGER NOT NULL
);
"#;

/// La petición no cabe en el tope diario (o en el de la ejecución). Es un
/// error tipado para que las descargas paren limpiamente, sin dar el token
/// por fallido.
#[derive(Debug)]
pub struct Exhausted(pub String);

impl std::fmt::Display for Exhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Exhausted {}

/// Ajustes de SQLite para la base de recolección (24/7 en el teléfono):
/// WAL, `auto_vacuum = INCREMENTAL` (solo surte efecto en una base nueva,
/// antes de crear tablas) y espera ante bloqueos de otro proceso.
pub fn open_vivo_conn(path: &str) -> Result<Connection> {
    let fresh = !std::path::Path::new(path).exists();
    let db = Connection::open(path).with_context(|| format!("abriendo {path}"))?;
    db.busy_timeout(std::time::Duration::from_secs(30))?;
    if !fresh {
        let tables: i64 = db.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'", [], |r| r.get(0))?;
        let vivo: bool = db.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'vivo_meta'",
            [],
            |r| r.get(0),
        )?;
        // Protege marxol.db (u otra base) de pasar a WAL o de recibir el libro.
        anyhow::ensure!(
            tables == 0 || vivo,
            "{path} no es una base de recolección (no tiene vivo_meta): no se abre como tal"
        );
    }
    db.execute_batch(
        "PRAGMA auto_vacuum = INCREMENTAL;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         CREATE TABLE IF NOT EXISTS vivo_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    Ok(db)
}

pub struct Ledger {
    db: Connection,
    pool: Pool,
    caps: Caps,
    run_id: i64,
}

/// Consumo del día UTC de `now`: (bolsa propia, total de las dos).
#[derive(Debug, PartialEq)]
pub struct Used {
    pub pool: u64,
    pub total: u64,
}

impl Ledger {
    pub fn open(path: &str, command: &str, pool: Pool, caps: Caps, provider: &str, now: i64) -> Result<Ledger> {
        let db = open_vivo_conn(path)?;
        db.execute_batch(SCHEMA)?;
        db.execute(
            "INSERT INTO cu_ledger_runs (started_at, command, pool, pid, provider, requests, units)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 0)",
            params![now, command, pool.name(), std::process::id(), provider],
        )?;
        let run_id = db.last_insert_rowid();
        Ok(Ledger { db, pool, caps, run_id })
    }

    pub fn pool(&self) -> Pool {
        self.pool
    }

    pub fn caps(&self) -> Caps {
        self.caps
    }

    pub fn used(&self, now: i64) -> Result<Used> {
        used(&self.db, self.pool, now)
    }

    /// CU que esta bolsa aún puede gastar hoy.
    pub fn remaining(&self, now: i64) -> Result<u64> {
        let u = self.used(now)?;
        Ok(self.caps.pool(self.pool).saturating_sub(u.pool).min(self.caps.total.saturating_sub(u.total)))
    }

    /// Carga una petición de `units` antes de hacerla, de forma atómica entre
    /// procesos (`BEGIN IMMEDIATE`). Falla con `Exhausted` si no cabe.
    pub fn charge(&self, units: u64, now: i64) -> Result<()> {
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        let r = (|| {
            let u = self.used(now)?;
            let cap = self.caps.pool(self.pool);
            if u.pool + units > cap || u.total + units > self.caps.total {
                bail!(Exhausted(format!(
                    "tope diario de CU alcanzado (bolsa {}: {} de {}; total del día: {} de {})",
                    self.pool.name(),
                    u.pool,
                    cap,
                    u.total,
                    self.caps.total
                )));
            }
            self.db.execute(
                "INSERT INTO cu_ledger_day (day, pool, units, requests) VALUES (date(?1, 'unixepoch'), ?2, ?3, 1)
                 ON CONFLICT (day, pool) DO UPDATE SET units = units + ?3, requests = requests + 1",
                params![now, self.pool.name(), units as i64],
            )?;
            self.db.execute(
                "UPDATE cu_ledger_runs SET requests = requests + 1, units = units + ?2 WHERE run_id = ?1",
                params![self.run_id, units as i64],
            )?;
            Ok(())
        })();
        self.db.execute_batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
        r
    }

    /// Pids de otros procesos vivos de `command` según el libro (fila sin
    /// `ended_at` y proceso `marxol` aún presente en `/proc`).
    pub fn others_running(&self, command: &str) -> Result<Vec<i64>> {
        let mut q = self.db.prepare("SELECT DISTINCT pid FROM cu_ledger_runs WHERE command = ?1 AND ended_at IS NULL AND run_id != ?2")?;
        let pids: Vec<i64> = q.query_map(params![command, self.run_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(pids
            .into_iter()
            .filter(|p| {
                std::fs::read(format!("/proc/{p}/cmdline")).is_ok_and(|c| c.windows(6).any(|w| w == b"marxol"))
            })
            .collect())
    }

    pub fn finish(&self, now: i64) -> Result<()> {
        self.db.execute("UPDATE cu_ledger_runs SET ended_at = ?2 WHERE run_id = ?1", params![self.run_id, now])?;
        Ok(())
    }

    /// Línea de estado del día para stderr.
    pub fn summary(&self, now: i64) -> Result<String> {
        let u = self.used(now)?;
        Ok(format!(
            "libro de CU (hoy UTC): bolsa {} {} de {}, total {} de {}",
            self.pool.name(),
            u.pool,
            self.caps.pool(self.pool),
            u.total,
            self.caps.total
        ))
    }
}

fn used(db: &Connection, pool: Pool, now: i64) -> Result<Used> {
    let get = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> Result<u64> {
        Ok(db.query_row(sql, p, |r| r.get::<_, Option<i64>>(0)).optional()?.flatten().unwrap_or(0) as u64)
    };
    Ok(Used {
        pool: get(
            "SELECT units FROM cu_ledger_day WHERE day = date(?1, 'unixepoch') AND pool = ?2",
            &[&now, &pool.name()],
        )?,
        total: get("SELECT SUM(units) FROM cu_ledger_day WHERE day = date(?1, 'unixepoch')", &[&now])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-22 23:59:59 UTC y el segundo siguiente.
    const T: i64 = 1_792_713_599;

    fn tmp(name: &str) -> String {
        let d = std::env::temp_dir().join(format!("marxol_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_file(&d);
        d.to_string_lossy().into_owned()
    }

    #[test]
    fn dos_procesos_comparten_el_tope_y_el_dia_utc_lo_reinicia() {
        let p = tmp("ledger.db");
        let caps = Caps::new(1000, 700).unwrap();
        let c = Ledger::open(&p, "collect", Pool::Collect, caps, "alchemy", T).unwrap();
        let m = Ledger::open(&p, "tx", Pool::Manual, caps, "alchemy", T).unwrap();
        for _ in 0..17 {
            c.charge(40, T).unwrap();
        }
        assert_eq!(c.remaining(T).unwrap(), 20);
        let e = c.charge(40, T).unwrap_err();
        assert!(e.is::<Exhausted>(), "{e:#}");
        // La bolsa manual no se ve afectada por collect.
        for _ in 0..7 {
            m.charge(40, T).unwrap();
        }
        assert_eq!(m.remaining(T).unwrap(), 20);
        assert!(m.charge(40, T).unwrap_err().is::<Exhausted>());
        assert_eq!(c.used(T).unwrap(), Used { pool: 680, total: 960 });
        // Día nuevo (UTC): todo a cero.
        assert_eq!(c.remaining(T + 1).unwrap(), 700);
        c.charge(40, T + 1).unwrap();
        assert_eq!(m.used(T + 1).unwrap(), Used { pool: 0, total: 40 });
        let runs: Vec<(String, String, i64, i64)> = c
            .db
            .prepare("SELECT command, pool, requests, units FROM cu_ledger_runs ORDER BY run_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(runs, vec![("collect".into(), "collect".into(), 18, 720), ("tx".into(), "manual".into(), 7, 280)]);
    }

    #[test]
    fn total_menor_que_la_cuota_de_collect_es_un_error() {
        assert!(Caps::new(500, 700).is_err());
        assert_eq!(Caps::new(1_000_000, 700_000).unwrap().pool(Pool::Manual), 300_000);
    }

    #[test]
    fn no_abre_como_base_de_recoleccion_una_base_que_no_lo_es() {
        let p = tmp("ajena.db");
        Connection::open(&p).unwrap().execute_batch("CREATE TABLE creations (mint TEXT)").unwrap();
        let before = std::fs::read(&p).unwrap();
        let e = Ledger::open(&p, "tx", Pool::Manual, Caps::new(10, 5).unwrap(), "alchemy", T).err().unwrap();
        assert!(format!("{e:#}").contains("no es una base de recolección"), "{e:#}");
        assert_eq!(std::fs::read(&p).unwrap(), before, "la base ajena no se toca");
    }

    #[test]
    fn base_nueva_en_wal_y_auto_vacuum_incremental() {
        let p = tmp("pragmas.db");
        let db = open_vivo_conn(&p).unwrap();
        let mode: String = db.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        let av: i64 = db.query_row("PRAGMA auto_vacuum", [], |r| r.get(0)).unwrap();
        assert_eq!((mode.as_str(), av), ("wal", 2));
    }

    #[test]
    fn ve_un_collect_vivo_y_no_uno_terminado() {
        let p = tmp("ledger_vivos.db");
        let caps = Caps::new(1000, 500).unwrap();
        // Este proceso de test se llama marxol-…: cuenta como un collect vivo.
        let c = Ledger::open(&p, "collect", Pool::Collect, caps, "alchemy", T).unwrap();
        let m = Ledger::open(&p, "payer-history", Pool::Manual, caps, "alchemy", T).unwrap();
        assert_eq!(m.others_running("collect").unwrap(), vec![std::process::id() as i64]);
        c.finish(T).unwrap();
        assert!(m.others_running("collect").unwrap().is_empty());
        // Un pid que ya no existe no cuenta.
        c.db.execute("INSERT INTO cu_ledger_runs (started_at, command, pool, pid, provider, requests, units) VALUES (0, 'collect', 'collect', 999999999, 'alchemy', 0, 0)", []).unwrap();
        assert!(m.others_running("collect").unwrap().is_empty());
    }
}
