//! Descargas desde la chain hacia la base local: `index`, `index-tramo`,
//! `windows` y `prices`, con presupuesto de RPC y registro en `rpc_usage`.

use crate::idl::Idl;
use crate::{h1b, pump, rpc, store, tx};
use anyhow::{Context, Result};
use solana_rpc_client::rpc_client::RpcClient;
use std::time::Duration;

pub fn index(rpc: &RpcClient, idl: &Idl, st: &mut store::Store, address: &str, limit: usize, delay_ms: u64) -> Result<()> {
    let mut before: Option<String> = None;
    let (mut walked, mut fetched, mut skipped) = (0, 0, 0);
    let mut total = store::Ingested::default();
    while walked < limit {
        let page = tx::recent_signatures(rpc, address, (limit - walked).min(1000), before.as_deref())?;
        let Some(last) = page.last() else { break };
        before = Some(last.signature.clone());
        walked += page.len();
        for s in page {
            let sig = s.signature;
            if st.seen(&sig)? {
                skipped += 1;
                continue;
            }
            if s.failed {
                // Una tx fallida no emite eventos: no se gasta getTransaction en ella.
                st.mark_seen(&sig, s.slot)?;
                continue;
            }
            std::thread::sleep(Duration::from_millis(delay_ms));
            match tx::fetch_events(rpc, idl, &sig) {
                Ok(t) => {
                    fetched += 1;
                    for e in &t.decode_errors {
                        eprintln!("{sig}: error decodificando evento: {e}");
                    }
                    let n = st.ingest(&t)?;
                    total.creations += n.creations;
                    total.creator_changes += n.creator_changes;
                    total.completions += n.completions;
                    total.migrations += n.migrations;
                    total.early_trades += n.early_trades;
                }
                // No se marca como vista: se reintentará en la próxima pasada.
                Err(e) => eprintln!("{sig}: {e:#}"),
            }
        }
        eprintln!("… {walked} firmas recorridas, {fetched} getTransaction, {skipped} ya vistas");
    }
    println!(
        "nuevas filas: {} creaciones, {} cambios de creador, {} completadas, {} migradas, \
         {} trades de ventana temprana",
        total.creations, total.creator_changes, total.completions, total.migrations, total.early_trades
    );
    Ok(())
}

/// Fase 2 de `index_tramo` con `--until` (validación 3): `getTransaction` de
/// todas las firmas exitosas con blockTime en [from, until), por slots
/// enteros, con `count` como tope de creaciones. El tramo se cierra después
/// por el `timestamp` de las creaciones, no por el count.
#[allow(clippy::too_many_arguments)]
fn index_tramo_until(
    rpc: &RpcClient,
    idl: &Idl,
    st: &mut store::Store,
    from: i64,
    until: i64,
    count: usize,
    ok: &[&tx::SigInfo],
    budget: &rpc::Budget,
    done: &mut usize,
) -> Result<()> {
    let (mut fetched, mut no_create, mut i) = (0, 0, 0);
    while i < ok.len() && st.creations_in_ts(from, until)?.len() < count {
        let slot = ok[i].slot;
        while i < ok.len() && ok[i].slot == slot {
            let sig = &ok[i].signature;
            i += 1;
            if st.seen(sig)? {
                continue;
            }
            let t = rpc::with_retry(budget, rpc::Method::GetTransaction, &format!("tx {sig}"), || tx::fetch_events(rpc, idl, sig))
                .with_context(|| format!("getTransaction {sig} (el slot {slot} queda incompleto; reanudable)"))?;
            fetched += 1;
            for e in &t.decode_errors {
                eprintln!("{sig}: error decodificando evento: {e}");
            }
            if !t.events.iter().any(|e| e.name == "CreateEvent") {
                no_create += 1;
            }
            st.ingest(&t)?;
            *done += 1;
        }
    }
    let created = st.creations_in_ts(from, until)?;
    println!("getTransaction: {fetched} (sin CreateEvent: {no_create}); creaciones con timestamp en [{from}, {until}): {}", created.len());
    if i < ok.len() {
        st.record_tramo(from, until, false, created.len(), 0, unix_now())?;
        anyhow::bail!(
            "tope --count {count} alcanzado con {} firmas del tramo sin pedir: el tramo queda INCOMPLETO (reanudable con un --count mayor)",
            ok.len() - i
        );
    }
    let (lo, hi) = (created.first().map(|c| c.0), created.last().map(|c| c.0));
    println!("tramo completo: {} creaciones, slots [{}, {}]", created.len(), lo.map_or("-".into(), |x| x.to_string()), hi.map_or("-".into(), |x| x.to_string()));
    // El tramo se define por el timestamp del CreateEvent; las firmas se
    // filtran por blockTime. Si difieren en el borde, se avisa.
    let bt: std::collections::HashMap<&str, i64> = ok.iter().filter_map(|s| s.block_time.map(|b| (s.signature.as_str(), b))).collect();
    let mismatch = st.creation_signatures_ts(&created.iter().map(|c| c.1.clone()).collect::<Vec<_>>())?
        .iter()
        .filter(|(sig, ts)| bt.get(sig.as_str()).is_some_and(|b| b != ts))
        .count();
    println!("creaciones con timestamp del CreateEvent ≠ blockTime de su firma: {mismatch}");
    st.record_tramo(from, until, true, created.len(), mismatch, unix_now())?;
    Ok(())
}

