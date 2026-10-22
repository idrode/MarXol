//! `marxol collect`: recolección continua de lanzamientos de pump.fun en
//! `marxol_vivo.db`, pensada para correr 24/7 en un servidor ligero.
//!
//! Cada ciclo, con la bolsa `collect` del libro de CU diario:
//! 1. Lista las firmas nuevas de la PDA mint-authority desde el punto de
//!    reanudación (1 000 por petición) y muestrea por hash de la firma, con
//!    tasa fija: cubre las 24 h sin franjas.
//! 2. Comprueba el estado de las bonding curves con seguimiento vencido
//!    (+1 h, +24 h, +7 d, +60 d), en lotes de 100 con `getMultipleAccounts`.
//! 3. Descarga la tx de creación de los tokens muestreados.
//! 4. Descarga la ventana temprana y la de precio (las mismas pasadas que
//!    `windows` y `prices`, con tope de tx por token).
//! 5. Agrega los tokens terminados con el código congelado.
//!
//! Todo es idempotente y se guarda en transacciones: tras un corte, el
//! siguiente arranque sigue desde el cursor y lo pendiente.

use crate::idl::Idl;
use crate::ingest::{prices_pass, windows_pass};
use crate::store::{self, Store};
use crate::{pump, rpc, tx, vivo};
use anyhow::{Context, Result};
use rusqlite::params;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Config {
    /// Tasa de muestreo, en partes por millón de las creaciones listadas.
    pub sample_ppm: u32,
    /// Segundos entre el inicio de dos ciclos.
    pub poll_secs: u64,
    /// Ciclos a ejecutar (`None`: sin fin).
    pub cycles: Option<u64>,
    /// Páginas de 1 000 firmas como máximo para alcanzar el cursor; si no
    /// bastan, queda un hueco registrado en `vivo_gaps`.
    pub max_backfill_pages: usize,
    /// Páginas por bonding curve en las ventanas (como `windows --max-pages`).
    pub max_pages: usize,
    /// Tope de tx por token y ventana; un token con más queda incompleto.
    pub max_token_sigs: usize,
    /// Tokens por etapa y ciclo.
    pub per_cycle: usize,
    /// Retención de crudos en días (`None`: sin borrado).
    pub retention_days: Option<i64>,
    pub export_dir: String,
    /// Mantenimiento (exportar, borrar, vacuum, checkpoint) cada N ciclos.
    pub maintenance_every: u64,
    pub delete_batch: usize,
}

/// CU por debajo de las cuales no se empieza un ciclo (se espera al día UTC
/// siguiente).
const MIN_CYCLE_UNITS: u64 = 2_000;

pub fn run(rpc: &RpcClient, idl: &Idl, st: &mut Store, budget: &rpc::Budget, cfg: &Config) -> Result<()> {
    let ledger = budget.ledger().context("collect necesita el libro de CU")?;
    let mut n = 0u64;
    loop {
        let start = rpc::unix_now();
        if ledger.remaining(start)? < MIN_CYCLE_UNITS {
            let next_day = (start.div_euclid(86_400) + 1) * 86_400;
            eprintln!("{}: se espera al día UTC siguiente", ledger.summary(start)?);
            sleep_until(next_day.min(start + 600));
            continue;
        }
        let r = cycle(rpc, idl, st, budget, cfg, start);
        if let Err(e) = &r {
            // Un error de red no tumba el proceso: se registra y se sigue en el próximo ciclo.
            eprintln!("ciclo con error (se reintenta en el siguiente): {e:#}");
        }
        n += 1;
        if let (Some(days), true) = (cfg.retention_days, n.is_multiple_of(cfg.maintenance_every.max(1))) {
            match vivo::maintenance(st, &cfg.export_dir, days, cfg.delete_batch, rpc::unix_now()) {
                Ok(m) => eprintln!(
                    "mantenimiento: {} exportaciones ({} filas), {} filas borradas, {} páginas liberadas",
                    m.exports, m.exported_rows, m.deleted_rows, m.freed_pages
                ),
                Err(e) => eprintln!("mantenimiento con error (no se ha borrado nada sin exportar): {e:#}"),
            }
        }
        if cfg.cycles.is_some_and(|c| n >= c) {
            return Ok(());
        }
        sleep_until(start + cfg.poll_secs as i64);
    }
}

