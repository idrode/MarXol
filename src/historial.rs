//! Historial de lanzamientos del pagador (fase 3, opción C' de
//! docs/propuesta_creadores_serie.md), sin `getTransaction`:
//!
//! 1. Lista las firmas de la PDA mint-authority hacia atrás desde la creación
//!    más nueva de los tokens elegidos, hasta `días` antes de la más antigua.
//!    Cada firma sin error de mint-authority es una creación.
//! 2. Lista las firmas de cada pagador (`creations.user`, la única wallet del
//!    creador que aparece en la tx de creación) hacia atrás desde su token,
//!    con tope de páginas.
//! 3. Lanzamientos previos del pagador = firmas de los dos listados en
//!    `[t0 − días, t0)`.
//!
//! Lee `marxol.db` en solo lectura y escribe en una base aparte
//! (`marxol_fase3.db`). Es un **mínimo**: no ve las creaciones con el mismo
//! `creator` pagadas por otra wallet, ni operadores con varias wallets, y una
//! wallet truncada por el tope de páginas puede tener más.

use crate::store::Store;
use crate::{pump, rpc, tx, vivo};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use solana_rpc_client::rpc_client::RpcClient;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS ph_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- Tokens elegidos de marxol.db.
CREATE TABLE IF NOT EXISTS ph_tokens (
    mint       TEXT PRIMARY KEY,
    signature  TEXT NOT NULL,
    payer      TEXT NOT NULL,
    creator    TEXT NOT NULL,
    t0         INTEGER NOT NULL,
    slot       INTEGER NOT NULL,
    rule       TEXT NOT NULL
);
-- Firmas de mint-authority listadas (todas las creaciones del periodo).
CREATE TABLE IF NOT EXISTS ph_ma_sigs (
    signature   TEXT PRIMARY KEY,
    slot        INTEGER NOT NULL,
    block_time  INTEGER,
    failed      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ph_ma_sigs_bt ON ph_ma_sigs(block_time);
-- Recorrido de cada pagador.
CREATE TABLE IF NOT EXISTS ph_payer_scan (
    payer        TEXT PRIMARY KEY,
    from_sig     TEXT NOT NULL,
    need_until   INTEGER NOT NULL,   -- blockTime hasta el que hace falta llegar
    oldest_bt    INTEGER,
    pages        INTEGER NOT NULL,
    sigs         INTEGER NOT NULL,
    truncated    INTEGER NOT NULL,   -- tope de páginas sin llegar a need_until
    complete     INTEGER NOT NULL,   -- llegó a need_until o al principio de la wallet
    scanned_at   INTEGER NOT NULL
);
-- Muestra de tx de creación: creator (argumento) y pago del pagador.
CREATE TABLE IF NOT EXISTS ph_create_tx (
    signature     TEXT PRIMARY KEY,
    kind          TEXT NOT NULL,     -- 'pagador≥N' o 'coste'
    mint          TEXT,
    creator       TEXT,
    payer         TEXT,
    payer_delta   INTEGER,           -- lamports que pierde el pagador (pre − post)
    tx_fee        INTEGER,
    buys          INTEGER NOT NULL,  -- compras del pagador en la tx: sol + fee + creator_fee
    sells         INTEGER NOT NULL,  -- ventas del pagador en la tx: sol − comisiones
    decode_errors INTEGER NOT NULL,
    fetched_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ph_payer_sigs (
    payer       TEXT NOT NULL,
    signature   TEXT NOT NULL,
    block_time  INTEGER,
    PRIMARY KEY (payer, signature)
);
"#;

pub struct Config {
    pub days: i64,
    pub max_pages_payer: usize,
    pub max_pages_ma: usize,
}

pub fn open(path: &str) -> Result<Connection> {
    let db = Connection::open(path).with_context(|| format!("abriendo {path}"))?;
    db.execute_batch(SCHEMA)?;
    // Columnas añadidas tras la etapa 1 (bases ya creadas).
    for (col, ty) in [("reached_end", "INTEGER"), ("last_sig", "TEXT")] {
        let exists: bool = db.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('ph_payer_scan') WHERE name = ?1",
            [col],
            |r| r.get(0),
        )?;
        if !exists {
            db.execute_batch(&format!("ALTER TABLE ph_payer_scan ADD COLUMN {col} {ty}"))?;
            if col == "reached_end" {
                // Etapa 1: 'complete' sin alcanzar need_until solo pudo ser por llegar al final de la wallet.
                db.execute_batch("UPDATE ph_payer_scan SET reached_end = (complete = 1 AND (oldest_bt IS NULL OR oldest_bt >= need_until))")?;
            } else {
                db.execute_batch(
                    "UPDATE ph_payer_scan SET last_sig = (SELECT p.signature FROM ph_payer_sigs p WHERE p.payer = ph_payer_scan.payer
                     ORDER BY p.block_time, p.signature LIMIT 1)",
                )?;
            }
        }
    }
    Ok(db)
}