/// Ver `Cmd::IndexTramo`. Fase 1: listas de firmas hasta `from` (40 CU por
/// página de 1000 en Alchemy). Fase 2: `getTransaction` de las firmas
/// exitosas con blockTime ≥ `from`, de la más antigua a la más nueva y por
/// slots enteros, hasta que haya ≥ `count` creaciones con slot ≥ S2; así el
/// slot frontera queda completo para ordenar por (slot, mint).
#[allow(clippy::too_many_arguments)]
pub fn index_tramo(
    rpc: &RpcClient,
    idl: &Idl,
    st: &mut store::Store,
    from: i64,
    count: usize,
    until: Option<i64>,
    budget: &rpc::Budget,
) -> Result<()> {
    let address = pump::mint_authority_pda().to_string();
    with_budget(st, "index-tramo", budget, |st, done| {
        let (mut sigs, mut before, mut pages) = (Vec::new(), None::<String>, 0);
        let mut oldest_before_from: Option<(u64, i64)> = None;
        while oldest_before_from.is_none() {
            let page = rpc::with_retry(budget, rpc::Method::GetSignatures, &format!("firmas de {address}"), || {
                tx::recent_signatures(rpc, &address, 1000, before.as_deref())
            })?;
            pages += 1;
            let full = page.len() == 1000;
            before = page.last().map(|s| s.signature.clone());
            for s in page {
                let bt = s.block_time.with_context(|| format!("firma sin blockTime: {}", s.signature))?;
                if bt >= from {
                    // Con `until`, las firmas de blockTime ≥ until quedan fuera del tramo.
                    if until.is_none_or(|u| bt < u) {
                        sigs.push(s);
                    }
                } else if oldest_before_from.is_none() {
                    oldest_before_from = Some((s.slot, bt));
                }
            }
            if pages % 10 == 0 {
                eprintln!("… {pages} páginas, {} firmas con blockTime ≥ from", sigs.len());
            }
            if oldest_before_from.is_none() && !full {
                anyhow::bail!("el historial de firmas se acaba antes de llegar a from = {from}");
            }
        }
        let s2 = sigs.iter().map(|s| s.slot).min().context("ninguna firma con blockTime ≥ from")?;
        let s2_time = sigs.iter().filter(|s| s.slot == s2).filter_map(|s| s.block_time).min().unwrap();
        let (last_slot, last_time) = oldest_before_from.unwrap();
        let failed = sigs.iter().filter(|s| s.failed).count();
        println!("listas: {pages} páginas, {} firmas con blockTime ≥ {from} ({failed} fallidas, descartadas)", sigs.len());
        println!("S2 = slot {s2} (blockTime {s2_time}); última firma anterior: slot {last_slot} (blockTime {last_time})");

        let mut ok: Vec<&tx::SigInfo> = sigs.iter().filter(|s| !s.failed).collect();
        ok.sort_by(|a, b| (a.slot, &a.signature).cmp(&(b.slot, &b.signature)));
        if let Some(until) = until {
            return index_tramo_until(rpc, idl, st, from, until, count, &ok, budget, done);
        }
        let (mut fetched, mut no_create, mut i) = (0, 0, 0);
        while i < ok.len() && st.creations_in_slots(s2, ok[i].slot.saturating_sub(1))?.len() < count {
            let slot = ok[i].slot;
            while i < ok.len() && ok[i].slot == slot {
                let sig = &ok[i].signature;
                i += 1;
                if st.seen(sig)? {
                    continue;
                }
                let t = rpc::with_retry(budget, rpc::Method::GetTransaction, &format!("tx {sig}"), || tx::fetch_events(rpc, idl, sig))
                    .with_context(|| format!("getTransaction {sig} (el slot {slot} queda incompleto; reanudable)"))?;
                fetched += 1;
                for e in &t.decode_errors {
                    eprintln!("{sig}: error decodificando evento: {e}");
                }
                if !t.events.iter().any(|e| e.name == "CreateEvent") {
                    no_create += 1;
                }
                st.ingest(&t)?;
                *done += 1;
            }
        }
        let last_needed = ok.get(i.saturating_sub(1)).map(|s| s.slot).unwrap_or(s2);
        let created = st.creations_in_slots(s2, last_needed)?;
        println!("getTransaction: {fetched} (sin CreateEvent: {no_create}); creaciones con slot en [S2, {last_needed}]: {}", created.len());
        if created.len() < count {
            anyhow::bail!("solo hay {} creaciones con slot ≥ S2 (se piden {count})", created.len());
        }
        let (fs, fm, ft) = &created[count - 1];
        let in_frontier = created.iter().filter(|c| c.0 == *fs).count();
        println!(
            "creación n.º {count} en orden (slot, mint): slot {fs}, mint {fm}, timestamp {ft}; \
             creaciones en el slot frontera: {in_frontier}; indexadas fuera del tramo (tras la n.º {count}): {}",
            created.len() - count
        );
        Ok(())
    })
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

/// Holgura (s) al filtrar firmas por `blockTime`: el filtro exacto de la
/// ventana se hace con el `timestamp` del evento al ingerir.
pub const BLOCK_TIME_SLACK: i64 = 10;

/// Firmas de una bonding curve con `blockTime` en `[from, to]` (± holgura).
/// Las firmas llegan de más nueva a más antigua: se pagina hasta pasar
/// `from` o hasta la tx de creación (`creation_sig`), que se incluye si
/// `from` es la creación. `None` si `max_pages` no basta para llegar.
fn curve_signatures(
    rpc: &RpcClient,
    budget: &rpc::Budget,
    bonding_curve: &str,
    creation_sig: &str,
    from: i64,
    to: i64,
    max_pages: usize,
) -> Result<Option<Vec<tx::SigInfo>>> {
    let mut out = Vec::new();
    let (mut before, mut pages) = (None::<String>, 0);
    while pages < max_pages {
        let what = format!("firmas de la curva {bonding_curve}");
        let page = rpc::with_retry(budget, rpc::Method::GetSignatures, &what, || {
            tx::recent_signatures(rpc, bonding_curve, 1000, before.as_deref())
        })?;
        pages += 1;
        let mut reached = page.len() < 1000;
        before = page.last().map(|s| s.signature.clone());
        for s in page {
            if s.signature == creation_sig {
                if s.block_time.is_none_or(|bt| bt >= from - BLOCK_TIME_SLACK) {
                    out.push(s);
                }
                reached = true;
                break;
            }
            match s.block_time {
                Some(bt) if bt > to + BLOCK_TIME_SLACK => {}
                Some(bt) if bt < from - BLOCK_TIME_SLACK => {
                    reached = true;
                    break;
                }
                _ => out.push(s),
            }
        }
        if reached {
            return Ok(Some(out));
        }
    }
    Ok(None)
}

/// Descarga e ingiere las firmas de un token que aún no constan en
/// `fetched_sigs` para `purpose`. Devuelve (errores, filas nuevas) o `None`
/// si se agotó el tope de peticiones antes de terminar (el token queda
/// pendiente y la próxima pasada sigue donde se quedó).
fn fetch_token_sigs<'a>(
    rpc: &RpcClient,
    idl: &Idl,
    st: &mut store::Store,
    budget: &rpc::Budget,
    purpose: &str,
    mint: &str,
    sigs: impl Iterator<Item = &'a str>,
    prices_horizon: Option<i64>,
) -> Result<Option<(usize, usize)>> {
    let (mut errors, mut rows) = (0, 0);
    for sig in sigs {
        if st.fetched(purpose, sig)? {
            continue;
        }
        if !budget.has_room(1) {
            return Ok(None);
        }
        match rpc::with_retry(budget, rpc::Method::GetTransaction, &format!("mint {mint}, tx {sig}"), || {
            tx::fetch_events(rpc, idl, sig)
        }) {
            Ok(t) => {
                errors += t.decode_errors.len();
                let n = st.ingest(&t)?;
                rows += match prices_horizon {
                    Some(h) => st.ingest_prices(&t, h)?,
                    None => {
                        // Guarda también el precio inicial del CreateEvent (T_entry2).
                        st.ingest_prices(&t, store::EARLY_WINDOW_SECS)?;
                        n.early_trades
                    }
                };
                if t.decode_errors.is_empty() {
                    st.mark_fetched(purpose, sig)?;
                }
            }
            Err(e) => {
                errors += 1;
                eprintln!("{sig}: {e:#}");
            }
        }
    }
    Ok(Some((errors, rows)))
}