fn sleep_until(t: i64) {
    let wait = t - rpc::unix_now();
    if wait > 0 {
        std::thread::sleep(std::time::Duration::from_secs(wait as u64));
    }
}

#[derive(Debug, Default)]
struct Cycle {
    units: [u64; 5],
    pages: usize,
    listed: usize,
    sampled: usize,
    creates: usize,
    windows: usize,
    prices: usize,
    outcomes: usize,
    results: usize,
}

fn cycle(rpc: &RpcClient, idl: &Idl, st: &mut Store, b: &rpc::Budget, cfg: &Config, now: i64) -> Result<()> {
    st.conn().execute("INSERT INTO vivo_cycles (started_at) VALUES (?1)", [now])?;
    let id = st.conn().last_insert_rowid();
    let mut c = Cycle::default();
    let r = (|| {
        let stage = |i: usize, c: &mut Cycle, f: &mut dyn FnMut(&mut Cycle) -> Result<()>| {
            let u0 = b.units();
            let r = f(c);
            c.units[i] += b.units() - u0;
            match r {
                Err(e) if rpc::is_exhausted(&e) => {
                    eprintln!("{e:#}: se para el ciclo (reanudable)");
                    Ok(false)
                }
                Err(e) => Err(e),
                Ok(()) => Ok(b.has_room(1)),
            }
        };
        if !stage(0, &mut c, &mut |c| poll(rpc, st, b, cfg, now, c))? {
            return Ok(());
        }
        if !stage(1, &mut c, &mut |c| outcomes(rpc, idl, st, b, cfg, c))? {
            return Ok(());
        }
        if !stage(2, &mut c, &mut |c| creates(rpc, idl, st, b, cfg, c))? {
            return Ok(());
        }
        if !stage(3, &mut c, &mut |c| {
            let mut done = 0;
            windows_pass(rpc, idl, st, cfg.per_cycle, cfg.max_pages, Some(cfg.max_token_sigs), b, &mut done)?;
            c.windows = done;
            Ok(())
        })? {
            return Ok(());
        }
        stage(4, &mut c, &mut |c| {
            let mut done = 0;
            prices_pass(rpc, idl, st, cfg.per_cycle, cfg.max_pages, Some(cfg.max_token_sigs), b, &mut done)?;
            c.prices = done;
            Ok(())
        })?;
        Ok(())
    })();
    // La agregación no usa la red: se hace aunque se haya agotado la cuota.
    let agg = (|| {
        for mint in vivo::finished_without_result(st.conn(), 500)? {
            vivo::aggregate(st, &mint, rpc::unix_now())?;
            c.results += 1;
        }
        anyhow::Ok(())
    })();
    let end = rpc::unix_now();
    let note = r.as_ref().err().or(agg.as_ref().err()).map(|e| format!("{e:#}"));
    st.conn().execute(
        "UPDATE vivo_cycles SET ended_at = ?2, units_poll = ?3, units_outcomes = ?4, units_create = ?5,
                units_windows = ?6, units_prices = ?7, pages = ?8, sigs_listed = ?9, sampled = ?10,
                creates = ?11, windows = ?12, prices = ?13, outcomes = ?14, results = ?15, note = ?16
         WHERE cycle_id = ?1",
        params![
            id,
            end,
            c.units[0] as i64,
            c.units[1] as i64,
            c.units[2] as i64,
            c.units[3] as i64,
            c.units[4] as i64,
            c.pages as i64,
            c.listed as i64,
            c.sampled as i64,
            c.creates as i64,
            c.windows as i64,
            c.prices as i64,
            c.outcomes as i64,
            c.results as i64,
            note
        ],
    )?;
    eprintln!(
        "ciclo {id}: {} CU (listado {}, seguimiento {}, creación {}, ventanas {}, precios {}); {} firmas en {} páginas, \
         {} muestreadas, {} creaciones, {} ventanas, {} precios, {} seguimientos, {} agregados; {}",
        c.units.iter().sum::<u64>(),
        c.units[0],
        c.units[1],
        c.units[2],
        c.units[3],
        c.units[4],
        c.listed,
        c.pages,
        c.sampled,
        c.creates,
        c.windows,
        c.prices,
        c.outcomes,
        c.results,
        b.ledger().map(|l| l.summary(end)).transpose()?.unwrap_or_default()
    );
    r.and(agg)
}