/// Añade a la base de trabajo los `limit` tokens de `[from, until)` de
/// `marxol.db` con menor `fnv1a64(mint)` (elección reproducible, sin mirar
/// ningún dato del token). Devuelve cuántos hay en total.
pub fn select_tokens(db: &Connection, src: &Store, from: i64, until: i64, limit: usize, rule: &str) -> Result<usize> {
    let mut q = src.conn().prepare(
        "SELECT mint, signature, user, creator, timestamp, slot FROM creations WHERE timestamp >= ?1 AND timestamp < ?2",
    )?;
    let mut rows: Vec<(String, String, String, String, i64, i64)> = q
        .query_map(params![from, until], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
        .collect::<rusqlite::Result<_>>()?;
    rows.sort_by_key(|r| (vivo::fnv1a64(&r.0), r.0.clone()));
    for r in rows.into_iter().take(limit) {
        db.execute(
            "INSERT OR IGNORE INTO ph_tokens VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![r.0, r.1, r.2, r.3, r.4, r.5, rule],
        )?;
    }
    Ok(db.query_row("SELECT COUNT(*) FROM ph_tokens", [], |r| r.get::<_, i64>(0))? as usize)
}

fn meta(db: &Connection, k: &str) -> Result<Option<String>> {
    Ok(db.query_row("SELECT value FROM ph_meta WHERE key = ?1", [k], |r| r.get(0)).optional()?)
}

fn set_meta(db: &Connection, k: &str, v: &str) -> Result<()> {
    db.execute("INSERT INTO ph_meta VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2", [k, v])?;
    Ok(())
}

/// Etapa 1: mint-authority hacia atrás. Reanudable: sigue desde la firma más
/// antigua ya listada. Si una tanda posterior incluye tokens más nuevos que
/// el inicio del listado, falla (habría que listar hacia delante).
pub fn list_mint_authority(rpc: &RpcClient, db: &Connection, b: &rpc::Budget, cfg: &Config) -> Result<()> {
    let (newest_sig, newest_slot, oldest_t0): (String, i64, i64) = db.query_row(
        "SELECT (SELECT signature FROM ph_tokens ORDER BY slot DESC, signature DESC LIMIT 1),
                (SELECT MAX(slot) FROM ph_tokens), (SELECT MIN(t0) FROM ph_tokens)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let need = oldest_t0 - cfg.days * 86_400;
    match meta(db, "ma_start_slot")? {
        Some(s) => anyhow::ensure!(
            newest_slot <= s.parse::<i64>()?,
            "hay tokens más nuevos que el inicio del listado de mint-authority (slot {s}): no cubierto"
        ),
        None => {
            set_meta(db, "ma_start_slot", &newest_slot.to_string())?;
            set_meta(db, "ma_before", &newest_sig)?;
            // La propia creación más nueva también es de mint-authority.
            let (slot, t0): (i64, i64) =
                db.query_row("SELECT slot, t0 FROM ph_tokens WHERE signature = ?1", [&newest_sig], |r| Ok((r.get(0)?, r.get(1)?)))?;
            db.execute("INSERT OR IGNORE INTO ph_ma_sigs VALUES (?1, ?2, ?3, 0)", params![newest_sig, slot, t0])?;
        }
    }
    let address = pump::mint_authority_pda().to_string();
    let mut pages = 0;
    loop {
        let before = meta(db, "ma_before")?.context("sin ma_before")?;
        let oldest: Option<i64> = db.query_row("SELECT MIN(block_time) FROM ph_ma_sigs", [], |r| r.get(0))?;
        if oldest.is_some_and(|o| o < need) || meta(db, "ma_end")?.is_some() {
            return Ok(());
        }
        if pages >= cfg.max_pages_ma {
            eprintln!("mint-authority: tope de {pages} páginas en esta ejecución (reanudable)");
            return Ok(());
        }
        let page = rpc::with_retry(b, rpc::Method::GetSignatures, "firmas de mint-authority", || {
            tx::signatures_page(rpc, &address, 1000, Some(&before), None)
        })?;
        pages += 1;
        let t = db.unchecked_transaction()?;
        for s in &page {
            t.execute(
                "INSERT OR IGNORE INTO ph_ma_sigs VALUES (?1, ?2, ?3, ?4)",
                params![s.signature, s.slot as i64, s.block_time, s.failed],
            )?;
        }
        match page.last() {
            Some(l) => set_meta(&t, "ma_before", &l.signature)?,
            None => set_meta(&t, "ma_end", "1")?,
        }
        if page.len() < 1000 {
            set_meta(&t, "ma_end", "1")?;
        }
        t.commit()?;
        if pages % 50 == 0 {
            eprintln!("mint-authority: {pages} páginas, {}", b.summary());
        }
    }
}

/// Etapa 2: firmas de cada pagador, desde su token más nuevo hasta `días`
/// antes del más antiguo, con tope de páginas por pagador. Si una tanda
/// nueva añade tokens más antiguos de un pagador ya recorrido, se sigue
/// desde su firma más antigua; si añade uno más nuevo que el inicio del
/// recorrido, se rehace entero.
pub fn scan_payers(rpc: &RpcClient, db: &Connection, b: &rpc::Budget, cfg: &Config) -> Result<usize> {
    let mut q = db.prepare(
        "SELECT t.payer, (SELECT signature FROM ph_tokens t2 WHERE t2.payer = t.payer ORDER BY slot DESC, signature DESC LIMIT 1),
                MIN(t.t0), s.from_sig, s.oldest_bt, s.reached_end, s.truncated, s.last_sig, s.pages, s.sigs
         FROM ph_tokens t LEFT JOIN ph_payer_scan s ON s.payer = t.payer GROUP BY t.payer ORDER BY t.payer",
    )?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(String, String, i64, Option<String>, Option<i64>, Option<bool>, Option<bool>, Option<String>, Option<i64>, Option<i64>)> = q
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut n = 0;
    for (payer, newest_sig, min_t0, from_sig, oldest_bt, reached_end, truncated, last_sig, pages0, sigs0) in rows {
        let need = min_t0 - cfg.days * 86_400;
        // Punto de partida: nuevo, continuación o nada que hacer.
        let (mut before, mut pages, mut sigs) = match &from_sig {
            Some(f) if f != &newest_sig => {
                db.execute("DELETE FROM ph_payer_sigs WHERE payer = ?1", [&payer])?;
                db.execute("DELETE FROM ph_payer_scan WHERE payer = ?1", [&payer])?;
                (newest_sig.clone(), 0, 0)
            }
            Some(_) if reached_end == Some(true) || truncated == Some(true) || oldest_bt.is_some_and(|o| o < need) => continue,
            Some(_) => (last_sig.clone().context("recorrido sin last_sig")?, pages0.unwrap_or(0) as usize, sigs0.unwrap_or(0) as usize),
            None => (newest_sig.clone(), 0, 0),
        };
        if !b.has_room(cfg.max_pages_payer as u64) {
            eprintln!("tope diario cerca: se para (reanudable)");
            break;
        }
        let mut oldest = if pages > 0 { oldest_bt } else { None };
        let (mut end, mut trunc, mut failed) = (false, false, false);
        let t = db.unchecked_transaction()?;
        loop {
            if pages >= cfg.max_pages_payer {
                trunc = true;
                break;
            }
            let page = match rpc::with_retry(b, rpc::Method::GetSignatures, &format!("firmas del pagador {payer}"), || {
                tx::signatures_page(rpc, &payer, 1000, Some(&before), None)
            }) {
                Ok(p) => p,
                Err(e) if rpc::is_exhausted(&e) => return Err(e),
                // Un pagador que falla (429 persistente) se salta; la próxima pasada lo reintenta.
                Err(e) => {
                    eprintln!("{payer}: se salta, queda pendiente: {}", rpc::redact(&format!("{e:#}")));
                    failed = true;
                    break;
                }
            };
            pages += 1;
            sigs += page.len();
            for s in &page {
                t.execute("INSERT OR IGNORE INTO ph_payer_sigs VALUES (?1, ?2, ?3)", params![payer, s.signature, s.block_time])?;
                oldest = s.block_time.or(oldest);
            }
            if let Some(l) = page.last() {
                before = l.signature.clone();
            }
            if page.len() < 1000 {
                end = true;
                break;
            }
            if oldest.is_some_and(|o| o < need) {
                break;
            }
        }
        if failed {
            // Sin fila de recorrido (o la anterior intacta): no se guarda nada a medias.
            drop(t);
            continue;
        }
        t.execute(
            "INSERT INTO ph_payer_scan (payer, from_sig, need_until, oldest_bt, pages, sigs, truncated, complete, scanned_at, reached_end, last_sig)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (payer) DO UPDATE SET need_until = ?3, oldest_bt = ?4, pages = ?5, sigs = ?6, truncated = ?7,
                complete = ?8, scanned_at = ?9, reached_end = ?10, last_sig = ?11",
            params![payer, newest_sig, need, oldest, pages as i64, sigs as i64, trunc, !trunc, rpc::unix_now(), end, before],
        )?;
        t.commit()?;
        n += 1;
    }
    Ok(n)
}

/// (mint, pagador, previos, pagador completo para este token, mint-authority cubierto).
pub type Prior = (String, String, i64, bool, bool);

/// Lanzamientos previos por token en `[t0 − días, t0)`. El recorrido del
/// pagador cubre el token si llegó al principio de la wallet o a una firma
/// anterior a `t0 − días`.
pub fn prior_launches(db: &Connection, days: i64) -> Result<Vec<Prior>> {
    let ma_oldest: Option<i64> = db.query_row("SELECT MIN(block_time) FROM ph_ma_sigs", [], |r| r.get(0))?;
    let mut q = db.prepare(
        "SELECT t.mint, t.payer, t.t0,
                (SELECT COUNT(*) FROM ph_payer_sigs p JOIN ph_ma_sigs m ON m.signature = p.signature
                 WHERE p.payer = t.payer AND m.failed = 0 AND m.signature != t.signature
                   AND m.block_time >= t.t0 - ?1 AND m.block_time < t.t0),
                s.reached_end = 1 OR s.oldest_bt < t.t0 - ?1
         FROM ph_tokens t JOIN ph_payer_scan s ON s.payer = t.payer ORDER BY t.mint",
    )?;
    let rows = q.query_map([days * 86_400], |r| {
        let t0: i64 = r.get(2)?;
        Ok((r.get(0)?, r.get(1)?, r.get(3)?, r.get(4)?, ma_oldest.is_some_and(|o| o <= t0 - days * 86_400)))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Muestra de tx de creación (una `getTransaction` cada una): el `creator`
/// (argumento) de cada lanzamiento y lo que paga el pagador, para el coste
/// de lanzar. `kind` dice de dónde sale la firma.
pub fn sample_creates(rpc: &RpcClient, idl: &crate::idl::Idl, db: &Connection, b: &rpc::Budget, sigs: &[(String, String)]) -> Result<usize> {
    let mut n = 0;
    for (sig, kind) in sigs {
        let seen: bool = db.query_row("SELECT COUNT(*) > 0 FROM ph_create_tx WHERE signature = ?1", [sig], |r| r.get(0))?;
        if seen {
            continue;
        }
        if !b.has_room(1) {
            eprintln!("tope diario cerca: se para (reanudable)");
            break;
        }
        let raw = match rpc::with_retry(b, rpc::Method::GetTransaction, &format!("creación {sig}"), || tx::fetch_raw(rpc, sig)) {
            Ok(r) => r,
            Err(e) if rpc::is_exhausted(&e) => return Err(e),
            Err(e) => {
                eprintln!("{sig}: se salta, queda pendiente: {}", rpc::redact(&format!("{e:#}")));
                continue;
            }
        };
        let t = tx::decode_events(idl, sig, &raw)?;
        let create = t.events.iter().find(|e| e.name == "CreateEvent");
        let (mint, creator, payer) = match create {
            Some(e) => (e.str("mint").map(str::to_owned), e.str("creator").map(str::to_owned), e.str("user").map(str::to_owned)),
            None => (None, None, None),
        };
        // Lo que el pagador pone en la curva y en comisiones de sus trades de la misma tx.
        let (mut buys, mut sells) = (0i64, 0i64);
        for e in t.events.iter().filter(|e| e.name == "TradeEvent" && e.str("user") == payer.as_deref()) {
            let v = (e.u64("sol_amount").unwrap_or(0) + e.u64("fee").unwrap_or(0) + e.u64("creator_fee").unwrap_or(0)) as i64;
            if e.bool("is_buy") == Some(true) {
                buys += v;
            } else {
                sells += e.u64("sol_amount").unwrap_or(0) as i64 - e.u64("fee").unwrap_or(0) as i64 - e.u64("creator_fee").unwrap_or(0) as i64;
            }
        }
        let delta = payer.as_deref().map(|p| tx::balance_delta(&raw, p)).transpose()?.flatten();
        db.execute(
            "INSERT INTO ph_create_tx VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                sig,
                kind,
                mint,
                creator,
                payer,
                delta.map(|d| d.0),
                delta.map(|d| d.1 as i64),
                buys,
                sells,
                t.decode_errors.len() as i64,
                rpc::unix_now()
            ],
        )?;
        n += 1;
    }
    Ok(n)
}

/// Firmas para `sample_creates`: `k` lanzamientos (por hash de la firma) de
/// cada pagador con ≥ `min_prior` previos, y `cost_n` creaciones de
/// `marxol.db` (por hash de la firma) para el coste de lanzar.
pub fn sample_signatures(db: &Connection, src: &Store, days: i64, min_prior: i64, k: usize, cost_n: usize) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut heavy: Vec<String> = prior_launches(db, days)?.into_iter().filter(|p| p.2 >= min_prior).map(|p| p.1).collect();
    heavy.sort();
    heavy.dedup();
    for payer in heavy {
        let mut q = db.prepare(
            "SELECT m.signature FROM ph_payer_sigs p JOIN ph_ma_sigs m ON m.signature = p.signature WHERE p.payer = ?1 AND m.failed = 0",
        )?;
        let mut s: Vec<String> = q.query_map([&payer], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        s.sort_by_key(|x| (vivo::fnv1a64(x), x.clone()));
        out.extend(s.into_iter().take(k).map(|x| (x, format!("pagador≥{min_prior}"))));
    }
    let mut q = src.conn().prepare("SELECT signature FROM creations")?;
    let mut s: Vec<String> = q.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    s.sort_by_key(|x| (vivo::fnv1a64(x), x.clone()));
    out.extend(s.into_iter().take(cost_n).map(|x| (x, "coste".to_string())));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previos_cuentan_solo_creaciones_sin_error_del_pagador_en_la_ventana() {
        let db = open(":memory:").unwrap();
        let d = 86_400;
        db.execute_batch(&format!(
            "INSERT INTO ph_tokens VALUES ('M', 'S0', 'P', 'P', {t0}, 100, 'test');
             INSERT INTO ph_ma_sigs VALUES ('S0', 100, {t0}, 0), ('A', 90, {a}, 0), ('B', 80, {b}, 1),
                                           ('C', 70, {c}, 0), ('D', 60, {old}, 0);
             INSERT INTO ph_payer_sigs VALUES ('P', 'S0', {t0}), ('P', 'A', {a}), ('P', 'B', {b}),
                                              ('P', 'C', {c}), ('P', 'D', {old}), ('P', 'X', {a});
             INSERT INTO ph_payer_scan VALUES ('P', 'S0', {need}, {old}, 1, 6, 0, 1, 0, 0, 'D');",
            t0 = 10 * d,
            a = 10 * d - 100,
            b = 9 * d,
            c = 4 * d,
            old = 2 * d,
            need = 3 * d,
        ))
        .unwrap();
        // A y C son creaciones sin error del pagador en [t0 − 7 d, t0); B falló, D es anterior, X no es creación.
        assert_eq!(prior_launches(&db, 7).unwrap(), vec![("M".into(), "P".into(), 2, true, true)]);
        // Con 1 día solo entra A.
        assert_eq!(prior_launches(&db, 1).unwrap()[0].2, 1);
        // Con 9 días el recorrido (hasta 2 d) no cubre t0 − 9 d = 1 d: no completo para ese token.
        assert!(!prior_launches(&db, 9).unwrap()[0].3);
    }
}