/// Ejecuta una descarga con presupuesto y deja constancia del consumo real
/// en `rpc_usage`, también si la descarga termina con error.
fn with_budget(
    st: &mut store::Store,
    command: &str,
    budget: &rpc::Budget,
    f: impl FnOnce(&mut store::Store, &mut usize) -> Result<()>,
) -> Result<()> {
    // `tokens` se actualiza sobre la marcha: si la ejecución termina en error
    // se registran igual los tokens que llegó a procesar.
    let mut tokens = 0;
    let r = f(st, &mut tokens);
    st.record_rpc_usage(command, budget, tokens, unix_now())?;
    eprintln!("consumo de esta ejecución: {}", budget.summary());
    for w in budget.retry_summary().into_iter().chain(tx::TRADE_EXTRA_BYTES.summary()) {
        eprintln!("{w}");
    }
    for (prov, gt, gs, units) in st.rpc_usage_totals()? {
        eprintln!("acumulado {prov}: {gt} getTransaction, {gs} getSignaturesForAddress, {units} unidades");
    }
    r
}

pub fn windows(rpc: &RpcClient, idl: &Idl, st: &mut store::Store, limit: usize, max_pages: usize, budget: &rpc::Budget) -> Result<()> {
    with_budget(st, "windows", budget, |st, done| {
        let now = unix_now();
        let pending = st.pending_windows(now, limit)?;
        eprintln!("{} tokens con ventana cerrada pendientes de descarga", pending.len());
        let (mut complete, mut incomplete, mut trades) = (0, 0, 0);
        for w in pending {
            if !budget.has_room(1) {
                eprintln!("tope de peticiones alcanzado: se para aquí (reanudable)");
                break;
            }
            let end = w.created_at + store::EARLY_WINDOW_SECS;
            let sigs = match curve_signatures(rpc, budget, &w.bonding_curve, &w.signature, w.created_at, end, max_pages) {
                Ok(s) => s,
                Err(e) => {
                    // El token queda pendiente; la próxima pasada lo reintenta.
                    eprintln!("{}: error listando firmas, queda pendiente: {e:#}", w.mint);
                    st.note_attempt("window", &w.mint)?;
                    incomplete += 1;
                    continue;
                }
            };
            let Some(in_window) = sigs else {
                incomplete += 1;
                st.record_window(&w.mint, 0, 0, false, Some("max_pages sin llegar a la creación"), now)?;
                eprintln!("{}: ventana incompleta (max_pages sin llegar a la creación)", w.mint);
                continue;
            };
            let sigs = in_window.iter().filter(|s| !s.failed).map(|s| s.signature.as_str());
            let Some((errors, n)) = fetch_token_sigs(rpc, idl, st, budget, "window", &w.mint, sigs, None)? else {
                eprintln!("{}: tope de peticiones a mitad de token: queda pendiente (reanudable)", w.mint);
                break;
            };
            trades += n;
            *done += 1;
            let ok = errors == 0;
            if ok { complete += 1 } else { incomplete += 1 }
            let note = (!ok).then_some("errores de getTransaction o decodificación");
            st.record_window(&w.mint, in_window.len(), errors, ok, note, now)?;
        }
        println!("ventanas completas: {complete}, incompletas: {incomplete}, trades nuevos: {trades}");
        Ok(())
    })
}

