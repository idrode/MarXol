//! Base de recolección continua (`marxol_vivo.db`): esquema propio encima del
//! de `store`, punto de reanudación, muestreo, resultados agregados por token,
//! retención de crudos y exportación.
//!
//! Todo lo que entra aquí es fuera de muestra respecto a `marxol.db`
//! (docs/congelado_vivo_2026-10-22.md). Nunca se guarda la URL del RPC ni
//! ninguna clave: solo el nombre del proveedor (en el libro de CU).

use crate::ledger;
use crate::store::Store;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};

/// Versión del congelado con la que se capturan los tokens.
pub const FROZEN: &str = "congelado_vivo_2026-10-22";
/// Regla de muestreo (se guarda en cada token).
pub const SAMPLE_RULE: &str = "fnv1a64(firma) % 1e6 < ppm";
/// Horizontes de seguimiento del estado de la bonding curve (s).
pub const OUTCOME_HORIZONS: [i64; 4] = [3600, 86_400, 7 * 86_400, 60 * 86_400];

const SCHEMA: &str = r#"
-- Crudo: todas las firmas de la PDA mint-authority listadas (muestreadas o no).
CREATE TABLE IF NOT EXISTS vivo_create_sigs (
    signature   TEXT PRIMARY KEY,
    slot        INTEGER NOT NULL,
    block_time  INTEGER,
    failed      INTEGER NOT NULL,
    sampled     INTEGER NOT NULL,
    listed_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS vivo_create_sigs_bt ON vivo_create_sigs(block_time);
-- Agregado por hora UTC de blockTime (se actualiza al listar; no se borra).
CREATE TABLE IF NOT EXISTS vivo_hourly (
    hour     INTEGER PRIMARY KEY,   -- unix / 3600
    listed   INTEGER NOT NULL,
    failed   INTEGER NOT NULL,
    sampled  INTEGER NOT NULL
);
-- Muestra: una fila por firma de creación muestreada, con su captura.
CREATE TABLE IF NOT EXISTS vivo_tokens (
    signature    TEXT PRIMARY KEY,
    mint         TEXT,
    slot         INTEGER NOT NULL,
    block_time   INTEGER,
    hour_utc     INTEGER,           -- 0-23, de blockTime
    captured_at  INTEGER NOT NULL,  -- cuándo lo listó collect
    sample_ppm   INTEGER NOT NULL,
    sample_rule  TEXT NOT NULL,
    frozen       TEXT NOT NULL,
    status       TEXT NOT NULL CHECK (status IN ('pendiente', 'creado', 'sin_create', 'error')),
    attempts     INTEGER NOT NULL DEFAULT 0,
    note         TEXT,
    updated_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS vivo_tokens_status ON vivo_tokens(status, slot);
CREATE INDEX IF NOT EXISTS vivo_tokens_mint ON vivo_tokens(mint);
-- Creadores vistos en la muestra (no es su historial completo).
CREATE TABLE IF NOT EXISTS vivo_creators (
    creator           TEXT PRIMARY KEY,
    first_seen_slot   INTEGER NOT NULL,
    first_seen_at     INTEGER NOT NULL,
    sampled_launches  INTEGER NOT NULL
);
-- Wallets relacionadas (fase 3: fuente de fondos, etc.). No se borra.
CREATE TABLE IF NOT EXISTS wallet_links (
    src            TEXT NOT NULL,
    dst            TEXT NOT NULL,
    kind           TEXT NOT NULL,
    amount         INTEGER,
    signature      TEXT NOT NULL,
    slot           INTEGER,
    discovered_at  INTEGER NOT NULL,
    PRIMARY KEY (signature, src, dst, kind)
);
-- Estado de la bonding curve a +1 h, +24 h, +7 d y +60 d.
CREATE TABLE IF NOT EXISTS vivo_outcomes (
    mint          TEXT NOT NULL,
    horizon_secs  INTEGER NOT NULL,
    due_at        INTEGER NOT NULL,
    checked_at    INTEGER,
    account       TEXT,             -- 'existe', 'cerrada' u 'otro_owner'
    complete      INTEGER,
    state_json    TEXT,
    PRIMARY KEY (mint, horizon_secs)
);
CREATE INDEX IF NOT EXISTS vivo_outcomes_due ON vivo_outcomes(checked_at, due_at);
-- Resultado agregado por token, con el código congelado. No se borra.
CREATE TABLE IF NOT EXISTS vivo_results (
    mint         TEXT PRIMARY KEY,
    computed_at  INTEGER NOT NULL,
    frozen       TEXT NOT NULL,
    status       TEXT NOT NULL,     -- 'evaluado', 'sin_entrada', 'sin_ventana', 'sin_precio_inicial'
    d2           INTEGER,
    result_json  TEXT
);
-- Una fila por ciclo de collect: CU por etapa y trabajo hecho.
CREATE TABLE IF NOT EXISTS vivo_cycles (
    cycle_id         INTEGER PRIMARY KEY,
    started_at       INTEGER NOT NULL,
    ended_at         INTEGER,
    units_poll       INTEGER NOT NULL DEFAULT 0,
    units_outcomes   INTEGER NOT NULL DEFAULT 0,
    units_create     INTEGER NOT NULL DEFAULT 0,
    units_windows    INTEGER NOT NULL DEFAULT 0,
    units_prices     INTEGER NOT NULL DEFAULT 0,
    pages            INTEGER NOT NULL DEFAULT 0,
    sigs_listed      INTEGER NOT NULL DEFAULT 0,
    sampled          INTEGER NOT NULL DEFAULT 0,
    creates          INTEGER NOT NULL DEFAULT 0,
    windows          INTEGER NOT NULL DEFAULT 0,
    prices           INTEGER NOT NULL DEFAULT 0,
    outcomes         INTEGER NOT NULL DEFAULT 0,
    results          INTEGER NOT NULL DEFAULT 0,
    note             TEXT
);
-- Huecos del listado (más páginas pendientes que el tope de recuperación).
CREATE TABLE IF NOT EXISTS vivo_gaps (
    after_slot   INTEGER,
    before_slot  INTEGER NOT NULL,
    detected_at  INTEGER NOT NULL,
    reason       TEXT NOT NULL
);
-- Descargas de ventana o precio que collect ya no reintenta (desde
-- 2026-10-26). 'incompleto_permanente': fallo determinista (tope de tx por
-- token, max_pages sin llegar a la creación o al rango). 'pendiente_agotado':
-- fallos transitorios (red, 429, decodificación) en 3 intentos. La fila de
-- trade_windows / price_windows sigue con complete = 0 y su nota, así que el
-- token cuenta igual en censura y exclusiones. No se borra.
CREATE TABLE IF NOT EXISTS vivo_download_final (
    purpose    TEXT NOT NULL CHECK (purpose IN ('window', 'prices')),
    mint       TEXT NOT NULL,
    state      TEXT NOT NULL CHECK (state IN ('incompleto_permanente', 'pendiente_agotado')),
    reason     TEXT NOT NULL,
    attempts   INTEGER NOT NULL,
    marked_at  INTEGER NOT NULL,
    PRIMARY KEY (purpose, mint)
);
-- Exportaciones verificadas (requisito para borrar crudos).
CREATE TABLE IF NOT EXISTS vivo_exports (
    id           INTEGER PRIMARY KEY,
    table_name   TEXT NOT NULL,
    from_ts      INTEGER NOT NULL,
    until_ts     INTEGER NOT NULL,
    rows         INTEGER NOT NULL,
    path         TEXT NOT NULL,
    bytes        INTEGER NOT NULL,
    sha256       TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    verified_at  INTEGER NOT NULL
);
"#;

/// Abre (o crea) la base de recolección con los ajustes de `ledger` (WAL,
/// auto_vacuum incremental) y los dos esquemas.
pub fn open(path: &str, now: i64) -> Result<Store> {
    let db = ledger::open_vivo_conn(path)?;
    db.execute_batch(SCHEMA)?;
    db.execute("INSERT OR IGNORE INTO vivo_meta VALUES ('creada', ?1)", [now.to_string()])?;
    db.execute("INSERT OR IGNORE INTO vivo_meta VALUES ('fuera_de_muestra', ?1)", [FROZEN])?;
    Store::from_conn(db)
}

pub fn meta(db: &Connection, key: &str) -> Result<Option<String>> {
    Ok(db.query_row("SELECT value FROM vivo_meta WHERE key = ?1", [key], |r| r.get(0)).optional()?)
}

pub fn set_meta(db: &Connection, key: &str, value: &str) -> Result<()> {
    db.execute("INSERT INTO vivo_meta VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2", [key, value])?;
    Ok(())
}

/// FNV-1a de 64 bits: las firmas son aleatorias, así que basta para un
/// muestreo uniforme, reproducible y sin depender de la hora.
pub fn fnv1a64(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

pub fn sampled(signature: &str, ppm: u32) -> bool {
    fnv1a64(signature) % 1_000_000 < ppm as u64
}

/// Punto de reanudación: la firma más nueva ya listada y su slot.
#[derive(Debug, Clone, PartialEq)]
pub struct Cursor {
    pub signature: String,
    pub slot: u64,
}

pub fn cursor(db: &Connection) -> Result<Option<Cursor>> {
    let (Some(signature), Some(slot)) = (meta(db, "cursor_signature")?, meta(db, "cursor_slot")?) else {
        return Ok(None);
    };
    Ok(Some(Cursor { signature, slot: slot.parse()? }))
}

/// Guarda en una transacción las firmas listadas (de más nueva a más
/// antigua), los agregados por hora, los tokens muestreados y el nuevo
/// punto de reanudación. `gap`: el listado no llegó al cursor anterior.
/// Devuelve cuántas firmas se muestrearon.
pub fn record_listing(
    st: &mut Store,
    sigs: &[crate::tx::SigInfo],
    ppm: u32,
    gap: Option<&str>,
    now: i64,
) -> Result<usize> {
    let prev = cursor(st.conn())?;
    let tx = st.conn_mut().transaction()?;
    let mut n = 0;
    {
        let mut ins = tx.prepare(
            "INSERT OR IGNORE INTO vivo_create_sigs (signature, slot, block_time, failed, sampled, listed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        let mut hour = tx.prepare(
            "INSERT INTO vivo_hourly (hour, listed, failed, sampled) VALUES (?1, 1, ?2, ?3)
             ON CONFLICT (hour) DO UPDATE SET listed = listed + 1, failed = failed + ?2, sampled = sampled + ?3",
        )?;
        let mut tok = tx.prepare(
            "INSERT OR IGNORE INTO vivo_tokens
             (signature, slot, block_time, hour_utc, captured_at, sample_ppm, sample_rule, frozen, status, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pendiente', ?5)",
        )?;
        for s in sigs {
            let take = !s.failed && sampled(&s.signature, ppm);
            let added = ins.execute(params![s.signature, s.slot as i64, s.block_time, s.failed, take, now])?;
            if added == 0 {
                continue;
            }
            if let Some(bt) = s.block_time {
                hour.execute(params![bt.div_euclid(3600), s.failed, take])?;
            }
            if take {
                let h = s.block_time.map(|bt| bt.rem_euclid(86_400) / 3600);
                tok.execute(params![s.signature, s.slot as i64, s.block_time, h, now, ppm, SAMPLE_RULE, FROZEN])?;
                n += 1;
            }
        }
    }
    if let Some(reason) = gap {
        let before = sigs.last().map(|s| s.slot as i64).unwrap_or(0);
        tx.execute(
            "INSERT INTO vivo_gaps (after_slot, before_slot, detected_at, reason) VALUES (?1, ?2, ?3, ?4)",
            params![prev.as_ref().map(|c| c.slot as i64), before, now, reason],
        )?;
    }
    if let Some(newest) = sigs.first() {
        for (k, v) in [("cursor_signature", newest.signature.clone()), ("cursor_slot", newest.slot.to_string())] {
            tx.execute("INSERT INTO vivo_meta VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2", [k, &v])?;
        }
    }
    tx.commit()?;
    Ok(n)
}

/// Tokens muestreados aún sin tx de creación descargada, del más antiguo al
/// más nuevo: (firma, intentos).
pub fn pending_creates(db: &Connection, limit: usize) -> Result<Vec<(String, i64)>> {
    let mut q = db.prepare(
        "SELECT signature, attempts FROM vivo_tokens WHERE status = 'pendiente' ORDER BY slot, signature LIMIT ?1",
    )?;
    let rows = q.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Marca la creación descargada: mint, creador en la muestra y seguimiento
/// del estado de la curva a los horizontes de `OUTCOME_HORIZONS`.
pub fn mark_created(db: &Connection, sig: &str, mint: &str, creator: &str, slot: u64, t0: i64, now: i64) -> Result<()> {
    db.execute(
        "UPDATE vivo_tokens SET status = 'creado', mint = ?2, note = NULL, updated_at = ?3 WHERE signature = ?1",
        params![sig, mint, now],
    )?;
    db.execute(
        "INSERT INTO vivo_creators (creator, first_seen_slot, first_seen_at, sampled_launches) VALUES (?1, ?2, ?3, 1)
         ON CONFLICT (creator) DO UPDATE SET sampled_launches = sampled_launches + 1",
        params![creator, slot as i64, t0],
    )?;
    for h in OUTCOME_HORIZONS {
        db.execute(
            "INSERT OR IGNORE INTO vivo_outcomes (mint, horizon_secs, due_at) VALUES (?1, ?2, ?3)",
            params![mint, h, t0 + h],
        )?;
    }
    Ok(())
}

/// Fallo o tx sin `CreateEvent`: tras 3 intentos fallidos queda en `error`.
pub fn mark_create_failed(db: &Connection, sig: &str, status: &str, note: &str, now: i64) -> Result<()> {
    db.execute(
        "UPDATE vivo_tokens SET attempts = attempts + 1, note = ?3, updated_at = ?4,
                status = CASE WHEN ?2 = 'sin_create' THEN 'sin_create'
                              WHEN attempts + 1 >= 3 THEN 'error' ELSE 'pendiente' END
         WHERE signature = ?1",
        params![sig, status, note, now],
    )?;
    Ok(())
}

/// Intentos máximos de una descarga con fallos transitorios antes de
/// marcarla `pendiente_agotado`.
pub const MAX_TRANSIENT_ATTEMPTS: i64 = 3;

/// Estado final de una descarga de ventana o precio que collect no reintenta.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DownloadFinal {
    /// Fallo determinista: reintentar daría lo mismo.
    Permanent,
    /// Fallos transitorios en `MAX_TRANSIENT_ATTEMPTS` intentos.
    Exhausted,
}

impl DownloadFinal {
    pub fn name(self) -> &'static str {
        match self {
            DownloadFinal::Permanent => "incompleto_permanente",
            DownloadFinal::Exhausted => "pendiente_agotado",
        }
    }
}

/// Marca una descarga (`purpose` = `window` o `prices`) como final, con su
/// motivo y los intentos registrados en `download_attempts`.
pub fn mark_download_final(db: &Connection, purpose: &str, mint: &str, state: DownloadFinal, reason: &str, now: i64) -> Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO vivo_download_final (purpose, mint, state, reason, attempts, marked_at)
         VALUES (?1, ?2, ?3, ?4,
                 COALESCE((SELECT attempts FROM download_attempts WHERE purpose = ?1 AND mint = ?2), 0), ?5)",
        params![purpose, mint, state.name(), reason, now],
    )?;
    Ok(())
}

/// Tras un fallo transitorio ya contado en `download_attempts`: si llega a
/// `MAX_TRANSIENT_ATTEMPTS`, lo marca `pendiente_agotado`. Devuelve si lo marcó.
pub fn note_transient_failure(st: &Store, purpose: &str, mint: &str, reason: &str, now: i64) -> Result<bool> {
    if st.attempts(purpose, mint)? < MAX_TRANSIENT_ATTEMPTS {
        return Ok(false);
    }
    mark_download_final(st.conn(), purpose, mint, DownloadFinal::Exhausted, reason, now)?;
    Ok(true)
}

/// Clasifica las ventanas incompletas anteriores al cambio del 2026-10-26
/// (el bucle de reintentos) por la nota que dejó la descarga: tope de tx
/// por token y max_pages → `incompleto_permanente`; cualquier otra con
/// ≥ `MAX_TRANSIENT_ATTEMPTS` intentos → `pendiente_agotado`. Idempotente.
/// Devuelve (permanentes, agotadas) marcadas ahora.
pub fn sweep_download_backlog(db: &Connection, now: i64) -> Result<(usize, usize)> {
    let (mut perm, mut exh) = (0, 0);
    for (purpose, table) in [("window", "trade_windows"), ("prices", "price_windows")] {
        perm += db.execute(
            &format!(
                "INSERT OR IGNORE INTO vivo_download_final (purpose, mint, state, reason, attempts, marked_at)
                 SELECT ?1, w.mint, 'incompleto_permanente', w.note,
                        COALESCE((SELECT attempts FROM download_attempts a WHERE a.purpose = ?1 AND a.mint = w.mint), 0), ?2
                 FROM {table} w
                 WHERE w.complete = 0 AND (w.note LIKE 'max_pages%' OR w.note LIKE '%más que el tope por token%')"
            ),
            params![purpose, now],
        )?;
        exh += db.execute(
            &format!(
                "INSERT OR IGNORE INTO vivo_download_final (purpose, mint, state, reason, attempts, marked_at)
                 SELECT ?1, a.mint, 'pendiente_agotado', COALESCE(w.note, 'error listando firmas'), a.attempts, ?2
                 FROM download_attempts a LEFT JOIN {table} w ON w.mint = a.mint
                 WHERE a.purpose = ?1 AND a.attempts >= ?3 AND COALESCE(w.complete, 0) = 0"
            ),
            params![purpose, now, MAX_TRANSIENT_ATTEMPTS],
        )?;
    }
    Ok((perm, exh))
}

/// Seguimientos vencidos: (mint, horizonte, bonding curve).
pub fn due_outcomes(db: &Connection, now: i64, limit: usize) -> Result<Vec<(String, i64, String)>> {
    let mut q = db.prepare(
        "SELECT o.mint, o.horizon_secs, c.bonding_curve FROM vivo_outcomes o JOIN creations c ON c.mint = o.mint
         WHERE o.checked_at IS NULL AND o.due_at <= ?1 ORDER BY o.due_at LIMIT ?2",
    )?;
    let rows = q.query_map(params![now, limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn record_outcome(
    db: &Connection,
    mint: &str,
    horizon: i64,
    account: &str,
    complete: Option<bool>,
    state: Option<&str>,
    now: i64,
) -> Result<()> {
    db.execute(
        "UPDATE vivo_outcomes SET checked_at = ?3, account = ?4, complete = ?5, state_json = ?6
         WHERE mint = ?1 AND horizon_secs = ?2",
        params![mint, horizon, now, account, complete, state],
    )?;
    Ok(())
}

/// Tokens cuya descarga ya terminó (ventana incompleta, o ventana de precio
/// registrada) y aún sin resultado agregado.
pub fn finished_without_result(db: &Connection, limit: usize) -> Result<Vec<String>> {
    let mut q = db.prepare(
        "SELECT t.mint FROM vivo_tokens t
         JOIN trade_windows w ON w.mint = t.mint
         WHERE t.status = 'creado' AND NOT EXISTS (SELECT 1 FROM vivo_results r WHERE r.mint = t.mint)
           AND (w.complete = 0 OR EXISTS (SELECT 1 FROM price_windows p WHERE p.mint = t.mint))
         ORDER BY t.slot LIMIT ?1",
    )?;
    let rows = q.query_map([limit as i64], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Agrega un token con el código congelado (`report::v3_rows`, las mismas
/// filas que la validación 3) y guarda el resultado. No se resume ni se
/// lee hasta el pre-registro de la fase 5.
pub fn aggregate(st: &Store, mint: &str, now: i64) -> Result<()> {
    let ret_b = |x: &crate::entry2::ExitB| match x {
        crate::entry2::ExitB::Single(v) | crate::entry2::ExitB::Rebuilt(v) => Some(*v),
        _ => None,
    };
    let (status, d2, json) = match st.entry2_input(mint)? {
        None => ("sin_ventana", None, None),
        Some(input) => {
            let inputs = [input];
            let (rows, no_init, _) = crate::report::v3_rows(st, &inputs)?;
            match rows.first() {
                None if no_init > 0 => ("sin_precio_inicial", None, None),
                None => ("sin_entrada", None, None),
                Some(r) => {
                    let v = serde_json::json!({
                        "entry2": r.e,
                        "creator": r.creator,
                        "mayhem": r.mayhem,
                        "h4": r.h4, "h6": r.h6, "h7": r.h7, "d2": r.d2,
                        "pump_y_caida": r.pd,
                        "horizontes": crate::v3::HORIZONS.map(|(h, _)| h),
                        "ret_a": r.ret_a,
                        "ret_b": r.ret_b.iter().map(ret_b).collect::<Vec<_>>(),
                        "first_2x_slot": r.first_2x,
                        "estricto_h4_h7": [r.strict.0, r.strict.1],
                        "price_horizon": r.i.price_horizon,
                    });
                    ("evaluado", r.d2, Some(v.to_string()))
                }
            }
        }
    };
    st.conn().execute(
        "INSERT OR REPLACE INTO vivo_results (mint, computed_at, frozen, status, d2, result_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![mint, now, FROZEN, status, d2, json],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Retención y exportación

/// Tablas de crudos con retención: (tabla, columna de tiempo, condición de
/// "ya agregado" para poder borrar).
const RAW: &[(&str, &str, &str)] = &[
    ("early_trades", "timestamp", "mint IN (SELECT mint FROM vivo_results)"),
    ("price_points", "timestamp", "mint IN (SELECT mint FROM vivo_results)"),
    // Cada firma ya está contada en vivo_hourly, y las muestreadas siguen en vivo_tokens.
    ("vivo_create_sigs", "block_time", "1"),
];

#[derive(Debug, Default, PartialEq)]
pub struct Maintenance {
    pub exported_rows: usize,
    pub exports: usize,
    pub deleted_rows: usize,
    pub freed_pages: i64,
}

/// Exporta a `dir` las filas de crudos con tiempo anterior a `now − days`
/// aún no exportadas, verifica cada archivo leyéndolo de vuelta y, solo
/// entonces, borra en lotes de `batch` las filas exportadas y ya agregadas.
/// Después, `incremental_vacuum` y `wal_checkpoint(TRUNCATE)`.
pub fn maintenance(st: &mut Store, dir: &str, days: i64, batch: usize, now: i64) -> Result<Maintenance> {
    anyhow::ensure!(days >= 2, "la retención mínima es de 2 días (las descargas de un token terminan en ~70 min)");
    let cutoff = now - days * 86_400;
    let mut m = Maintenance::default();
    std::fs::create_dir_all(dir).with_context(|| format!("creando {dir}"))?;
    for &(table, col, aggregated) in RAW {
        let key = format!("exportado_hasta:{table}");
        let from: i64 = meta(st.conn(), &key)?.map(|v| v.parse()).transpose()?.unwrap_or(0);
        if cutoff > from {
            let path = format!("{dir}/{table}_{from}_{cutoff}.csv.gz");
            let rows = export_csv_gz(st.conn(), table, col, from, cutoff, &path)?;
            let (bytes, sha) = verify_export(&path, rows)?;
            let tx = st.conn_mut().transaction()?;
            tx.execute(
                "INSERT INTO vivo_exports (table_name, from_ts, until_ts, rows, path, bytes, sha256, created_at, verified_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![table, from, cutoff, rows as i64, path, bytes as i64, sha, now],
            )?;
            tx.execute(
                "INSERT INTO vivo_meta VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2",
                [&key, &cutoff.to_string()],
            )?;
            tx.commit()?;
            m.exports += 1;
            m.exported_rows += rows;
        }
        // Solo se borra lo cubierto por una exportación verificada.
        let Some(until) = meta(st.conn(), &key)?.map(|v| v.parse::<i64>()).transpose()? else { continue };
        let cond = format!("{col} < ?1 AND {aggregated}");
        if table != "vivo_create_sigs" {
            // `fetched_sigs` y `seen_signatures` solo sirven mientras se descarga.
            for aux in ["fetched_sigs", "seen_signatures"] {
                m.deleted_rows += delete_batched(
                    st.conn(),
                    &format!("DELETE FROM {aux} WHERE rowid IN (SELECT a.rowid FROM {aux} a WHERE a.signature IN (SELECT signature FROM {table} WHERE {cond}) LIMIT ?2)"),
                    until,
                    batch,
                )?;
            }
        }
        m.deleted_rows += delete_batched(
            st.conn(),
            &format!("DELETE FROM {table} WHERE rowid IN (SELECT rowid FROM {table} WHERE {cond} LIMIT ?2)"),
            until,
            batch,
        )?;
    }
    let free: i64 = st.conn().query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    st.conn().execute_batch("PRAGMA incremental_vacuum;")?;
    let after: i64 = st.conn().query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    m.freed_pages = free - after;
    st.conn().query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    Ok(m)
}

/// Borra en transacciones pequeñas (`batch` filas cada una) hasta que no
/// quede nada: no bloquea la base mucho rato.
fn delete_batched(db: &Connection, sql: &str, until: i64, batch: usize) -> Result<usize> {
    let mut total = 0;
    loop {
        let n = db.execute(sql, params![until, batch as i64])?;
        total += n;
        if n == 0 {
            return Ok(total);
        }
    }
}

fn csv_field(v: rusqlite::types::ValueRef) -> String {
    use rusqlite::types::ValueRef::*;
    let s = match v {
        Null => return String::new(),
        Integer(i) => return i.to_string(),
        Real(f) => return f.to_string(),
        Text(t) => String::from_utf8_lossy(t).into_owned(),
        Blob(b) => b.iter().map(|x| format!("{x:02x}")).collect(),
    };
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
}

/// Escribe `table` con `col` en `[from, until)` como CSV comprimido con gzip
/// (cabecera con los nombres de columna) y hace fsync. Devuelve las filas.
fn export_csv_gz(db: &Connection, table: &str, col: &str, from: i64, until: i64, path: &str) -> Result<usize> {
    let tmp = format!("{path}.tmp");
    let f = std::fs::File::create(&tmp).with_context(|| format!("creando {tmp}"))?;
    let mut gz = flate2::write::GzEncoder::new(std::io::BufWriter::new(f), flate2::Compression::default());
    let mut q = db.prepare(&format!("SELECT * FROM {table} WHERE {col} >= ?1 AND {col} < ?2 ORDER BY {col}"))?;
    let names: Vec<String> = q.column_names().iter().map(|s| s.to_string()).collect();
    writeln!(gz, "{}", names.join(","))?;
    let mut rows = q.query(params![from, until])?;
    let mut n = 0;
    while let Some(r) = rows.next()? {
        let line: Vec<String> = (0..names.len()).map(|i| r.get_ref(i).map(csv_field)).collect::<rusqlite::Result<_>>()?;
        writeln!(gz, "{}", line.join(","))?;
        n += 1;
    }
    let w = gz.finish()?;
    let f = w.into_inner().map_err(|e| e.into_error())?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(n)
}

/// Confirma que la copia llegó: lee el archivo del disco, lo descomprime
/// entero y cuenta las filas. Devuelve (bytes, sha256 del archivo).
pub fn verify_export(path: &str, rows: usize) -> Result<(u64, String)> {
    let bytes = std::fs::read(path).with_context(|| format!("leyendo {path} para verificar la exportación"))?;
    let lines = std::io::BufReader::new(flate2::read::GzDecoder::new(&bytes[..])).lines().count();
    anyhow::ensure!(
        lines == rows + 1,
        "{path}: {} filas leídas de vuelta, se escribieron {rows}: no se borra nada",
        lines.saturating_sub(1)
    );
    let sha = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    Ok((bytes.len() as u64, sha))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::SigInfo;

    pub fn tmp(name: &str) -> String {
        let d = std::env::temp_dir().join(format!("marxol_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_file(&d);
        let _ = std::fs::remove_dir_all(&d);
        d.to_string_lossy().into_owned()
    }

    fn sig(s: &str, slot: u64, bt: i64, failed: bool) -> SigInfo {
        SigInfo { signature: s.into(), slot, block_time: Some(bt), failed }
    }

    #[test]
    fn muestreo_uniforme_y_reproducible() {
        let n = (0..200_000).filter(|i| sampled(&format!("firma{i}"), 5_000)).count();
        assert!((800..1200).contains(&n), "{n}");
        assert!(sampled("abc", 1_000_000));
        assert!(!sampled("abc", 0));
        assert_eq!(fnv1a64(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn listado_guarda_cursor_horas_y_muestra_en_una_transaccion() {
        let p = tmp("vivo_listado.db");
        let mut st = open(&p, 100).unwrap();
        // 2026-10-22 13:xx UTC; todas muestreadas (ppm = 1e6) salvo la fallida.
        let t = 1_792_674_000;
        let sigs = vec![sig("C", 30, t + 60, false), sig("B", 20, t + 30, true), sig("A", 10, t - 3600, false)];
        assert_eq!(record_listing(&mut st, &sigs, 1_000_000, None, 200).unwrap(), 2);
        assert_eq!(cursor(st.conn()).unwrap(), Some(Cursor { signature: "C".into(), slot: 30 }));
        let h: Vec<(i64, i64, i64)> = st
            .conn()
            .prepare("SELECT listed, failed, sampled FROM vivo_hourly ORDER BY hour")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(h, vec![(1, 0, 1), (2, 1, 1)]);
        let (hour, cap, frozen): (i64, i64, String) = st
            .conn()
            .query_row("SELECT hour_utc, captured_at, frozen FROM vivo_tokens WHERE signature = 'C'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((hour, cap, frozen.as_str()), (13, 200, FROZEN));
        // Repetir el listado no duplica ni cuenta dos veces.
        assert_eq!(record_listing(&mut st, &sigs, 1_000_000, None, 300).unwrap(), 0);
        let listed: i64 = st.conn().query_row("SELECT SUM(listed) FROM vivo_hourly", [], |r| r.get(0)).unwrap();
        assert_eq!(listed, 3);
        // Un hueco queda registrado con el slot del cursor anterior.
        record_listing(&mut st, &[sig("E", 99, t + 900, false)], 0, Some("tope de páginas"), 400).unwrap();
        let g: (i64, i64) =
            st.conn().query_row("SELECT after_slot, before_slot FROM vivo_gaps", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(g, (30, 99));
        assert_eq!(cursor(st.conn()).unwrap().unwrap().slot, 99);
    }

    #[test]
    fn create_fallido_tres_veces_queda_en_error_y_sin_create_al_momento() {
        let p = tmp("vivo_fallos.db");
        let mut st = open(&p, 0).unwrap();
        record_listing(&mut st, &[sig("X", 1, 10, false), sig("Y", 2, 11, false)], 1_000_000, None, 0).unwrap();
        for _ in 0..3 {
            mark_create_failed(st.conn(), "X", "error", "red", 1).unwrap();
        }
        mark_create_failed(st.conn(), "Y", "sin_create", "sin CreateEvent", 1).unwrap();
        let s: Vec<String> = st
            .conn()
            .prepare("SELECT status FROM vivo_tokens ORDER BY signature")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(s, vec!["error", "sin_create"]);
        assert!(pending_creates(st.conn(), 10).unwrap().is_empty());
    }

    #[test]
    fn exporta_verifica_y_solo_borra_lo_agregado_y_exportado() {
        let p = tmp("vivo_ret.db");
        let dir = tmp("vivo_export");
        let mut st = open(&p, 0).unwrap();
        let day = 86_400;
        let now = 100 * day;
        let db = st.conn();
        for (mint, ts) in [("M1", 10 * day), ("M1", 10 * day + 1), ("M2", 10 * day), ("M1", 99 * day)] {
            db.execute(
                "INSERT INTO early_trades (mint, user, is_buy, timestamp, slot, signature, ev_index) VALUES (?1, 'u', 1, ?2, 1, ?1 || ?2, 0)",
                params![mint, ts],
            )
            .unwrap();
            db.execute("INSERT INTO fetched_sigs VALUES ('window', ?1 || ?2)", params![mint, ts]).unwrap();
        }
        // Solo M1 está agregado.
        db.execute("INSERT INTO vivo_results VALUES ('M1', 0, 'x', 'evaluado', 0, NULL)", []).unwrap();
        set_meta(db, "cursor_slot", "5").unwrap();
        let m = maintenance(&mut st, &dir, 30, 1, now).unwrap();
        assert_eq!((m.exports, m.exported_rows), (3, 3), "una exportación por tabla; 3 filas antiguas");
        let left: Vec<(String, i64)> = st
            .conn()
            .prepare("SELECT mint, timestamp FROM early_trades ORDER BY mint, timestamp")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        // Se borran las 2 antiguas de M1; quedan la reciente de M1 y la de M2 (sin agregar).
        assert_eq!(left, vec![("M1".into(), 99 * day), ("M2".into(), 10 * day)]);
        let fetched: i64 = st.conn().query_row("SELECT COUNT(*) FROM fetched_sigs", [], |r| r.get(0)).unwrap();
        assert_eq!(fetched, 2);
        assert_eq!(meta(st.conn(), "cursor_slot").unwrap().as_deref(), Some("5"), "el punto de reanudación no se toca");
        // La exportación está registrada con su sha256 y se puede volver a verificar.
        let (path, rows, sha): (String, i64, String) = st
            .conn()
            .query_row("SELECT path, rows, sha256 FROM vivo_exports WHERE table_name = 'early_trades'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(verify_export(&path, rows as usize).unwrap().1, sha);
        // Una segunda pasada no vuelve a exportar lo ya exportado.
        assert_eq!(maintenance(&mut st, &dir, 30, 100, now).unwrap().exports, 0);
        // Si la copia no cuadra, no se da por buena.
        std::fs::write(&path, b"no es gzip").unwrap();
        assert!(verify_export(&path, rows as usize).is_err());
    }

    #[test]
    fn la_base_no_guarda_la_url_ni_la_clave_del_rpc() {
        let p = tmp("vivo_clave.db");
        let secret = "CLAVE-SECRETA-123";
        let url = format!("https://solana-mainnet.g.alchemy.com/v2/{secret}");
        let provider = crate::rpc::Provider::parse("auto", &url).unwrap();
        let caps = ledger::Caps::new(1000, 500).unwrap();
        let l = ledger::Ledger::open(&p, "collect", ledger::Pool::Collect, caps, provider.name, 0).unwrap();
        l.charge(40, 0).unwrap();
        let mut st = open(&p, 0).unwrap();
        record_listing(&mut st, &[sig("S", 1, 10, false)], 1_000_000, None, 0).unwrap();
        drop(st);
        drop(l);
        let mut all = std::fs::read(&p).unwrap();
        all.extend(std::fs::read(format!("{p}-wal")).unwrap_or_default());
        assert!(!all.windows(secret.len()).any(|w| w == secret.as_bytes()));
        assert!(!all.windows(10).any(|w| w == b"alchemy.co"));
    }
}