/// Etapa 1: firmas nuevas de mint-authority hasta el cursor.
fn poll(rpc: &RpcClient, st: &mut Store, b: &rpc::Budget, cfg: &Config, now: i64, c: &mut Cycle) -> Result<()> {
    let address = pump::mint_authority_pda().to_string();
    let cur = vivo::cursor(st.conn())?;
    let mut all: Vec<tx::SigInfo> = Vec::new();
    let mut before: Option<String> = None;
    let mut gap = None;
    loop {
        let page = rpc::with_retry(b, rpc::Method::GetSignatures, "firmas de mint-authority", || {
            tx::signatures_page(rpc, &address, 1000, before.as_deref(), cur.as_ref().map(|c| c.signature.as_str()))
        })?;
        c.pages += 1;
        let full = page.len() == 1000;
        before = page.last().map(|s| s.signature.clone());
        all.extend(page);
        // Primer arranque: solo la página más reciente, sin recuperar hacia atrás.
        if !full || cur.is_none() {
            break;
        }
        if c.pages >= cfg.max_backfill_pages {
            gap = Some(format!("{} páginas sin alcanzar el cursor", c.pages));
            break;
        }
    }
    c.listed = all.len();
    c.sampled = vivo::record_listing(st, &all, cfg.sample_ppm, gap.as_deref(), now)?;
    if let Some(g) = gap {
        eprintln!("aviso: hueco en el listado ({g}); queda en vivo_gaps");
    }
    Ok(())
}

/// Etapa 2: estado de las bonding curves con seguimiento vencido.
fn outcomes(rpc: &RpcClient, idl: &Idl, st: &mut Store, b: &rpc::Budget, cfg: &Config, c: &mut Cycle) -> Result<()> {
    let due = vivo::due_outcomes(st.conn(), rpc::unix_now(), cfg.per_cycle * 100)?;
    for chunk in due.chunks(100) {
        let keys: Vec<Pubkey> = chunk.iter().map(|(_, _, bc)| Pubkey::from_str(bc)).collect::<Result<_, _>>()?;
        let accs = rpc::with_retry(b, rpc::Method::GetMultipleAccounts, "bonding curves", || {
            Ok(rpc.get_multiple_accounts(&keys)?)
        })?;
        let now = rpc::unix_now();
        let t = st.conn_mut().transaction()?;
        for ((mint, h, _), acc) in chunk.iter().zip(accs) {
            let (kind, complete, state) = match acc {
                None => ("cerrada", None, None),
                Some(a) if a.owner != pump::program_id() => ("otro_owner", None, None),
                Some(a) => match idl.decode_account(&a.data) {
                    Ok(d) => ("existe", d.bool("complete"), Some(d.to_json().to_string())),
                    Err(e) => ("existe", None, Some(serde_json::json!({ "error": format!("{e:#}") }).to_string())),
                },
            };
            vivo::record_outcome(&t, mint, *h, kind, complete, state.as_deref(), now)?;
            c.outcomes += 1;
        }
        t.commit()?;
    }
    Ok(())
}

/// Etapa 3: tx de creación de los tokens muestreados. Guarda la creación
/// con `Store::ingest` (como `index`) y la da por descargada para la
/// ventana y el precio, así esas etapas no la vuelven a pedir.
fn creates(rpc: &RpcClient, idl: &Idl, st: &mut Store, b: &rpc::Budget, cfg: &Config, c: &mut Cycle) -> Result<()> {
    for (sig, _) in vivo::pending_creates(st.conn(), cfg.per_cycle)? {
        if !b.has_room(1) {
            return Ok(());
        }
        let now = rpc::unix_now();
        let t = match rpc::with_retry(b, rpc::Method::GetTransaction, &format!("creación {sig}"), || {
            tx::fetch_events(rpc, idl, &sig)
        }) {
            Ok(t) => t,
            Err(e) if rpc::is_exhausted(&e) => return Err(e),
            Err(e) => {
                vivo::mark_create_failed(st.conn(), &sig, "error", &format!("{e:#}"), now)?;
                continue;
            }
        };
        if !t.decode_errors.is_empty() {
            // Posible IDL desactualizado: no se da por buena ni se marca como descargada.
            let note = format!("errores de decodificación: {:?}", t.decode_errors);
            eprintln!("{sig}: {note}");
            vivo::mark_create_failed(st.conn(), &sig, "error", &note, now)?;
            continue;
        }
        if ingest_create(st, &sig, &t, now)? {
            c.creates += 1;
        }
    }
    Ok(())
}