/// Horizonte de descarga de precio: 1 h de H1b más la ventana temprana, para
/// que el retorno a +60 min desde T_entry2 (≤ 5 min) quede cubierto.
pub const PRICE_DOWNLOAD_SECS: i64 = h1b::PARAMS_1H.horizon_secs + store::EARLY_WINDOW_SECS;

pub fn prices(rpc: &RpcClient, idl: &Idl, st: &mut store::Store, limit: usize, max_pages: usize, budget: &rpc::Budget) -> Result<()> {
    with_budget(st, "prices", budget, |st, done| {
        let horizon = PRICE_DOWNLOAD_SECS;
        let now = unix_now();
        let copied = st.backfill_prices_from_early_trades()?;
        let pending = st.pending_price_windows(now, horizon, limit)?;
        eprintln!("{copied} puntos copiados de la ventana temprana; {} tokens pendientes", pending.len());
        let (mut complete, mut incomplete, mut points) = (0, 0, 0);
        for w in pending {
            if !budget.has_room(1) {
                eprintln!("tope de peticiones alcanzado: se para aquí (reanudable)");
                break;
            }
            // Los 5 primeros minutos ya están en early_trades: solo se piden las
            // firmas posteriores, más la creación (precio inicial del CreateEvent).
            let from = w.created_at + store::EARLY_WINDOW_SECS;
            let listed =
                match curve_signatures(rpc, budget, &w.bonding_curve, &w.signature, from, w.created_at + horizon, max_pages) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("{}: error listando firmas, queda pendiente: {e:#}", w.mint);
                        st.note_attempt("prices", &w.mint)?;
                        incomplete += 1;
                        continue;
                    }
                };
            let Some(sigs) = listed else {
                incomplete += 1;
                st.record_price_window(&w.mint, horizon, 0, 0, false, Some("max_pages sin llegar al rango"), now)?;
                eprintln!("{}: ventana de precio incompleta (max_pages)", w.mint);
                continue;
            };
            let fetch = std::iter::once(w.signature.as_str())
                .chain(sigs.iter().filter(|s| !s.failed).map(|s| s.signature.as_str()));
            let Some((errors, n)) = fetch_token_sigs(rpc, idl, st, budget, "prices", &w.mint, fetch, Some(horizon))? else {
                eprintln!("{}: tope de peticiones a mitad de token: queda pendiente (reanudable)", w.mint);
                break;
            };
            points += n;
            *done += 1;
            let ok = errors == 0;
            if ok { complete += 1 } else { incomplete += 1 }
            let note = (!ok).then_some("errores de getTransaction o decodificación");
            st.record_price_window(&w.mint, horizon, sigs.len(), errors, ok, note, now)?;
        }
        println!("ventanas de precio completas: {complete}, incompletas: {incomplete}, puntos nuevos: {points}");
        Ok(())
    })
}
