//! Almacenamiento local (SQLite) de eventos de identidad y ciclo de vida.
//!
//! Las tres señales de identidad de creador (CLAUDE.md 5.2) se guardan como
//! filas separadas y nunca se sobreescriben: `creations` (creador original,
//! inmutable) y `creator_changes` (una fila por `SetCreatorEvent` o
//! `MigrateBondingCurveCreatorEvent`, distinguidas por `kind`).
//! Único punto que muta el estado: `Store` (patrón de concurrencia, sección 2).

use crate::idl::Decoded;
use crate::tx::TxEvents;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS creations (
    mint              TEXT PRIMARY KEY,
    bonding_curve     TEXT NOT NULL,
    creator           TEXT NOT NULL,
    user              TEXT NOT NULL,
    name              TEXT,
    symbol            TEXT,
    uri               TEXT,
    quote_mint        TEXT,
    is_mayhem_mode    INTEGER,
    is_cashback       INTEGER,
    creator_fee_bps   INTEGER,
    is_holder_reward  INTEGER,
    timestamp         INTEGER NOT NULL,
    slot              INTEGER NOT NULL,
    signature         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS creations_creator ON creations(creator);

CREATE TABLE IF NOT EXISTS creator_changes (
    kind            TEXT NOT NULL CHECK (kind IN ('set_creator', 'migrate_to_sharing')),
    mint            TEXT NOT NULL,
    bonding_curve   TEXT NOT NULL,
    old_creator     TEXT,            -- solo en migrate_to_sharing
    new_creator     TEXT NOT NULL,
    sharing_config  TEXT,            -- solo en migrate_to_sharing
    timestamp       INTEGER NOT NULL,
    slot            INTEGER NOT NULL,
    signature       TEXT NOT NULL,
    ev_index        INTEGER NOT NULL,
    PRIMARY KEY (signature, ev_index)
);
CREATE INDEX IF NOT EXISTS creator_changes_mint ON creator_changes(mint);
CREATE INDEX IF NOT EXISTS creator_changes_new ON creator_changes(new_creator);

CREATE TABLE IF NOT EXISTS completions (
    mint        TEXT PRIMARY KEY,
    user        TEXT NOT NULL,
    quote_mint  TEXT,
    timestamp   INTEGER NOT NULL,
    slot        INTEGER NOT NULL,
    signature   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS migrations (
    mint               TEXT PRIMARY KEY,
    pool               TEXT NOT NULL,
    quote_mint         TEXT,
    mint_amount        INTEGER,
    quote_amount       INTEGER,  -- campo `sol_amount` del evento; unidades del quote_mint
    pool_migration_fee INTEGER,
    timestamp          INTEGER NOT NULL,
    slot               INTEGER NOT NULL,
    signature          TEXT NOT NULL
);

-- TradeEvent de la ventana temprana de cada token (H1, CLAUDE.md 8). Solo se
-- guardan trades de mints con `creations` conocida y dentro de la ventana.
-- `creator` es el embebido en el evento: el creator vigente en ese trade.
CREATE TABLE IF NOT EXISTS early_trades (
    mint                    TEXT NOT NULL,
    user                    TEXT NOT NULL,
    is_buy                  INTEGER NOT NULL,
    sol_amount              INTEGER,
    quote_amount            INTEGER,
    token_amount            INTEGER,
    quote_mint              TEXT,
    creator                 TEXT,
    ix_name                 TEXT,
    virtual_sol_reserves    INTEGER,
    virtual_token_reserves  INTEGER,
    virtual_quote_reserves  INTEGER,
    timestamp               INTEGER NOT NULL,
    slot                    INTEGER NOT NULL,
    signature               TEXT NOT NULL,
    ev_index                INTEGER NOT NULL,
    PRIMARY KEY (signature, ev_index)
);
CREATE INDEX IF NOT EXISTS early_trades_mint ON early_trades(mint, timestamp);

-- Cobertura de la ventana temprana por mint: solo con complete = 1 están
-- todos los trades de la ventana y H1 es evaluable.
CREATE TABLE IF NOT EXISTS trade_windows (
    mint          TEXT PRIMARY KEY,
    window_secs   INTEGER NOT NULL,
    sigs_in_window INTEGER NOT NULL,
    fetch_errors  INTEGER NOT NULL,
    complete      INTEGER NOT NULL,
    note          TEXT,
    fetched_at    INTEGER NOT NULL
);

-- Puntos de precio de la bonding curve para H1b: reservas virtuales tras
-- cada TradeEvent (kind = 'trade') y las iniciales del CreateEvent
-- (kind = 'create'), en unidades del quote_mint del token.
CREATE TABLE IF NOT EXISTS price_points (
    mint            TEXT NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('create', 'trade')),
    quote_reserves  INTEGER NOT NULL,
    token_reserves  INTEGER NOT NULL,
    timestamp       INTEGER NOT NULL,
    slot            INTEGER NOT NULL,
    signature       TEXT NOT NULL,
    ev_index        INTEGER NOT NULL,
    PRIMARY KEY (signature, ev_index)
);
CREATE INDEX IF NOT EXISTS price_points_mint ON price_points(mint, timestamp);

-- Intentos de descarga por propósito (`window`, `prices`) y mint
-- (validación 3, adenda 3: "sin datos" tras 3 intentos). Cuenta los intentos
-- registrados desde que existe la tabla.
CREATE TABLE IF NOT EXISTS download_attempts (
    purpose   TEXT NOT NULL,
    mint      TEXT NOT NULL,
    attempts  INTEGER NOT NULL,
    PRIMARY KEY (purpose, mint)
);

-- Indexado de un tramo por tiempo (`index-tramo --until`): rango de blockTime
-- listado y si quedó completo (validación 3, adenda 3 y 4).
CREATE TABLE IF NOT EXISTS tramo_index (
    from_ts      INTEGER NOT NULL,
    until_bt     INTEGER NOT NULL,
    complete     INTEGER NOT NULL,
    creations    INTEGER NOT NULL,
    ts_mismatch  INTEGER NOT NULL,
    recorded_at  INTEGER NOT NULL,
    PRIMARY KEY (from_ts, until_bt)
);

-- Cobertura de la ventana de precio (H1b) por mint.
CREATE TABLE IF NOT EXISTS price_windows (
    mint           TEXT PRIMARY KEY,
    horizon_secs   INTEGER NOT NULL,
    sigs_in_window INTEGER NOT NULL,
    fetch_errors   INTEGER NOT NULL,
    complete       INTEGER NOT NULL,
    note           TEXT,
    fetched_at     INTEGER NOT NULL
);

-- Resultado derivado de H1 por variante del criterio (v1, h1c...):
-- recalculable, se reemplaza. No es una señal de la chain, así que
-- sobrescribir no viola el principio de 5.2. Sustituye a `h1_signals`
-- (sin variante), que se borra al abrir: era derivada.
DROP TABLE IF EXISTS h1_signals;
CREATE TABLE IF NOT EXISTS h1_results (
    mint              TEXT NOT NULL,
    variant           TEXT NOT NULL,
    params            TEXT NOT NULL,
    trades            INTEGER NOT NULL,
    creator_trades    INTEGER NOT NULL,
    bot_wallets       INTEGER NOT NULL,
    bot_trades        INTEGER NOT NULL,
    fast_pair_wallets INTEGER NOT NULL,
    whale_trades      INTEGER NOT NULL,
    wallets_remaining INTEGER NOT NULL,
    organic_wallets   INTEGER NOT NULL,
    signal            INTEGER NOT NULL,
    computed_at       INTEGER NOT NULL,
    PRIMARY KEY (mint, variant)
);

-- Firmas ya procesadas, para no repetir getTransaction (cada uno cuesta CU).
CREATE TABLE IF NOT EXISTS seen_signatures (
    signature TEXT PRIMARY KEY,
    slot      INTEGER NOT NULL
);

-- Firmas ya descargadas e ingeridas por `windows` ('window') o `prices`
-- ('prices'), para reanudar sin repetir getTransaction. Es aparte de
-- `seen_signatures` porque una firma vista por `index` antes de conocer la
-- creación del mint no tiene sus trades ni sus precios guardados.
CREATE TABLE IF NOT EXISTS fetched_sigs (
    purpose    TEXT NOT NULL,
    signature  TEXT NOT NULL,
    PRIMARY KEY (purpose, signature)
);

-- Peticiones RPC reales hechas por cada ejecución de `windows`/`prices`.
CREATE TABLE IF NOT EXISTS rpc_usage (
    run_at            INTEGER NOT NULL,
    command           TEXT NOT NULL,
    provider          TEXT NOT NULL,
    get_transaction   INTEGER NOT NULL,
    get_signatures    INTEGER NOT NULL,
    units             INTEGER NOT NULL,  -- CU (Alchemy) o créditos (Helius); 0 en RPC público
    tokens            INTEGER NOT NULL
);
"#;

/// Columnas añadidas después de crear la tabla (bases antiguas).
const MIGRATIONS: &[(&str, &str, &str)] = &[
    ("early_trades", "fee_basis_points", "INTEGER"),
    ("early_trades", "creator_fee_basis_points", "INTEGER"),
];

pub struct Store {
    db: Connection,
}

#[derive(Debug, Default)]
pub struct Ingested {
    pub creations: usize,
    pub creator_changes: usize,
    pub completions: usize,
    pub migrations: usize,
    pub early_trades: usize,
}

/// Ventana temprana de H1 en segundos (CLAUDE.md 8, valor [P]).
pub const EARLY_WINDOW_SECS: i64 = crate::h1::PARAMS_V1.window_secs;

/// Todo lo que necesitan `entry2::compute` y el resultado primario de un
/// token con ventana temprana completa.
pub struct Entry2Input {
    pub mint: String,
    pub t0: i64,
    pub create_slot: u64,
    pub initial: Option<crate::h1b::Point>,
    pub trades: Vec<crate::entry::Trade>,
    pub creator_ids: std::collections::HashSet<String>,
    /// Horizonte de la ventana de precio si está completa.
    pub price_horizon: Option<i64>,
    pub points: Vec<crate::h1b::Point>,
    /// Slot de cada punto de `points` (mismo orden).
    pub point_slots: Vec<u64>,
    pub completed_at: Option<i64>,
    pub migrated_at: Option<i64>,
}

pub struct H1bInput {
    pub mint: String,
    pub t0: i64,
    pub h1: bool,
    pub initial: crate::h1b::Point,
    pub points: Vec<crate::h1b::Point>,
    pub completed_at: Option<i64>,
    pub migrated_at: Option<i64>,
}

/// Token cuya ventana temprana está cerrada y aún no se ha descargado.
pub struct PendingWindow {
    pub mint: String,
    pub bonding_curve: String,
    pub created_at: i64,
    pub signature: String,
}

impl Store {
    pub fn open(path: &str) -> Result<Self> {
        let db = Connection::open(path).with_context(|| format!("abriendo {path}"))?;
        db.execute_batch(SCHEMA)?;
        for (table, col, ty) in MIGRATIONS {
            let exists: bool = db.query_row(
                &format!("SELECT COUNT(*) > 0 FROM pragma_table_info('{table}') WHERE name = ?1"),
                [col],
                |r| r.get(0),
            )?;
            if !exists {
                db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {ty}"))?;
            }
        }
        Ok(Store { db })
    }

    /// Creaciones con slot en `[lo, hi]`, ordenadas por (slot, mint):
    /// (slot, mint, timestamp).
    pub fn creations_in_slots(&self, lo: u64, hi: u64) -> Result<Vec<(u64, String, i64)>> {
        let mut q = self.db.prepare(
            "SELECT slot, mint, timestamp FROM creations WHERE slot BETWEEN ?1 AND ?2 ORDER BY slot, mint",
        )?;
        let rows = q
            .query_map(params![lo as i64, hi as i64], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn seen(&self, sig: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row("SELECT 1 FROM seen_signatures WHERE signature = ?1", [sig], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// Guarda los eventos relevantes de una transacción y la marca como vista.
    pub fn ingest(&mut self, t: &TxEvents) -> Result<Ingested> {
        let tx = self.db.transaction()?;
        let mut n = Ingested::default();
        let s = |e: &Decoded, k: &str| e.str(k).map(str::to_owned);
        let req = |e: &Decoded, k: &str| {
            e.str(k).map(str::to_owned).with_context(|| format!("{}.{k} ausente", e.name))
        };
        let ts = |e: &Decoded| e.i64("timestamp").with_context(|| format!("{}.timestamp", e.name));
        if !t.failed {
            for (i, e) in t.events.iter().enumerate() {
                match e.name.as_str() {
                    "CreateEvent" => {
                        n.creations += tx.execute(
                            "INSERT OR IGNORE INTO creations VALUES
                             (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                            params![
                                req(e, "mint")?,
                                req(e, "bonding_curve")?,
                                req(e, "creator")?,
                                req(e, "user")?,
                                s(e, "name"),
                                s(e, "symbol"),
                                s(e, "uri"),
                                s(e, "quote_mint"),
                                e.bool("is_mayhem_mode"),
                                e.bool("is_cashback_enabled"),
                                e.u64("creator_fee_bps").map(|v| v as i64),
                                e.bool("is_holder_reward"),
                                ts(e)?,
                                t.slot as i64,
                                t.signature
                            ],
                        )?;
                    }
                    "SetCreatorEvent" | "MigrateBondingCurveCreatorEvent" => {
                        let (kind, new_creator) = if e.name == "SetCreatorEvent" {
                            ("set_creator", req(e, "creator")?)
                        } else {
                            ("migrate_to_sharing", req(e, "new_creator")?)
                        };
                        n.creator_changes += tx.execute(
                            "INSERT OR IGNORE INTO creator_changes VALUES
                             (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                            params![
                                kind,
                                req(e, "mint")?,
                                req(e, "bonding_curve")?,
                                s(e, "old_creator"),
                                new_creator,
                                s(e, "sharing_config"),
                                ts(e)?,
                                t.slot as i64,
                                t.signature,
                                i as i64
                            ],
                        )?;
                    }
                    "CompleteEvent" => {
                        n.completions += tx.execute(
                            "INSERT OR IGNORE INTO completions VALUES (?1,?2,?3,?4,?5,?6)",
                            params![
                                req(e, "mint")?,
                                req(e, "user")?,
                                s(e, "quote_mint"),
                                ts(e)?,
                                t.slot as i64,
                                t.signature
                            ],
                        )?;
                    }
                    "CompletePumpAmmMigrationEvent" => {
                        n.migrations += tx.execute(
                            "INSERT OR IGNORE INTO migrations VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                            params![
                                req(e, "mint")?,
                                req(e, "pool")?,
                                s(e, "quote_mint"),
                                e.u64("mint_amount").map(|v| v as i64),
                                e.u64("sol_amount").map(|v| v as i64),
                                e.u64("pool_migration_fee").map(|v| v as i64),
                                ts(e)?,
                                t.slot as i64,
                                t.signature
                            ],
                        )?;
                    }
                    "TradeEvent" => {
                        let mint = req(e, "mint")?;
                        let t0: Option<i64> = tx
                            .query_row("SELECT timestamp FROM creations WHERE mint = ?1", [&mint], |r| r.get(0))
                            .optional()?;
                        let ts = ts(e)?;
                        if !t0.is_some_and(|t0| (t0..t0 + EARLY_WINDOW_SECS).contains(&ts)) {
                            continue;
                        }
                        let u = |k: &str| e.u64(k).map(|v| v as i64);
                        n.early_trades += tx.execute(
                            "INSERT OR IGNORE INTO early_trades
                             (mint, user, is_buy, sol_amount, quote_amount, token_amount, quote_mint,
                              creator, ix_name, virtual_sol_reserves, virtual_token_reserves,
                              virtual_quote_reserves, timestamp, slot, signature, ev_index,
                              fee_basis_points, creator_fee_basis_points)
                             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                            params![
                                mint,
                                req(e, "user")?,
                                e.bool("is_buy").context("TradeEvent.is_buy ausente")?,
                                u("sol_amount"),
                                u("quote_amount"),
                                u("token_amount"),
                                s(e, "quote_mint"),
                                s(e, "creator"),
                                s(e, "ix_name"),
                                u("virtual_sol_reserves"),
                                u("virtual_token_reserves"),
                                u("virtual_quote_reserves"),
                                ts,
                                t.slot as i64,
                                t.signature,
                                i as i64,
                                u("fee_basis_points"),
                                u("creator_fee_basis_points")
                            ],
                        )?;
                    }
                    _ => {}
                }
            }
        }
        tx.execute(
            "INSERT OR IGNORE INTO seen_signatures VALUES (?1, ?2)",
            params![t.signature, t.slot as i64],
        )?;
        tx.commit()?;
        Ok(n)
    }

    pub fn mark_seen(&self, sig: &str, slot: u64) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO seen_signatures VALUES (?1, ?2)",
            params![sig, slot as i64],
        )?;
        Ok(())
    }

    /// Informe de un operador: tokens que creó originalmente y tokens cuya
    /// identidad de creador llegó a él por reasignación, por separado.
    pub fn operator_report(&self, creator: &str) -> Result<OperatorReport> {
        let mut q = self.db.prepare(
            "SELECT c.mint, c.symbol, c.timestamp, c.user,
                    cp.timestamp, m.timestamp, m.pool,
                    (SELECT COUNT(*) FROM creator_changes x WHERE x.mint = c.mint)
             FROM creations c
             LEFT JOIN completions cp ON cp.mint = c.mint
             LEFT JOIN migrations m   ON m.mint  = c.mint
             WHERE c.creator = ?1
             ORDER BY c.timestamp",
        )?;
        let created = q
            .query_map([creator], |r| {
                Ok(TokenRow {
                    mint: r.get(0)?,
                    symbol: r.get(1)?,
                    created_at: r.get(2)?,
                    payer: r.get(3)?,
                    completed_at: r.get(4)?,
                    migrated_at: r.get(5)?,
                    pool: r.get(6)?,
                    creator_changes: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut q = self.db.prepare(
            "SELECT kind, mint, old_creator, new_creator, sharing_config, timestamp, signature
             FROM creator_changes
             WHERE new_creator = ?1 OR old_creator = ?1
             ORDER BY timestamp",
        )?;
        let changes = q
            .query_map([creator], |r| {
                Ok(ChangeRow {
                    kind: r.get(0)?,
                    mint: r.get(1)?,
                    old_creator: r.get(2)?,
                    new_creator: r.get(3)?,
                    sharing_config: r.get(4)?,
                    timestamp: r.get(5)?,
                    signature: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut q = self.db.prepare(
            "SELECT mint, symbol, creator, timestamp, signature FROM creations
             WHERE user = ?1 AND creator != ?1 ORDER BY timestamp",
        )?;
        let paid_for_others = q
            .query_map([creator], |r| {
                Ok(PaidRow {
                    mint: r.get(0)?,
                    symbol: r.get(1)?,
                    creator: r.get(2)?,
                    timestamp: r.get(3)?,
                    signature: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(OperatorReport { creator: creator.to_string(), created, changes, paid_for_others })
    }

    /// Creaciones con la ventana temprana ya cerrada en `now` y sin descarga
    /// completa de sus trades, más antiguas primero.
    pub fn pending_windows(&self, now: i64, limit: usize) -> Result<Vec<PendingWindow>> {
        let mut q = self.db.prepare(
            "SELECT c.mint, c.bonding_curve, c.timestamp, c.signature FROM creations c
             LEFT JOIN trade_windows w ON w.mint = c.mint
             WHERE c.timestamp + ?1 <= ?2 AND COALESCE(w.complete, 0) = 0
             ORDER BY c.timestamp LIMIT ?3",
        )?;
        let rows = q.query_map(params![EARLY_WINDOW_SECS, now, limit as i64], |r| {
            Ok(PendingWindow {
                mint: r.get(0)?,
                bonding_curve: r.get(1)?,
                created_at: r.get(2)?,
                signature: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn record_window(
        &self,
        mint: &str,
        sigs_in_window: usize,
        fetch_errors: usize,
        complete: bool,
        note: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO trade_windows VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![mint, EARLY_WINDOW_SECS, sigs_in_window as i64, fetch_errors as i64, complete, note, now],
        )?;
        self.note_attempt("window", mint)
    }

    /// Suma un intento de descarga (`purpose` = `window` o `prices`).
    pub fn note_attempt(&self, purpose: &str, mint: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO download_attempts VALUES (?1, ?2, 1)
             ON CONFLICT (purpose, mint) DO UPDATE SET attempts = attempts + 1",
            params![purpose, mint],
        )?;
        Ok(())
    }

    pub fn attempts(&self, purpose: &str, mint: &str) -> Result<i64> {
        Ok(self
            .db
            .query_row("SELECT attempts FROM download_attempts WHERE purpose = ?1 AND mint = ?2", params![purpose, mint], |r| r.get(0))
            .optional()?
            .unwrap_or(0))
    }

    /// Registra el resultado de `index-tramo --until` (se reemplaza en cada pasada).
    pub fn record_tramo(&self, from: i64, until_bt: i64, complete: bool, creations: usize, mismatch: usize, now: i64) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO tramo_index VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![from, until_bt, complete, creations as i64, mismatch as i64, now],
        )?;
        Ok(())
    }

    /// Indexado completo que cubre [from, until + margen) por blockTime:
    /// (until_bt, diferencias timestamp/blockTime) del mayor `until_bt`.
    pub fn tramo_complete(&self, from: i64, min_until_bt: i64) -> Result<Option<(i64, i64)>> {
        Ok(self
            .db
            .query_row(
                "SELECT until_bt, ts_mismatch FROM tramo_index
                 WHERE complete = 1 AND from_ts <= ?1 AND until_bt >= ?2 ORDER BY until_bt DESC LIMIT 1",
                params![from, min_until_bt],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// (creaciones, con ventana cerrada, descargadas completas, intentadas sin completar)
    pub fn window_coverage(&self, now: i64) -> Result<(i64, i64, i64, i64)> {
        Ok(self.db.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(c.timestamp + ?1 <= ?2), 0),
                    COALESCE(SUM(w.complete = 1), 0),
                    COALESCE(SUM(w.complete = 0), 0)
             FROM creations c LEFT JOIN trade_windows w ON w.mint = c.mint",
            params![EARLY_WINDOW_SECS, now],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?)
    }

    pub fn creation_time(&self, mint: &str) -> Result<i64> {
        Ok(self.db.query_row("SELECT timestamp FROM creations WHERE mint = ?1", [mint], |r| r.get(0))?)
    }

    /// Mints con la ventana temprana descargada completa (H1 evaluable).
    pub fn evaluable_mints(&self) -> Result<Vec<(String, i64)>> {
        let mut q = self.db.prepare(
            "SELECT c.mint, c.timestamp FROM creations c
             JOIN trade_windows w ON w.mint = c.mint
             WHERE w.complete = 1 ORDER BY c.timestamp",
        )?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Trades de la ventana `[t0, t0 + window)`. Tamaño en unidades del
    /// `quote_mint`: `quote_amount` si viene y es > 0, si no `sol_amount`.
    pub fn window_trades(&self, mint: &str, t0: i64, window: i64) -> Result<Vec<crate::h1::Trade>> {
        let mut q = self.db.prepare(
            "SELECT user, is_buy, COALESCE(NULLIF(quote_amount, 0), sol_amount, 0), timestamp
             FROM early_trades WHERE mint = ?1 AND timestamp >= ?2 AND timestamp < ?3
             ORDER BY slot, timestamp, signature, ev_index",
        )?;
        let rows = q.query_map(params![mint, t0, t0 + window], |r| {
            Ok(crate::h1::Trade {
                user: r.get(0)?,
                is_buy: r.get(1)?,
                size: r.get::<_, i64>(2)? as u64,
                timestamp: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn is_mayhem(&self, mint: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row("SELECT COALESCE(is_mayhem_mode, 0) FROM creations WHERE mint = ?1", [mint], |r| r.get(0))?)
    }

    /// `creator` original del `CreateEvent` (identidad de operador).
    pub fn creation_creator(&self, mint: &str) -> Result<String> {
        Ok(self.db.query_row("SELECT creator FROM creations WHERE mint = ?1", [mint], |r| r.get(0))?)
    }

    pub fn creation_slot(&self, mint: &str) -> Result<u64> {
        Ok(self.db.query_row("SELECT slot FROM creations WHERE mint = ?1", [mint], |r| r.get::<_, i64>(0))? as u64)
    }

    /// Trades de la ventana temprana con slot y reservas virtuales tras cada
    /// uno, para T_entry y H4–H7. Reservas de quote como en `price_points`.
    pub fn entry_trades(&self, mint: &str, t0: i64, window: i64) -> Result<Vec<crate::entry::Trade>> {
        let mut q = self.db.prepare(
            "SELECT user, is_buy, COALESCE(token_amount, 0), slot, timestamp,
                    COALESCE(NULLIF(virtual_quote_reserves, 0), virtual_sol_reserves, 0),
                    COALESCE(virtual_token_reserves, 0),
                    fee_basis_points + creator_fee_basis_points
             FROM early_trades WHERE mint = ?1 AND timestamp >= ?2 AND timestamp < ?3
             ORDER BY slot, timestamp, signature, ev_index",
        )?;
        let rows = q.query_map(params![mint, t0, t0 + window], |r| {
            Ok(crate::entry::Trade {
                user: r.get(0)?,
                is_buy: r.get(1)?,
                token_amount: r.get::<_, i64>(2)? as u64,
                slot: r.get::<_, i64>(3)? as u64,
                timestamp: r.get(4)?,
                quote_reserves: r.get::<_, i64>(5)? as u64,
                token_reserves: r.get::<_, i64>(6)? as u64,
                fee_bps: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Unión de todas las identidades de creador conocidas del mint: creator
    /// original, payer de la creación, cambios de creador y el creator
    /// embebido en cada trade guardado (CLAUDE.md 8, H1 paso 2).
    pub fn creator_identities(&self, mint: &str) -> Result<std::collections::HashSet<String>> {
        let mut q = self.db.prepare(
            "SELECT creator FROM creations WHERE mint = ?1
             UNION SELECT user FROM creations WHERE mint = ?1
             UNION SELECT new_creator FROM creator_changes WHERE mint = ?1
             UNION SELECT old_creator FROM creator_changes WHERE mint = ?1 AND old_creator IS NOT NULL
             UNION SELECT creator FROM early_trades WHERE mint = ?1 AND creator IS NOT NULL",
        )?;
        let rows = q.query_map([mint], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn save_h1(
        &self,
        mint: &str,
        variant: &str,
        params_json: &str,
        r: &crate::h1::H1Result,
        now: i64,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO h1_results VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                mint,
                variant,
                params_json,
                r.trades as i64,
                r.creator_trades as i64,
                r.bot_wallets as i64,
                r.bot_trades as i64,
                r.fast_pair_wallets as i64,
                r.whale_trades as i64,
                r.wallets_remaining as i64,
                r.organic_wallets as i64,
                r.signal,
                now
            ],
        )?;
        Ok(())
    }

    /// Guarda como puntos de precio el `CreateEvent` y los `TradeEvent` de
    /// mints conocidos con timestamp en `[t0, t0 + horizon)`. Complementa a
    /// `ingest`, que sigue guardando el resto de eventos de la tx.
    pub fn ingest_prices(&mut self, t: &TxEvents, horizon: i64) -> Result<usize> {
        let tx = self.db.transaction()?;
        let mut n = 0;
        for (i, e) in t.events.iter().enumerate() {
            let kind = match e.name.as_str() {
                "CreateEvent" => "create",
                "TradeEvent" => "trade",
                _ => continue,
            };
            let (Some(mint), Some(ts)) = (e.str("mint"), e.i64("timestamp")) else { continue };
            let t0: Option<i64> = tx
                .query_row("SELECT timestamp FROM creations WHERE mint = ?1", [mint], |r| r.get(0))
                .optional()?;
            if !t0.is_some_and(|t0| (t0..t0 + horizon).contains(&ts)) {
                continue;
            }
            // Con quote SOL ambos campos coinciden; con otro quote solo viene
            // virtual_quote_reserves (CLAUDE.md 5.5).
            let q = e.u64("virtual_quote_reserves").filter(|&v| v > 0).or(e.u64("virtual_sol_reserves"));
            let (Some(q), Some(tok)) = (q, e.u64("virtual_token_reserves")) else { continue };
            n += tx.execute(
                "INSERT OR IGNORE INTO price_points VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![mint, kind, q as i64, tok as i64, ts, t.slot as i64, t.signature, i as i64],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Copia a `price_points` los trades ya descargados de la ventana temprana,
    /// para no volver a pedir esas transacciones.
    pub fn backfill_prices_from_early_trades(&self) -> Result<usize> {
        Ok(self.db.execute(
            "INSERT OR IGNORE INTO price_points
             SELECT mint, 'trade', COALESCE(NULLIF(virtual_quote_reserves, 0), virtual_sol_reserves),
                    virtual_token_reserves, timestamp, slot, signature, ev_index
             FROM early_trades
             WHERE virtual_token_reserves IS NOT NULL
               AND COALESCE(NULLIF(virtual_quote_reserves, 0), virtual_sol_reserves) IS NOT NULL",
            [],
        )?)
    }

    /// Tokens con ventana temprana completa, ventana de precio cerrada en
    /// `now` y sin descarga completa de precio (con cualquier horizonte).
    pub fn pending_price_windows(&self, now: i64, horizon: i64, limit: usize) -> Result<Vec<PendingWindow>> {
        let mut q = self.db.prepare(
            "SELECT c.mint, c.bonding_curve, c.timestamp, c.signature FROM creations c
             LEFT JOIN price_windows w ON w.mint = c.mint
             WHERE c.timestamp + ?1 <= ?2 AND COALESCE(w.complete, 0) = 0
               AND EXISTS (SELECT 1 FROM trade_windows t WHERE t.mint = c.mint AND t.complete = 1)
             ORDER BY c.timestamp LIMIT ?3",
        )?;
        let rows = q.query_map(params![horizon, now, limit as i64], |r| {
            Ok(PendingWindow {
                mint: r.get(0)?,
                bonding_curve: r.get(1)?,
                created_at: r.get(2)?,
                signature: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_price_window(
        &self,
        mint: &str,
        horizon: i64,
        sigs: usize,
        errors: usize,
        complete: bool,
        note: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO price_windows VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![mint, horizon, sigs as i64, errors as i64, complete, note, now],
        )?;
        self.note_attempt("prices", mint)
    }

    /// Todo lo que necesita `h1b::compute` para un mint con ventana de precio
    /// completa, junto con su señal H1 en la variante dada. Los mints sin
    /// punto `create` se omiten.
    pub fn h1b_inputs(&self, horizon: i64, variant: &str) -> Result<Vec<H1bInput>> {
        let mut q = self.db.prepare(
            "SELECT c.mint, c.timestamp, h.signal, cp.timestamp, m.timestamp
             FROM creations c
             JOIN h1_results h ON h.mint = c.mint AND h.variant = ?2
             JOIN price_windows w ON w.mint = c.mint AND w.horizon_secs >= ?1 AND w.complete = 1
             LEFT JOIN completions cp ON cp.mint = c.mint
             LEFT JOIN migrations m ON m.mint = c.mint
             ORDER BY c.timestamp",
        )?;
        let heads = q
            .query_map(params![horizon, variant], |r| {
                Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<rusqlite::Result<Vec<(String, i64, bool, Option<i64>, Option<i64>)>>>()?;
        let mut pts = self.db.prepare(
            "SELECT kind, timestamp, quote_reserves, token_reserves, slot FROM price_points
             WHERE mint = ?1 ORDER BY timestamp, slot",
        )?;
        let mut out = Vec::new();
        for (mint, t0, h1, completed_at, migrated_at) in heads {
            let rows = pts
                .query_map([&mint], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        crate::h1b::Point {
                            timestamp: r.get(1)?,
                            quote_reserves: r.get::<_, i64>(2)? as u64,
                            token_reserves: r.get::<_, i64>(3)? as u64,
                        },
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let Some(initial) = rows.iter().find(|(k, _)| k == "create").map(|(_, p)| *p) else { continue };
            let points = rows.into_iter().filter(|(k, _)| k == "trade").map(|(_, p)| p).collect();
            out.push(H1bInput { mint, t0, h1, initial, points, completed_at, migrated_at });
        }
        Ok(out)
    }

    pub fn fetched(&self, purpose: &str, sig: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row("SELECT 1 FROM fetched_sigs WHERE purpose = ?1 AND signature = ?2", [purpose, sig], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub fn mark_fetched(&self, purpose: &str, sig: &str) -> Result<()> {
        self.db.execute("INSERT OR IGNORE INTO fetched_sigs VALUES (?1, ?2)", [purpose, sig])?;
        Ok(())
    }

    pub fn record_rpc_usage(&self, command: &str, b: &crate::rpc::Budget, tokens: usize, now: i64) -> Result<()> {
        self.db.execute(
            "INSERT INTO rpc_usage VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                now,
                command,
                b.provider.name,
                b.get_transaction.get() as i64,
                b.get_signatures.get() as i64,
                b.units() as i64,
                tokens as i64
            ],
        )?;
        Ok(())
    }

    /// Consumo acumulado por proveedor: (proveedor, getTransaction, getSignatures, unidades).
    pub fn rpc_usage_totals(&self) -> Result<Vec<(String, i64, i64, i64)>> {
        let mut q = self.db.prepare(
            "SELECT provider, SUM(get_transaction), SUM(get_signatures), SUM(units)
             FROM rpc_usage GROUP BY provider ORDER BY provider",
        )?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Tokens con ventana temprana completa y slot de creación en
    /// `[min_slot, max_slot]`.
    pub fn entry2_inputs(&self, min_slot: u64, max_slot: u64) -> Result<Vec<Entry2Input>> {
        self.entry2_inputs_where("c.slot BETWEEN ?1 AND ?2", min_slot as i64, max_slot.min(i64::MAX as u64) as i64)
    }

    /// Como `entry2_inputs`, con `timestamp` de creación en `[from, until)`
    /// (validación 3: el tramo se cierra por timestamp).
    pub fn entry2_inputs_ts(&self, from: i64, until: i64) -> Result<Vec<Entry2Input>> {
        self.entry2_inputs_where("c.timestamp >= ?1 AND c.timestamp < ?2", from, until)
    }

    /// Creaciones con `timestamp` en `[from, until)`: (slot, mint, timestamp,
    /// ventana temprana completa).
    pub fn creations_in_ts(&self, from: i64, until: i64) -> Result<Vec<(u64, String, i64, bool)>> {
        let mut q = self.db.prepare(
            "SELECT c.slot, c.mint, c.timestamp, EXISTS(SELECT 1 FROM trade_windows w WHERE w.mint = c.mint AND w.complete = 1)
             FROM creations c WHERE c.timestamp >= ?1 AND c.timestamp < ?2 ORDER BY c.slot, c.mint",
        )?;
        let rows = q
            .query_map(params![from, until], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Unión de identidades de creador limitada a lo anterior a `slot`
    /// (descriptivo de la validación 3): creator y payer de la creación,
    /// cambios de creator con slot < `slot` y creator embebido en los trades
    /// guardados con slot < `slot`. No sustituye a `creator_identities`.
    pub fn creator_identities_before(&self, mint: &str, slot: u64) -> Result<std::collections::HashSet<String>> {
        let mut q = self.db.prepare(
            "SELECT creator FROM creations WHERE mint = ?1
             UNION SELECT user FROM creations WHERE mint = ?1
             UNION SELECT new_creator FROM creator_changes WHERE mint = ?1 AND slot < ?2
             UNION SELECT old_creator FROM creator_changes WHERE mint = ?1 AND slot < ?2 AND old_creator IS NOT NULL
             UNION SELECT creator FROM early_trades WHERE mint = ?1 AND slot < ?2 AND creator IS NOT NULL",
        )?;
        let rows = q.query_map(params![mint, slot as i64], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// (firma, timestamp) de la creación de cada mint.
    pub fn creation_signatures_ts(&self, mints: &[String]) -> Result<Vec<(String, i64)>> {
        let mut q = self.db.prepare("SELECT signature, timestamp FROM creations WHERE mint = ?1")?;
        let mut out = Vec::new();
        for m in mints {
            out.push(q.query_row([m], |r| Ok((r.get(0)?, r.get(1)?)))?);
        }
        Ok(out)
    }

    fn entry2_inputs_where(&self, cond: &str, a: i64, b: i64) -> Result<Vec<Entry2Input>> {
        let mut q = self.db.prepare(&format!(
            "SELECT c.mint, c.timestamp, c.slot,
                    (SELECT MAX(pw.horizon_secs) FROM price_windows pw WHERE pw.mint = c.mint AND pw.complete = 1),
                    cp.timestamp, m.timestamp
             FROM creations c
             JOIN trade_windows w ON w.mint = c.mint AND w.complete = 1
             LEFT JOIN completions cp ON cp.mint = c.mint
             LEFT JOIN migrations m ON m.mint = c.mint
             WHERE {cond}
             ORDER BY c.slot, c.timestamp"
        ))?;
        let heads = q
            .query_map(params![a, b], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)? as u64,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut pts = self.db.prepare(
            "SELECT kind, timestamp, quote_reserves, token_reserves, slot FROM price_points
             WHERE mint = ?1 ORDER BY timestamp, slot",
        )?;
        let mut out = Vec::new();
        for (mint, t0, create_slot, price_horizon, completed_at, migrated_at) in heads {
            let rows = pts
                .query_map([&mint], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        crate::h1b::Point {
                            timestamp: r.get(1)?,
                            quote_reserves: r.get::<_, i64>(2)? as u64,
                            token_reserves: r.get::<_, i64>(3)? as u64,
                        },
                        r.get::<_, i64>(4)? as u64,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out.push(Entry2Input {
                trades: self.entry_trades(&mint, t0, EARLY_WINDOW_SECS)?,
                creator_ids: self.creator_identities(&mint)?,
                initial: rows.iter().find(|(k, ..)| k == "create").map(|(_, p, _)| *p),
                point_slots: rows.iter().filter(|(k, ..)| k == "trade").map(|x| x.2).collect(),
                points: rows.into_iter().filter(|(k, ..)| k == "trade").map(|(_, p, _)| p).collect(),
                mint,
                t0,
                create_slot,
                price_horizon,
                completed_at,
                migrated_at,
            });
        }
        Ok(out)
    }

    pub fn counts(&self) -> Result<Vec<(&'static str, i64)>> {
        [
            "creations",
            "creator_changes",
            "completions",
            "migrations",
            "early_trades",
            "trade_windows",
            "h1_results",
            "price_points",
            "price_windows",
            "fetched_sigs",
            "rpc_usage",
            "seen_signatures",
        ]
            .into_iter()
            .map(|t| {
                let n = self.db.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))?;
                Ok((t, n))
            })
            .collect()
    }
}

#[derive(Debug, serde::Serialize)]
pub struct TokenRow {
    pub mint: String,
    pub symbol: Option<String>,
    pub created_at: i64,
    pub payer: String,
    pub completed_at: Option<i64>,
    pub migrated_at: Option<i64>,
    pub pool: Option<String>,
    pub creator_changes: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct ChangeRow {
    pub kind: String,
    pub mint: String,
    pub old_creator: Option<String>,
    pub new_creator: String,
    pub sharing_config: Option<String>,
    pub timestamp: i64,
    pub signature: String,
}

#[derive(Debug, serde::Serialize)]
pub struct OperatorReport {
    pub creator: String,
    pub created: Vec<TokenRow>,
    pub changes: Vec<ChangeRow>,
    /// Creaciones que esta wallet pagó (`user`) con otro `creator` registrado.
    pub paid_for_others: Vec<PaidRow>,
}

#[derive(Debug, serde::Serialize)]
pub struct PaidRow {
    pub mint: String,
    pub symbol: Option<String>,
    pub creator: String,
    pub timestamp: i64,
    pub signature: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn ev(name: &str, fields: Value) -> Decoded {
        let Value::Object(fields) = fields else { unreachable!() };
        Decoded { name: name.into(), fields, missing: vec![], trailing_bytes: 0 }
    }

    fn tx(sig: &str, events: Vec<Decoded>) -> TxEvents {
        TxEvents {
            signature: sig.into(),
            slot: 1,
            block_time: None,
            failed: false,
            instructions: vec![],
            events,
            decode_errors: vec![],
        }
    }

    #[test]
    fn intentos_de_descarga_y_registro_del_tramo() {
        let st = Store::open(":memory:").unwrap();
        assert_eq!(st.attempts("window", "M").unwrap(), 0);
        st.record_window("M", 0, 0, false, Some("x"), 1).unwrap();
        st.record_window("M", 0, 1, false, None, 2).unwrap();
        st.note_attempt("window", "M").unwrap();
        assert_eq!(st.attempts("window", "M").unwrap(), 3);
        st.record_price_window("M", 3900, 0, 0, false, None, 3).unwrap();
        assert_eq!(st.attempts("prices", "M").unwrap(), 1);
        assert_eq!(st.attempts("window", "N").unwrap(), 0);

        assert_eq!(st.tramo_complete(100, 220).unwrap(), None);
        st.record_tramo(100, 220, false, 5, 0, 1).unwrap();
        assert_eq!(st.tramo_complete(100, 220).unwrap(), None, "incompleto no vale");
        st.record_tramo(100, 220, true, 7, 2, 2).unwrap();
        assert_eq!(st.tramo_complete(100, 220).unwrap(), Some((220, 2)));
        assert_eq!(st.tramo_complete(100, 221).unwrap(), None, "no cubre el margen");
        assert_eq!(st.tramo_complete(99, 220).unwrap(), None, "no cubre el inicio");
    }

    #[test]
    fn identidad_original_y_cambios_se_guardan_por_separado() {
        let mut st = Store::open(":memory:").unwrap();
        st.ingest(&tx(
            "s1",
            vec![ev(
                "CreateEvent",
                json!({"mint":"M","bonding_curve":"B","creator":"A","user":"P","timestamp":10}),
            )],
        ))
        .unwrap();
        st.ingest(&tx(
            "s2",
            vec![ev(
                "MigrateBondingCurveCreatorEvent",
                json!({"mint":"M","bonding_curve":"B","old_creator":"A","new_creator":"S",
                       "sharing_config":"S","timestamp":20}),
            )],
        ))
        .unwrap();
        // Reingestar la misma firma no duplica.
        let n = st
            .ingest(&tx(
                "s2",
                vec![ev(
                    "MigrateBondingCurveCreatorEvent",
                    json!({"mint":"M","bonding_curve":"B","old_creator":"A","new_creator":"S",
                           "sharing_config":"S","timestamp":20}),
                )],
            ))
            .unwrap();
        assert_eq!(n.creator_changes, 0);

        let r = st.operator_report("A").unwrap();
        assert_eq!(r.created.len(), 1, "el creador original conserva su token");
        assert_eq!(r.created[0].creator_changes, 1);
        assert_eq!(r.changes[0].kind, "migrate_to_sharing");
        assert!(st.seen("s1").unwrap());
    }

    #[test]
    fn solo_se_guardan_trades_de_la_ventana_temprana() {
        let mut st = Store::open(":memory:").unwrap();
        let trade = |ts: i64, mint: &str| {
            ev(
                "TradeEvent",
                json!({"mint":mint,"user":"U","is_buy":true,"sol_amount":5,"quote_amount":7,
                       "creator":"C2","timestamp":ts}),
            )
        };
        // Creación y compra del creador en la misma tx: la compra ya cuenta.
        st.ingest(&tx(
            "s1",
            vec![
                ev("CreateEvent", json!({"mint":"M","bonding_curve":"B","creator":"A","user":"P","timestamp":100})),
                trade(100, "M"),
            ],
        ))
        .unwrap();
        let n = st
            .ingest(&tx("s2", vec![trade(399, "M"), trade(400, "M"), trade(150, "OTRO")]))
            .unwrap();
        assert_eq!(n.early_trades, 1, "t=400 queda fuera y OTRO no tiene creación");
        let ts = st.window_trades("M", 100, EARLY_WINDOW_SECS).unwrap();
        assert_eq!(ts.len(), 2);
        assert_eq!(ts[0].size, 7, "se usa quote_amount");
        let ids = st.creator_identities("M").unwrap();
        for id in ["A", "P", "C2"] {
            assert!(ids.contains(id), "falta {id}");
        }
    }

    #[test]
    fn comisiones_del_trade_y_firmas_descargadas_para_reanudar() {
        let mut st = Store::open(":memory:").unwrap();
        st.ingest(&tx(
            "s1",
            vec![
                ev("CreateEvent", json!({"mint":"M","bonding_curve":"B","creator":"A","user":"A","timestamp":100})),
                ev(
                    "TradeEvent",
                    json!({"mint":"M","user":"U","is_buy":true,"quote_amount":7,"token_amount":3,
                           "virtual_quote_reserves":10,"virtual_token_reserves":20,
                           "fee_basis_points":95,"creator_fee_basis_points":5,"timestamp":101}),
                ),
            ],
        ))
        .unwrap();
        let ts = st.entry_trades("M", 100, EARLY_WINDOW_SECS).unwrap();
        assert_eq!(ts[0].fee_bps, Some(100));
        assert!(!st.fetched("window", "s1").unwrap());
        st.mark_fetched("window", "s1").unwrap();
        assert!(st.fetched("window", "s1").unwrap());
        assert!(!st.fetched("prices", "s1").unwrap(), "cada propósito lleva su propio registro");
    }
}