/// Guarda la tx de creación de un token muestreado. `false` si no trae un
/// `CreateEvent` utilizable (el token queda marcado).
pub fn ingest_create(st: &mut Store, sig: &str, t: &tx::TxEvents, now: i64) -> Result<bool> {
    let Some(ev) = t.events.iter().find(|e| e.name == "CreateEvent") else {
        vivo::mark_create_failed(st.conn(), sig, "sin_create", "la tx no emite CreateEvent", now)?;
        return Ok(false);
    };
    let (Some(mint), Some(creator), Some(t0)) =
        (ev.str("mint").map(str::to_owned), ev.str("creator").map(str::to_owned), ev.get("timestamp").and_then(|v| v.as_i64()))
    else {
        vivo::mark_create_failed(st.conn(), sig, "error", "CreateEvent sin mint, creator o timestamp", now)?;
        return Ok(false);
    };
    st.ingest(t)?;
    st.ingest_prices(t, store::EARLY_WINDOW_SECS)?;
    st.mark_fetched("window", sig)?;
    st.mark_fetched("prices", sig)?;
    vivo::mark_created(st.conn(), sig, &mint, &creator, t.slot, t0, now)?;
    Ok(true)
}

/// Resumen para `collect-status`: CU por ciclo medidos y proyección diaria.
pub fn status(st: &Store, poll_secs: u64, sample_ppm: u32) -> Result<String> {
    use std::fmt::Write;
    let db = st.conn();
    let mut out = String::new();
    let cur = vivo::cursor(db)?;
    writeln!(out, "punto de reanudación: {}", cur.map(|c| format!("slot {} ({})", c.slot, c.signature)).unwrap_or("ninguno".into()))?;
    let mut q = db.prepare("SELECT status, COUNT(*) FROM vivo_tokens GROUP BY status ORDER BY status")?;
    let s: Vec<(String, i64)> = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    writeln!(out, "tokens muestreados por estado: {s:?}")?;
    let w: (i64, i64) = db.query_row(
        "SELECT COALESCE(SUM(complete), 0), COUNT(*) FROM trade_windows",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let p: (i64, i64) = db.query_row(
        "SELECT COALESCE(SUM(complete), 0), COUNT(*) FROM price_windows",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    writeln!(out, "ventanas tempranas completas {}/{}, de precio {}/{}", w.0, w.1, p.0, p.1)?;
    let o: (i64, i64) = db.query_row(
        "SELECT COUNT(checked_at), COUNT(*) FROM vivo_outcomes",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    writeln!(out, "seguimientos hechos {}/{}; resultados agregados (sin leer): {}", o.0, o.1,
        db.query_row("SELECT COUNT(*) FROM vivo_results", [], |r| r.get::<_, i64>(0))?)?;
    let gaps: i64 = db.query_row("SELECT COUNT(*) FROM vivo_gaps", [], |r| r.get(0))?;
    writeln!(out, "huecos en el listado: {gaps}")?;

    // Medido en los ciclos.
    let (cycles, span, u_poll, u_out, u_create, u_win, u_pr, listed, creates, wins, prs, outs): (
        i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64,
    ) = db.query_row(
        "SELECT COUNT(*), COALESCE(MAX(ended_at) - MIN(started_at), 0), COALESCE(SUM(units_poll), 0),
                COALESCE(SUM(units_outcomes), 0), COALESCE(SUM(units_create), 0), COALESCE(SUM(units_windows), 0),
                COALESCE(SUM(units_prices), 0), COALESCE(SUM(sigs_listed), 0), COALESCE(SUM(creates), 0),
                COALESCE(SUM(windows), 0), COALESCE(SUM(prices), 0), COALESCE(SUM(outcomes), 0)
         FROM vivo_cycles WHERE ended_at IS NOT NULL",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?)),
    )?;
    writeln!(out, "\nmedido en {cycles} ciclos ({span} s):")?;
    writeln!(out, "  listado      {u_poll} CU, {listed} firmas")?;
    writeln!(out, "  seguimiento  {u_out} CU, {outs} comprobaciones")?;
    writeln!(out, "  creación     {u_create} CU, {creates} tokens")?;
    writeln!(out, "  ventanas     {u_win} CU, {wins} tokens")?;
    writeln!(out, "  precios      {u_pr} CU, {prs} tokens")?;
    let per = |u: i64, n: i64| if n > 0 { u as f64 / n as f64 } else { f64::NAN };
    let poll_per_cycle = per(u_poll, cycles);
    let sigs_per_sec = if span > 0 { listed as f64 / span as f64 } else { f64::NAN };
    // Firmas de creación por día; cada ciclo lista al menos una página.
    let sigs_day = sigs_per_sec * 86_400.0;
    let cycles_day = 86_400.0 / poll_secs as f64;
    let pages_per_cycle = (sigs_per_sec * poll_secs as f64 / 1000.0).ceil().max(1.0);
    let poll_day = cycles_day * pages_per_cycle * 40.0;
    let ok_share = {
        let (ok, all): (i64, i64) = db.query_row(
            "SELECT COALESCE(SUM(1 - failed), 0), COUNT(*) FROM vivo_create_sigs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if all > 0 { ok as f64 / all as f64 } else { f64::NAN }
    };
    let tokens_day = sigs_day * ok_share * sample_ppm as f64 / 1e6;
    let per_token = per(u_create, creates) + per(u_win, wins) + per(u_pr, prs) + 4.0 * 20.0 / 100.0;
    writeln!(out, "\nproyección diaria (poll {poll_secs} s, muestreo {sample_ppm} ppm):")?;
    writeln!(out, "  firmas de mint-authority/día ≈ {sigs_day:.0} ({:.0} % sin error)", ok_share * 100.0)?;
    writeln!(
        out,
        "  listado ≈ {poll_day:.0} CU/día ({pages_per_cycle:.0} página(s) por ciclo; {poll_per_cycle:.0} CU por ciclo medido)"
    )?;
    writeln!(
        out,
        "  tokens muestreados/día ≈ {tokens_day:.0} × {per_token:.0} CU/token (creación {:.0} + ventana {:.0} + precio {:.0} + seguimiento ≈ 1) ≈ {:.0} CU/día",
        per(u_create, creates),
        per(u_win, wins),
        per(u_pr, prs),
        tokens_day * per_token
    )?;
    writeln!(out, "  total ≈ {:.0} CU/día", poll_day + tokens_day * per_token)?;

    writeln!(out, "\ncobertura por hora UTC (últimas 48 h de blockTime: listadas / muestreadas):")?;
    let mut q = db.prepare(
        "SELECT hour, listed, sampled FROM vivo_hourly WHERE hour >= (SELECT MAX(hour) FROM vivo_hourly) - 47 ORDER BY hour",
    )?;
    for r in q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))? {
        let (h, l, s) = r?;
        writeln!(out, "  {:02}:00 (día {})  {l:6} / {s}", h.rem_euclid(24), h.div_euclid(24))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::tests::fixture;

    fn tmp(name: &str) -> String {
        let d = std::env::temp_dir().join(format!("marxol_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_file(&d);
        d.to_string_lossy().into_owned()
    }

    fn fixtures() -> Vec<(String, tx::TxEvents)> {
        let idl = Idl::pump().unwrap();
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tx");
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path().file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v.into_iter().map(|s| (s.clone(), tx::decode_events(&idl, &s, &fixture(&s)).unwrap())).collect()
    }

    #[test]
    fn creacion_muestreada_queda_guardada_con_seguimiento_y_no_se_vuelve_a_pedir() {
        let all = fixtures();
        let (sig, t) = all.iter().find(|(_, t)| t.events.iter().any(|e| e.name == "CreateEvent")).expect("fixture con CreateEvent");
        let (other, t2) = all.iter().find(|(_, t)| t.events.iter().all(|e| e.name != "CreateEvent")).unwrap();
        let mut st = vivo::open(&tmp("collect_create.db"), 0).unwrap();
        let s = |sig: &str| tx::SigInfo { signature: sig.into(), slot: 1, block_time: Some(0), failed: false };
        vivo::record_listing(&mut st, &[s(sig), s(other)], 1_000_000, None, 0).unwrap();
        assert!(ingest_create(&mut st, sig, t, 5).unwrap());
        assert!(!ingest_create(&mut st, other, t2, 5).unwrap());
        let c = st.conn();
        let (mint, status): (String, String) =
            c.query_row("SELECT mint, status FROM vivo_tokens WHERE signature = ?1", [sig], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(status, "creado");
        let info = st.creation_info(&mint).unwrap().unwrap();
        assert!(st.creation_initial(&mint).unwrap().is_some(), "precio inicial del CreateEvent");
        let due: Vec<i64> = c
            .prepare("SELECT due_at - ?2 FROM vivo_outcomes WHERE mint = ?1 ORDER BY horizon_secs")
            .unwrap()
            .query_map(params![mint, info.t0], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(due, vivo::OUTCOME_HORIZONS.to_vec());
        assert!(st.fetched("window", sig).unwrap() && st.fetched("prices", sig).unwrap());
        let n: i64 = c.query_row("SELECT sampled_launches FROM vivo_creators WHERE creator = ?1", [&info.creator], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let st2: String = c.query_row("SELECT status FROM vivo_tokens WHERE signature = ?1", [other], |r| r.get(0)).unwrap();
        assert_eq!(st2, "sin_create");
    }

    /// La agregación de collect da lo mismo que las filas de la validación 3:
    /// se copian los 196 tokens de V3 de marxol.db (adjunta en solo lectura)
    /// a una base de recolección y se comparan D2 y los retornos (a).
    #[test]
    fn agregacion_coincide_con_v3_rows_en_los_196_de_v3() {
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/marxol.db");
        if !std::path::Path::new(src).exists() {
            return;
        }
        let ref_st = Store::open_readonly(src).unwrap();
        let inputs = ref_st.entry2_inputs_ts(crate::entry2::VALIDATION3_FROM_TS, crate::entry2::VALIDATION3_UNTIL_TS).unwrap();
        let (rows, _, _) = crate::report::v3_rows(&ref_st, &inputs).unwrap();
        let st = vivo::open(&tmp("collect_v3.db"), 0).unwrap();
        let c = st.conn();
        c.execute_batch(&format!("ATTACH 'file:{src}?mode=ro' AS src")).unwrap();
        let (from, until) = (crate::entry2::VALIDATION3_FROM_TS, crate::entry2::VALIDATION3_UNTIL_TS);
        let sel = format!("SELECT mint FROM src.creations WHERE timestamp >= {from} AND timestamp < {until}");
        for t in ["creations", "creator_changes", "completions", "migrations", "early_trades", "trade_windows", "price_points", "price_windows"] {
            c.execute_batch(&format!("INSERT INTO main.{t} SELECT * FROM src.{t} WHERE mint IN ({sel})")).unwrap();
        }
        c.execute_batch(&format!(
            "INSERT INTO vivo_tokens (signature, mint, slot, captured_at, sample_ppm, sample_rule, frozen, status, updated_at)
             SELECT signature, mint, slot, 0, 0, 'test', 'test', 'creado', 0 FROM src.creations WHERE mint IN ({sel});
             DETACH src;"
        ))
        .unwrap();
        let mints = vivo::finished_without_result(c, 1000).unwrap();
        assert_eq!(mints.len(), 196);
        for m in &mints {
            vivo::aggregate(&st, m, 0).unwrap();
        }
        let mut evaluated = 0;
        for r in &rows {
            let (status, d2, json): (String, Option<bool>, String) = c
                .query_row("SELECT status, d2, result_json FROM vivo_results WHERE mint = ?1", [&r.i.mint], |x| {
                    Ok((x.get(0)?, x.get(1)?, x.get(2)?))
                })
                .unwrap();
            let v: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!((status.as_str(), d2), ("evaluado", r.d2), "{}", r.i.mint);
            let ret_a: Vec<Option<f64>> = serde_json::from_value(v["ret_a"].clone()).unwrap();
            // El texto guardado es exacto (ryu); al leerlo, serde_json sin `float_roundtrip` puede variar un ulp.
            let close = |a: &Option<f64>, b: &Option<f64>| match (a, b) {
                (Some(x), Some(y)) => (x - y).abs() <= 1e-12,
                _ => a == b,
            };
            assert!(ret_a.iter().zip(&r.ret_a).all(|(a, b)| close(a, b)), "{} {ret_a:?} {:?}", r.i.mint, r.ret_a);
            evaluated += 1;
        }
        let others: i64 = c.query_row("SELECT COUNT(*) FROM vivo_results WHERE status != 'evaluado'", [], |r| r.get(0)).unwrap();
        assert_eq!(evaluated as i64 + others, 196);
        assert!(evaluated > 100, "{evaluated}");
    }
}
