//! Lectura de transacciones y extracción de eventos Anchor `emit_cpi!`.
//!
//! Flujo (CLAUDE.md 5.3): `getTransaction` → inner instructions dirigidas al
//! programa pump.fun cuyo primer account es el `event_authority` PDA → datos
//! con prefijo `ANCHOR_EVENT_IX_TAG` → Borsh contra el IDL. No se parsean logs.

use crate::idl::{Decoded, Idl};
use crate::pump;
use anyhow::{bail, Context, Result};
use solana_commitment_config::CommitmentConfig;
use solana_rpc_client::rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient};
use solana_rpc_client_api::config::RpcTransactionConfig;
use solana_signature::Signature;
use solana_transaction_status_client_types::option_serializer::OptionSerializer;
use solana_transaction_status_client_types::{
    EncodedConfirmedTransactionWithStatusMeta, EncodedTransaction, UiInstruction, UiMessage, UiTransactionEncoding,
};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Cuenta los `TradeEvent` con más bytes de los que describe el IDL (campos
/// añadidos al final por un despliegue posterior). No es un error: el prefijo
/// se decodifica igual y lo que sobra se ignora. Se avisa una sola vez por
/// ejecución, con la longitud observada.
pub struct ExtraBytes {
    count: AtomicU64,
    warned: AtomicBool,
}

impl Default for ExtraBytes {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtraBytes {
    pub const fn new() -> Self {
        ExtraBytes { count: AtomicU64::new(0), warned: AtomicBool::new(false) }
    }

    /// Cuenta un evento de `len` bytes con `extra` sin describir; devuelve la
    /// línea de aviso solo la primera vez.
    pub fn observe(&self, len: usize, extra: usize) -> Option<String> {
        self.count.fetch_add(1, Ordering::Relaxed);
        (!self.warned.swap(true, Ordering::Relaxed)).then(|| {
            format!(
                "aviso: TradeEvent de {len} bytes, {extra} más de los {} que describe el IDL embebido; se ignoran \
                 (campos añadidos por un despliegue posterior: revisar idl/pump.json). Se avisa una vez por ejecución",
                len - extra
            )
        })
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Línea de resumen para el final de la ejecución (`None` si no hubo ninguno).
    pub fn summary(&self) -> Option<String> {
        let n = self.count();
        (n > 0).then(|| format!("aviso: {n} TradeEvent con bytes de más respecto al IDL embebido en esta ejecución"))
    }
}

pub static TRADE_EXTRA_BYTES: ExtraBytes = ExtraBytes::new();

#[derive(Debug)]
pub struct TxEvents {
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<i64>,
    pub failed: bool,
    /// Instrucciones pump.fun de nivel superior (por nombre del IDL).
    pub instructions: Vec<String>,
    pub events: Vec<Decoded>,
    /// Inner instructions de evento que no se pudieron decodificar.
    pub decode_errors: Vec<String>,
    /// Por cada `TradeEvent`: (bytes de datos sin prefijo ni discriminador,
    /// bytes sobrantes en hex). Solo diagnóstico (`scan --json`).
    pub trade_sizes: Vec<(usize, String)>,
}

pub fn fetch_events(rpc: &RpcClient, idl: &Idl, sig: &str) -> Result<TxEvents> {
    decode_events(idl, sig, &fetch_raw(rpc, sig)?)
}

/// `getTransaction` en codificación `json`, sin decodificar.
pub fn fetch_raw(rpc: &RpcClient, sig: &str) -> Result<EncodedConfirmedTransactionWithStatusMeta> {
    let signature = Signature::from_str(sig).context("firma inválida")?;
    rpc
        .get_transaction_with_config(
            &signature,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Json),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(1),
            },
        )
        .with_context(|| format!("getTransaction {sig}"))
}

/// Lamports que pierde `wallet` en la tx (pre − post) y comisión de la tx.
/// `None` si la wallet no está entre las cuentas.
pub fn balance_delta(tx: &EncodedConfirmedTransactionWithStatusMeta, wallet: &str) -> Result<Option<(i64, u64)>> {
    let EncodedTransaction::Json(ui_tx) = &tx.transaction.transaction else {
        bail!("codificación de transacción inesperada");
    };
    let UiMessage::Raw(msg) = &ui_tx.message else {
        bail!("mensaje no es Raw");
    };
    let meta = tx.transaction.meta.as_ref().context("transacción sin meta")?;
    // Los saldos siguen el orden de las claves estáticas y luego las cargadas.
    let mut keys = msg.account_keys.clone();
    if let OptionSerializer::Some(loaded) = &meta.loaded_addresses {
        keys.extend(loaded.writable.iter().cloned());
        keys.extend(loaded.readonly.iter().cloned());
    }
    Ok(keys.iter().position(|k| k == wallet).and_then(|i| {
        Some((*meta.pre_balances.get(i)? as i64 - *meta.post_balances.get(i)? as i64, meta.fee))
    }))
}

/// Extrae y decodifica los eventos pump.fun de una transacción ya leída
/// (respuesta de `getTransaction` con codificación `json`). Separado de
/// `fetch_events` para poder probarlo con transacciones guardadas.
pub fn decode_events(idl: &Idl, sig: &str, tx: &EncodedConfirmedTransactionWithStatusMeta) -> Result<TxEvents> {
    let EncodedTransaction::Json(ui_tx) = &tx.transaction.transaction else {
        bail!("codificación de transacción inesperada");
    };
    let UiMessage::Raw(msg) = &ui_tx.message else {
        bail!("mensaje no es Raw (¿se pidió jsonParsed?)");
    };
    let meta = tx.transaction.meta.as_ref().context("transacción sin meta")?;

    // Claves completas: estáticas + cargadas desde lookup tables (tx v0).
    let mut keys = msg.account_keys.clone();
    if let OptionSerializer::Some(loaded) = &meta.loaded_addresses {
        keys.extend(loaded.writable.iter().cloned());
        keys.extend(loaded.readonly.iter().cloned());
    }

    let program = pump::PROGRAM_ID;
    let event_authority = pump::event_authority_pda().to_string();
    let key = |i: u8| keys.get(i as usize).map(String::as_str);

    let instructions = msg
        .instructions
        .iter()
        .filter(|ix| key(ix.program_id_index) == Some(program))
        .map(|ix| {
            bs58::decode(&ix.data)
                .into_vec()
                .ok()
                .and_then(|d| idl.instruction_by_disc(&d).map(|i| i.name.clone()))
                .unwrap_or_else(|| "<desconocida>".into())
        })
        .collect();

    let mut events = Vec::new();
    let mut decode_errors = Vec::new();
    let mut trade_sizes = Vec::new();
    if let OptionSerializer::Some(inner) = &meta.inner_instructions {
        for group in inner {
            for ix in &group.instructions {
                let UiInstruction::Compiled(c) = ix else { continue };
                if key(c.program_id_index) != Some(program)
                    || c.accounts.first().and_then(|&a| key(a)) != Some(event_authority.as_str())
                {
                    continue;
                }
                let data = bs58::decode(&c.data).into_vec().context("inner ix no es base58")?;
                match idl.decode_event_cpi(&data) {
                    Some(Ok(ev)) => {
                        if ev.name == "TradeEvent" {
                            let len = data.len().saturating_sub(16);
                            let extra: String = data[data.len() - ev.trailing_bytes..].iter().map(|b| format!("{b:02x}")).collect();
                            trade_sizes.push((len, extra));
                        }
                        if ev.name == "TradeEvent" && ev.trailing_bytes > 0 {
                            // Datos de evento sin el prefijo de emit_cpi! ni el discriminador.
                            let len = data.len().saturating_sub(16);
                            if let Some(w) = TRADE_EXTRA_BYTES.observe(len, ev.trailing_bytes) {
                                eprintln!("{w}");
                            }
                        }
                        events.push(ev)
                    }
                    Some(Err(e)) => decode_errors.push(format!("{e:#}")),
                    None => {}
                }
            }
        }
    }

    Ok(TxEvents {
        signature: sig.to_string(),
        slot: tx.slot,
        block_time: tx.block_time,
        failed: meta.err.is_some(),
        instructions,
        events,
        decode_errors,
        trade_sizes,
    })
}

pub struct SigInfo {
    pub signature: String,
    pub failed: bool,
    pub slot: u64,
    pub block_time: Option<i64>,
}

/// Firmas recientes que tocan `address` (más recientes primero).
pub fn recent_signatures(
    rpc: &RpcClient,
    address: &str,
    limit: usize,
    before: Option<&str>,
) -> Result<Vec<SigInfo>> {
    signatures_page(rpc, address, limit, before, None)
}

/// Como `recent_signatures`, parando en `until` (excluida): solo firmas más
/// nuevas que ella (`collect`, desde el punto de reanudación).
pub fn signatures_page(
    rpc: &RpcClient,
    address: &str,
    limit: usize,
    before: Option<&str>,
    until: Option<&str>,
) -> Result<Vec<SigInfo>> {
    let addr = solana_pubkey::Pubkey::from_str(address).context("dirección inválida")?;
    let sigs = rpc.get_signatures_for_address_with_config(
        &addr,
        GetConfirmedSignaturesForAddress2Config {
            before: before.map(Signature::from_str).transpose()?,
            until: until.map(Signature::from_str).transpose()?,
            limit: Some(limit),
            commitment: Some(CommitmentConfig::confirmed()),
        },
    )?;
    Ok(sigs
        .into_iter()
        .map(|s| SigInfo {
            failed: s.err.is_some(),
            slot: s.slot,
            block_time: s.block_time,
            signature: s.signature,
        })
        .collect())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::store::{self, Store};

    /// Transacciones reales guardadas (`result` de `getTransaction`, `json`).
    pub fn fixture(sig: &str) -> EncodedConfirmedTransactionWithStatusMeta {
        let path = format!("{}/tests/fixtures/tx/{sig}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    /// IDL embebido hasta el 2026-10-21 (descargado el 2026-09-28).
    pub fn old_idl() -> Idl {
        Idl::parse(include_str!("../tests/fixtures/pump_idl_2026-09-28.json")).unwrap()
    }

    pub fn decode(idl: &Idl, sig: &str) -> TxEvents {
        decode_events(idl, sig, &fixture(sig)).unwrap()
    }

    /// Una transacción por tipo (creación, buy, buy_exact_sol_in,
    /// buy_exact_quote_in, sell, migrate_to_sharing, completado) de cada
    /// tramo: V1, V2 y V3.
    const HISTORICAL: [&str; 18] = [
        "2JVXhpLyCvtsxbv7xmzbgmLzj5ZXruyeXJkxzJU6aAw6dpfLucbj5K8fSpsMCXPiYdhgmiH8J3P7XL3oJn4aAKe4",
        "CDk6yq4s5LeQ6DSebGk4mdFL3Gou1dYKZmdhSRuvzhsvdPBi3Ubxt3PByBDAnNQPEAWfGmsVoJYscKCqmEtejzb",
        "BKX1uzRndhugczqS9tYhGJ7fMviPArgeJM69LphCydk3WQvkMLARCpP5Ye49QTMHrX6sULa6wgeDYxWX8EQenMR",
        "5KhYZ3FaH6ASYQEKedEiWx2wqvYPs9MQQYjH6LN1iSeSubasAAwsKESsbjQi7nm2XqTvVPpHEcbXVPsneJMkaf3X",
        "3LxRsEXSRigNFz8NQtBpkZcvSiD4zej4TJbUxJk7fBHaMJmrG8w6Q9h3J97RdCiwWrQut4cXrFNgijm9HEfNqJ4Y",
        "5NZKxQYzK4J1YyfLAkjHo8B2tp7bPtvWuqsyPZCPXtkBu8WCbkEQVUAArwH31jz2yTLaF6c7be9merZWWPKpunBb",
        "3mxY1rgK8EUe3PaBo1iDMkTyjnLYYR5F6ffN9WoT6rfDKyqQbwLtfCDFsqKDsv5cjQDqojN5A6oHPoHkiRtX9ka2",
        "3uAdnJ4qwbUCfNVhxv5p5hJfk37dYMUP3EXu9oSzn1BnNUhPsC15p4CVTuyxBefqfXrPurVR4TaVCTHney1WqPmn",
        "2nVKPbCHDT7SwkVfT7mGtKbSpw4R54c4GP7wGaNgKCseX1EXjQNDwsogmmpVpx6LgA7KfFg6bx7GHJXbc1fjasTS",
        "4zsKKauRaVVsbqrWkHgKxWF1tRZPxhN2CFNhjaQEXRgCFQB8hZsH4mSXP54YxZo1Z4VkPCWE89HWCbNDCYzmC7XB",
        "5Vp5wCfWoYy51Xmoq2CaZ13ZpsLJt33eiWJCRrGcMUkfbtjVWnBiqctrHkToLvW3bY75WSemGkaV6FuWFwkD4G6z",
        "4Xc5aER3hCUpSTW5jYEV6uQChyqczqcCefcbynrbUcz29GLcJKQFKT3SxHtsJQ1EGDB1M7Ngbc3fid1Jd8wMWvwE",
        "mUKGav3xWvj5ejZ5mDJjMycY9UFDe5viMTiBSY6dNMsKgnKV8ZSN2VUf6AqDWppS77ACjsBtRvCYDX6xhswxboY",
        "5WPsKu3yXBXFajzXFqmRE2of9nvYdq9GnhCem29Te44rD6ffSsKzczg14kziS5sbbicUqYpRZqCAtnoEMpbLFiNe",
        "4r4qUbYKVi56XgYwietTsJDbmd4sJxycTDJmf7j22nKaBCMbp4xXa8fCo4eKh1jbZyrxfu1oAfk5x1HJn4urxDqM",
        "3mngqmzZseHJiUEVJFHU75e5n84uVVRhBSw18HTfnjpnvv5XMy12B1zRoz8EqxY5gpLu481waRn5kQF9CiT6MXV8",
        "5YuZz3wgvPhsc7VKyo3nbaqoANzqHFTFTqBKjrbwoNxaAaB2s3Ug8vSiypG9NgBDKHcNmeprZ9GuQCWaT5WvPY4b",
        "3njLtj6QHuK5eNkDpkTCsQN6eouNgLA7xR8Cf4z5tQEhUEWmxdQcBoALA4L8vrZMaFa7o1kZ8RvedjazLWSWpmdf",
    ];
    /// 6 `sweep_protocol_fee`/`sweep_creator_fee` sin ningún TradeEvent (2026-10-21).
    pub const SWEEPS: &str = "4jWiadygYj78SMC1YmZUSWN7cAJiE34v9JJfodaVc6d1NdgwjAw2rU2STriano1yfRng6LJA8khE9fDBi4mWz7jw";
    /// `sweep_creator_fee` + migración a fee sharing + reparto (2026-10-21).
    const SWEEP_AND_SHARING: &str =
        "3C3JbTdWHdnqTGRqf92G8VLN3LNPDcscTkdRTt2xWZjL5C2w2auCRhdSirAyNu43RL2h7bTKp1ZxqyWvoiu7gj9H";
    /// `buy_v2` y `sell_v2` con el TradeEvent actual (2026-10-21).
    pub const BUY_V2: &str = "64NxjLonjngttdNzhz6nk6T8Ady7JpVEHT2QnpHJa36wjkyEM7cbTLGn4WiN2k7AywEM2ZDPwhPq9j6b51v6mSFF";
    pub const SELL_V2: &str = "bAWYS4t1nUqsSVPQw6yDdgBk4ZKmmjwXvRGogXeopK7frSEWXHKcKXx9SUpv1u6uMLrpuuxRhkvCSiSuTYkTze1";

    /// Volcado de las tablas que llena `ingest` / `ingest_prices`.
    fn dump(st: &Store) -> Vec<String> {
        let mut out = Vec::new();
        for t in ["creations", "creator_changes", "completions", "migrations", "early_trades", "price_points"] {
            let mut q = st.conn().prepare(&format!("SELECT * FROM {t} ORDER BY 1, 2, 3")).unwrap();
            let n = q.column_count();
            let rows = q
                .query_map([], |r| {
                    let v: Vec<String> =
                        (0..n).map(|i| format!("{:?}", r.get::<_, rusqlite::types::Value>(i).unwrap())).collect();
                    Ok(format!("{t}: {}", v.join(" | ")))
                })
                .unwrap();
            out.extend(rows.map(Result::unwrap));
        }
        out
    }

    #[test]
    fn fixtures_existen_y_coinciden_con_su_firma() {
        for sig in HISTORICAL.iter().chain(&[SWEEPS, SWEEP_AND_SHARING, BUY_V2, SELL_V2]) {
            let EncodedTransaction::Json(t) = fixture(sig).transaction.transaction else { panic!("{sig}") };
            assert_eq!(t.signatures[0], *sig);
        }
    }

    #[test]
    fn idl_nuevo_decodifica_v1_v2_v3_igual_que_el_anterior() {
        let (old, new) = (old_idl(), Idl::pump().unwrap());
        let (mut st_old, mut st_new) = (Store::open(":memory:").unwrap(), Store::open(":memory:").unwrap());
        let mut events = 0;
        for sig in HISTORICAL {
            let (a, b) = (decode(&old, sig), decode(&new, sig));
            assert!(a.decode_errors.is_empty() && b.decode_errors.is_empty(), "{sig}");
            assert_eq!(a.instructions, b.instructions, "{sig}");
            assert_eq!(a.events.len(), b.events.len(), "{sig}");
            for (x, y) in a.events.iter().zip(&b.events) {
                assert_eq!((&x.name, &x.fields, x.trailing_bytes), (&y.name, &y.fields, y.trailing_bytes), "{sig}");
                // Los campos añadidos al IDL después de estos datos solo aparecen como ausentes.
                assert!(x.missing.iter().all(|m| y.missing.contains(m)), "{sig}");
                for m in y.missing.iter().filter(|m| !x.missing.contains(m)) {
                    assert!(["creator_fee_unclaimed", "depth"].contains(&m.as_str()), "{sig}: {m}");
                }
            }
            events += a.events.len();
            for (st, t) in [(&mut st_old, &a), (&mut st_new, &b)] {
                st.ingest(t).unwrap();
                st.ingest_prices(t, store::EARLY_WINDOW_SECS).unwrap();
            }
        }
        assert!(events >= 18);
        let (da, db) = (dump(&st_old), dump(&st_new));
        assert!(da.iter().any(|r| r.starts_with("early_trades")) && da.iter().any(|r| r.starts_with("creator_changes")));
        assert_eq!(da, db);
    }

    #[test]
    fn sweep_bonding_curve_fee_event_no_es_un_trade() {
        // Con el IDL anterior: 6 eventos desconocidos con el discriminador 742b4dbd117a482b.
        let a = decode(&old_idl(), SWEEPS);
        assert!(a.events.is_empty());
        assert_eq!(a.decode_errors.len(), 6);
        assert!(a.decode_errors.iter().all(|e| e.contains("[74, 2b, 4d, bd, 11, 7a, 48, 2b]")), "{:?}", a.decode_errors);
        // Con el actual: 6 barridos de comisión (bucket 0 = protocolo, 1 = creador), sin trades.
        let b = decode(&Idl::pump().unwrap(), SWEEPS);
        assert!(b.decode_errors.is_empty());
        assert_eq!(b.instructions.iter().filter(|i| i.starts_with("sweep_")).count(), 6);
        assert_eq!(b.events.len(), 6);
        for e in &b.events {
            assert_eq!(e.name, "SweepBondingCurveFeeEvent");
            assert!(e.missing.is_empty() && e.trailing_bytes == 0);
            let keys: Vec<&str> = e.fields.keys().map(String::as_str).collect();
            assert_eq!(keys, ["timestamp", "mint", "bonding_curve", "quote_mint", "recipient", "amount", "bucket"]);
        }
        let e = &b.events[4];
        assert_eq!(e.str("mint"), Some("4mekvGznnqViqJxZPA1YGiduq7evu8kSHrFkgyjEpump"));
        assert_eq!((e.u64("amount"), e.u64("bucket"), e.i64("timestamp")), (Some(20_185_443), Some(0), Some(1_792_615_113)));
        let mut st = Store::open(":memory:").unwrap();
        st.ingest(&b).unwrap();
        assert!(dump(&st).is_empty());
    }

    #[test]
    fn barrido_con_migracion_a_fee_sharing() {
        let b = decode(&Idl::pump().unwrap(), SWEEP_AND_SHARING);
        assert!(b.decode_errors.is_empty());
        let names: Vec<&str> = b.events.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["SweepBondingCurveFeeEvent", "MigrateBondingCurveCreatorEvent", "DistributeCreatorFeesEvent"]);
        let mut st = Store::open(":memory:").unwrap();
        st.ingest(&b).unwrap();
        let d = dump(&st);
        assert_eq!(d.len(), 1);
        assert!(d[0].starts_with("creator_changes") && d[0].contains("migrate_to_sharing"), "{d:?}");
        assert!(!decode(&old_idl(), SWEEP_AND_SHARING).decode_errors.is_empty());
    }

    #[test]
    fn aviso_de_bytes_de_mas_una_vez_con_contador() {
        let w = ExtraBytes::new();
        assert!(w.summary().is_none());
        let first = w.observe(383, 8).unwrap();
        assert!(first.contains("383 bytes, 8 más de los 375"), "{first}");
        assert!(w.observe(382, 8).is_none() && w.observe(383, 8).is_none());
        assert_eq!(w.count(), 3);
        assert!(w.summary().unwrap().contains("3 TradeEvent"));
    }

    #[test]
    fn trade_event_con_bytes_de_mas_se_cuenta_sin_error() {
        let before = TRADE_EXTRA_BYTES.count();
        let t = decode(&Idl::pump().unwrap(), SELL_V2);
        assert!(t.decode_errors.is_empty());
        assert_eq!(t.events[0].trailing_bytes, 8);
        // Otros tests pueden decodificar a la vez: solo se comprueba que sube.
        assert!(TRADE_EXTRA_BYTES.count() > before);
        // Los históricos de V1–V3 no tienen bytes de más.
        for sig in HISTORICAL {
            assert!(decode(&Idl::pump().unwrap(), sig).events.iter().all(|e| e.trailing_bytes == 0), "{sig}");
        }
    }

    #[test]
    fn trade_event_actual_trae_creator_fee_unclaimed() {
        let (old, new) = (old_idl(), Idl::pump().unwrap());
        for (sig, ix, ev_ix, buy) in [(BUY_V2, "buy_v2", "buy", true), (SELL_V2, "sell_v2", "sell", false)] {
            let (a, b) = (decode(&old, sig), decode(&new, sig));
            assert!(a.decode_errors.is_empty() && b.decode_errors.is_empty());
            let (x, y) = (&a.events[0], &b.events[0]);
            assert_eq!((x.name.as_str(), y.name.as_str()), ("TradeEvent", "TradeEvent"));
            // El IDL anterior deja sin leer el u64 `creator_fee_unclaimed` y 8 bytes
            // más que tampoco están en el IDL público actual (commit del 2026-10-13):
            // los añadió un despliegue posterior (2026-10-18/19). Se toleran como
            // bytes sobrantes, igual que hasta ahora; no se inventa su nombre.
            assert_eq!((x.trailing_bytes, y.trailing_bytes), (16, 8));
            assert!(y.missing.is_empty() && y.u64("creator_fee_unclaimed").is_some());
            let mut z = y.fields.clone();
            z.remove("creator_fee_unclaimed");
            assert_eq!(x.fields, z);
            // `ix_name` del evento no distingue la versión de la instrucción.
            assert!(b.instructions.iter().any(|i| i == ix));
            assert_eq!((y.str("ix_name"), y.bool("is_buy")), (Some(ev_ix), Some(buy)));
        }
    }
}
