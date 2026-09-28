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

-- Firmas ya procesadas, para no repetir getTransaction (cada uno cuesta CU).
CREATE TABLE IF NOT EXISTS seen_signatures (
    signature TEXT PRIMARY KEY,
    slot      INTEGER NOT NULL
);
"#;

pub struct Store {
    db: Connection,
}

#[derive(Debug, Default)]
pub struct Ingested {
    pub creations: usize,
    pub creator_changes: usize,
    pub completions: usize,
    pub migrations: usize,
}

impl Store {
    pub fn open(path: &str) -> Result<Self> {
        let db = Connection::open(path).with_context(|| format!("abriendo {path}"))?;
        db.execute_batch(SCHEMA)?;
        Ok(Store { db })
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

    pub fn counts(&self) -> Result<Vec<(&'static str, i64)>> {
        ["creations", "creator_changes", "completions", "migrations", "seen_signatures"]
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
}
