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

/// Etapa 2: firmas de cada pagador aún sin recorrer, desde su token más
/// nuevo hasta `días` antes del más antiguo, con tope de páginas.
pub fn scan_payers(rpc: &RpcClient, db: &Connection, b: &rpc::Budget, cfg: &Config) -> Result<usize> {
    let mut q = db.prepare(
        "SELECT payer, (SELECT signature FROM ph_tokens t2 WHERE t2.payer = t.payer ORDER BY slot DESC LIMIT 1), MIN(t0)
         FROM ph_tokens t WHERE payer NOT IN (SELECT payer FROM ph_payer_scan) GROUP BY payer ORDER BY payer",
    )?;
    let todo: Vec<(String, String, i64)> =
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    let mut n = 0;
    for (payer, from_sig, min_t0) in todo {
        if !b.has_room(cfg.max_pages_payer as u64) {
            eprintln!("tope diario cerca: se para (reanudable)");
            break;
        }
        let need = min_t0 - cfg.days * 86_400;
        let (mut before, mut pages, mut sigs, mut oldest) = (from_sig.clone(), 0, 0, None::<i64>);
        let (mut complete, mut truncated) = (false, false);
        let t = db.unchecked_transaction()?;
        loop {
            if pages >= cfg.max_pages_payer {
                truncated = true;
                break;
            }
            let page = rpc::with_retry(b, rpc::Method::GetSignatures, &format!("firmas del pagador {payer}"), || {
                tx::signatures_page(rpc, &payer, 1000, Some(&before), None)
            })?;
            pages += 1;
            sigs += page.len();
            for s in &page {
                t.execute("INSERT OR IGNORE INTO ph_payer_sigs VALUES (?1, ?2, ?3)", params![payer, s.signature, s.block_time])?;
                oldest = s.block_time.or(oldest);
            }
            if page.len() < 1000 || oldest.is_some_and(|o| o < need) {
                complete = true;
                break;
            }
            before = page.last().unwrap().signature.clone();
        }
        t.execute(
            "INSERT INTO ph_payer_scan VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![payer, from_sig, need, oldest, pages as i64, sigs as i64, truncated, complete, rpc::unix_now()],
        )?;
        t.commit()?;
        n += 1;
    }
    Ok(n)
}

/// (mint, pagador, previos, pagador completo, mint-authority cubierto).
pub type Prior = (String, String, i64, bool, bool);

/// Lanzamientos previos por token: (mint, pagador, previos en `[t0 − días, t0)`,
/// recorrido completo del pagador, listado de mint-authority completo para el token).
pub fn prior_launches(db: &Connection, days: i64) -> Result<Vec<Prior>> {
    let ma_oldest: Option<i64> = db.query_row("SELECT MIN(block_time) FROM ph_ma_sigs", [], |r| r.get(0))?;
    let mut q = db.prepare(
        "SELECT t.mint, t.payer, t.t0,
                (SELECT COUNT(*) FROM ph_payer_sigs p JOIN ph_ma_sigs m ON m.signature = p.signature
                 WHERE p.payer = t.payer AND m.failed = 0 AND m.signature != t.signature
                   AND m.block_time >= t.t0 - ?1 AND m.block_time < t.t0),
                COALESCE((SELECT complete FROM ph_payer_scan s WHERE s.payer = t.payer), 0)
         FROM ph_tokens t WHERE t.payer IN (SELECT payer FROM ph_payer_scan) ORDER BY t.mint",
    )?;
    let rows = q.query_map([days * 86_400], |r| {
        let t0: i64 = r.get(2)?;
        Ok((r.get(0)?, r.get(1)?, r.get(3)?, r.get(4)?, ma_oldest.is_some_and(|o| o <= t0 - days * 86_400)))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
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
             INSERT INTO ph_payer_scan VALUES ('P', 'S0', {need}, {old}, 1, 6, 0, 1, 0);",
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
    }
}
