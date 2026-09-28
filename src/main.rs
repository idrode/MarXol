//! marXol — CLI de búsqueda y análisis on-chain de operadores de pump.fun.

mod h1;
mod h1b;
mod idl;
mod pump;
mod store;
mod tx;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use idl::{Decoded, Idl};
use pump::{Graduation, LAMPORTS_PER_SOL};
use solana_commitment_config::CommitmentConfig;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use std::str::FromStr;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "marxol", version, about)]
struct Cli {
    /// Endpoint RPC de Solana (Alchemy, Helius, público...).
    #[arg(long, env = "MARXOL_RPC_URL", default_value = "https://api.mainnet-beta.solana.com")]
    rpc: String,
    /// Base de datos SQLite local.
    #[arg(long, env = "MARXOL_DB", default_value = "marxol.db", global = true)]
    db: String,
    /// Salida en JSON en vez de texto.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Verifica contra la chain los supuestos de CLAUDE.md sección 9.
    Verify,
    /// Lee y decodifica la cuenta Global (parámetros económicos en vivo).
    Global,
    /// Lee la bonding curve de un mint.
    Curve { mint: String },
    /// Decodifica los eventos pump.fun de una transacción.
    Tx { signature: String },
    /// Recorre transacciones recientes del programa y muestra eventos.
    Scan {
        /// Dirección cuyas firmas se recorren (por defecto, el programa pump.fun).
        /// Útil para ir directo a un creador, un mint o una autoridad del Global.
        #[arg(long, default_value = pump::PROGRAM_ID)]
        address: String,
        /// Número de firmas a recorrer.
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Solo mostrar eventos con estos nombres (por defecto: identidad + ciclo de vida).
        #[arg(long, value_delimiter = ',')]
        events: Option<Vec<String>>,
        /// Mostrar todos los eventos, incluido TradeEvent.
        #[arg(long)]
        all: bool,
        /// Pausa entre getTransaction (ms), para no saturar RPC públicos.
        #[arg(long, default_value_t = 150)]
        delay_ms: u64,
    },
    /// Indexa en la base local los eventos de identidad y ciclo de vida de las
    /// firmas de una dirección, paginando hacia atrás y saltando las ya vistas.
    Index {
        #[arg(long, default_value = pump::PROGRAM_ID)]
        address: String,
        /// Número máximo de firmas a recorrer en total.
        #[arg(long, default_value_t = 200)]
        limit: usize,
        #[arg(long, default_value_t = 150)]
        delay_ms: u64,
    },
    /// Descarga los trades de la ventana temprana (H1) de los tokens indexados
    /// cuya ventana ya se cerró, recorriendo las firmas de su bonding curve.
    Windows {
        /// Máximo de tokens a procesar en esta pasada.
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Máximo de páginas (1000 firmas) por bonding curve. Si no basta para
        /// llegar a la creación, la ventana queda incompleta.
        #[arg(long, default_value_t = 20)]
        max_pages: usize,
        #[arg(long, default_value_t = 150)]
        delay_ms: u64,
    },
    /// Calcula la señal H1 sobre los tokens con ventana temprana completa.
    H1 {
        /// Variante del criterio (CLAUDE.md 8): v1 (pre-registrada) o h1c.
        #[arg(long, default_value = "v1")]
        variant: String,
    },
    /// Descarga la trayectoria de precio de la ventana de H1b (1 h) de los
    /// tokens con H1 calculada. Reutiliza los trades de la ventana temprana.
    Prices {
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, default_value_t = 20)]
        max_pages: usize,
        #[arg(long, default_value_t = 150)]
        delay_ms: u64,
    },
    /// Calcula H1b (1 h) y la cruza con H1: tablas 2×2 y test exacto de Fisher.
    H1b {
        /// Variante de H1 con la que se cruza (debe estar calculada con `h1`).
        #[arg(long, default_value = "v1")]
        variant: String,
    },
    /// Informe de un operador (identidad = campo `creator`, no el payer).
    Operator { creator: String },
    /// Recuento de filas de la base local.
    Stats,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let rpc = RpcClient::new_with_timeout_and_commitment(
        cli.rpc.clone(),
        Duration::from_secs(30),
        CommitmentConfig::confirmed(),
    );
    let idl = Idl::pump()?;
    match cli.cmd {
        Cmd::Verify => verify(&rpc, &idl),
        Cmd::Global => {
            let g = read_account(&rpc, &idl, pump::GLOBAL_PDA)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&g.to_json())?);
            } else {
                print_decoded(&g);
                print_graduation(&Graduation::from_global(&g)?);
            }
            Ok(())
        }
        Cmd::Curve { mint } => {
            let mint = Pubkey::from_str(&mint).context("mint inválido")?;
            let pda = pump::bonding_curve_pda(&mint);
            let c = read_account(&rpc, &idl, &pda.to_string())?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&c.to_json())?);
            } else {
                println!("bonding_curve: {pda}");
                print_decoded(&c);
            }
            Ok(())
        }
        Cmd::Index { address, limit, delay_ms } => {
            let mut st = store::Store::open(&cli.db)?;
            index(&rpc, &idl, &mut st, &address, limit, delay_ms)
        }
        Cmd::Windows { limit, max_pages, delay_ms } => {
            let mut st = store::Store::open(&cli.db)?;
            windows(&rpc, &idl, &mut st, limit, max_pages, delay_ms)
        }
        Cmd::H1 { variant } => {
            let st = store::Store::open(&cli.db)?;
            h1_report(&st, cli.json, &variant)
        }
        Cmd::Prices { limit, max_pages, delay_ms } => {
            let mut st = store::Store::open(&cli.db)?;
            prices(&rpc, &idl, &mut st, limit, max_pages, delay_ms)
        }
        Cmd::H1b { variant } => {
            let st = store::Store::open(&cli.db)?;
            h1b_report(&st, cli.json, &variant)
        }
        Cmd::Operator { creator } => {
            let st = store::Store::open(&cli.db)?;
            let r = st.operator_report(&creator)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print_operator(&r);
            }
            Ok(())
        }
        Cmd::Stats => {
            let st = store::Store::open(&cli.db)?;
            for (t, n) in st.counts()? {
                println!("{t:18} {n}");
            }
            Ok(())
        }
        Cmd::Tx { signature } => {
            let t = tx::fetch_events(&rpc, &idl, &signature)?;
            print_tx(&t, None, cli.json);
            Ok(())
        }
        Cmd::Scan { address, limit, events, all, delay_ms } => {
            let filter: Option<Vec<String>> = if all {
                None
            } else {
                Some(events.unwrap_or_else(|| {
                    pump::IDENTITY_EVENTS
                        .iter()
                        .chain(pump::LIFECYCLE_EVENTS)
                        .map(|s| s.to_string())
                        .collect()
                }))
            };
            let sigs = tx::recent_signatures(&rpc, &address, limit, None)?;
            let mut counts = std::collections::BTreeMap::<String, usize>::new();
            let (mut errors, mut failed_txs, mut fetched) = (0, 0, 0);
            for s in sigs {
                if s.failed {
                    failed_txs += 1;
                    continue;
                }
                let sig = s.signature;
                fetched += 1;
                std::thread::sleep(Duration::from_millis(delay_ms));
                match tx::fetch_events(&rpc, &idl, &sig) {
                    Ok(t) => {
                        for e in &t.events {
                            *counts.entry(e.name.clone()).or_default() += 1;
                        }
                        errors += t.decode_errors.len();
                        print_tx(&t, filter.as_deref(), cli.json);
                    }
                    Err(e) => eprintln!("{sig}: {e:#}"),
                }
            }
            if !cli.json {
                eprintln!(
                    "\n{fetched} tx leídas, {failed_txs} fallidas omitidas; eventos: {counts:?}; \
                     errores de decodificación: {errors}"
                );
            }
            Ok(())
        }
    }
}

fn index(rpc: &RpcClient, idl: &Idl, st: &mut store::Store, address: &str, limit: usize, delay_ms: u64) -> Result<()> {
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

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

/// Holgura (s) al filtrar firmas por `blockTime`: el filtro exacto de la
/// ventana se hace con el `timestamp` del evento al ingerir.
const BLOCK_TIME_SLACK: i64 = 10;

/// Firmas de una bonding curve con `blockTime` en `[from, to]` (± holgura).
/// Las firmas llegan de más nueva a más antigua: se pagina hasta pasar
/// `from` o hasta la tx de creación (`creation_sig`), que se incluye si
/// `from` es la creación. `None` si `max_pages` no basta para llegar.
fn curve_signatures(
    rpc: &RpcClient,
    bonding_curve: &str,
    creation_sig: &str,
    from: i64,
    to: i64,
    max_pages: usize,
    delay_ms: u64,
) -> Result<Option<Vec<tx::SigInfo>>> {
    let mut out = Vec::new();
    let (mut before, mut pages) = (None::<String>, 0);
    while pages < max_pages {
        let page = tx::recent_signatures(rpc, bonding_curve, 1000, before.as_deref())?;
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
        std::thread::sleep(Duration::from_millis(delay_ms));
    }
    Ok(None)
}

fn windows(rpc: &RpcClient, idl: &Idl, st: &mut store::Store, limit: usize, max_pages: usize, delay_ms: u64) -> Result<()> {
    let now = unix_now();
    let pending = st.pending_windows(now, limit)?;
    eprintln!("{} tokens con ventana cerrada pendientes de descarga", pending.len());
    let (mut complete, mut incomplete, mut trades) = (0, 0, 0);
    for w in pending {
        let end = w.created_at + store::EARLY_WINDOW_SECS;
        let Some(in_window) =
            curve_signatures(rpc, &w.bonding_curve, &w.signature, w.created_at, end, max_pages, delay_ms)?
        else {
            incomplete += 1;
            st.record_window(&w.mint, 0, 0, false, Some("max_pages sin llegar a la creación"), now)?;
            eprintln!("{}: ventana incompleta (max_pages sin llegar a la creación)", w.mint);
            continue;
        };
        let mut errors = 0;
        for s in in_window.iter().filter(|s| !s.failed) {
            std::thread::sleep(Duration::from_millis(delay_ms));
            match tx::fetch_events(rpc, idl, &s.signature) {
                Ok(t) => {
                    errors += t.decode_errors.len();
                    trades += st.ingest(&t)?.early_trades;
                }
                Err(e) => {
                    errors += 1;
                    eprintln!("{}: {e:#}", s.signature);
                }
            }
        }
        let ok = errors == 0;
        if ok { complete += 1 } else { incomplete += 1 }
        let note = (!ok).then_some("errores de getTransaction o decodificación");
        st.record_window(&w.mint, in_window.len(), errors, ok, note, now)?;
    }
    println!("ventanas completas: {complete}, incompletas: {incomplete}, trades nuevos: {trades}");
    Ok(())
}

fn prices(rpc: &RpcClient, idl: &Idl, st: &mut store::Store, limit: usize, max_pages: usize, delay_ms: u64) -> Result<()> {
    let horizon = h1b::PARAMS_1H.horizon_secs;
    let now = unix_now();
    let copied = st.backfill_prices_from_early_trades()?;
    let pending = st.pending_price_windows(now, horizon, limit)?;
    eprintln!("{copied} puntos copiados de la ventana temprana; {} tokens pendientes", pending.len());
    let (mut complete, mut incomplete, mut points) = (0, 0, 0);
    for w in pending {
        // Los 5 primeros minutos ya están en early_trades: solo se piden las
        // firmas posteriores, más la creación (precio inicial del CreateEvent).
        let from = w.created_at + store::EARLY_WINDOW_SECS;
        let Some(sigs) = curve_signatures(
            rpc,
            &w.bonding_curve,
            &w.signature,
            from,
            w.created_at + horizon,
            max_pages,
            delay_ms,
        )?
        else {
            incomplete += 1;
            st.record_price_window(&w.mint, horizon, 0, 0, false, Some("max_pages sin llegar al rango"), now)?;
            eprintln!("{}: ventana de precio incompleta (max_pages)", w.mint);
            continue;
        };
        let mut errors = 0;
        let fetch = std::iter::once(w.signature.as_str())
            .chain(sigs.iter().filter(|s| !s.failed).map(|s| s.signature.as_str()));
        for sig in fetch {
            std::thread::sleep(Duration::from_millis(delay_ms));
            match tx::fetch_events(rpc, idl, sig) {
                Ok(t) => {
                    errors += t.decode_errors.len();
                    st.ingest(&t)?;
                    points += st.ingest_prices(&t, horizon)?;
                }
                Err(e) => {
                    errors += 1;
                    eprintln!("{sig}: {e:#}");
                }
            }
        }
        let ok = errors == 0;
        if ok { complete += 1 } else { incomplete += 1 }
        let note = (!ok).then_some("errores de getTransaction o decodificación");
        st.record_price_window(&w.mint, horizon, sigs.len(), errors, ok, note, now)?;
    }
    println!("ventanas de precio completas: {complete}, incompletas: {incomplete}, puntos nuevos: {points}");
    Ok(())
}

fn h1b_report(st: &store::Store, json: bool, variant: &str) -> Result<()> {
    let p = h1b::PARAMS_1H;
    let rows: Vec<_> = st
        .h1b_inputs(p.horizon_secs, variant)?
        .into_iter()
        .map(|i| {
            let r = h1b::compute(i.t0, i.initial, &i.points, i.completed_at, i.migrated_at, &p);
            (i.mint, i.h1, r)
        })
        .collect();
    if json {
        let v: Vec<_> =
            rows.iter().map(|(m, h1, r)| serde_json::json!({"mint": m, "h1": h1, "h1b": r})).collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!("H1 variante {variant}; H1b parámetros [P]: {}", serde_json::to_string(&p)?);
    println!("{:46} {:>3} {:>5} {:>9} {:>8} {:>7} {:>6}", "mint", "H1", "grad", "sostenido", "pico×", "caída", "puntos");
    for (m, h1, r) in &rows {
        let s = match r.sustained {
            Some(true) => "sí",
            Some(false) => "no",
            None => "n/e",
        };
        println!(
            "{m:46} {:>3} {:>5} {s:>9} {:>8.2} {:>6.0}% {:>6}",
            if *h1 { "sí" } else { "no" },
            if r.graduated { "sí" } else { "no" },
            r.peak_multiple,
            100.0 * r.drawdown,
            r.points
        );
    }
    let table = |ok: &dyn Fn(&h1b::H1bResult) -> Option<bool>| {
        let mut t = h1b::Table::default();
        for (_, h1, r) in &rows {
            match (h1, ok(r)) {
                (true, Some(true)) => t.a += 1,
                (true, Some(false)) => t.b += 1,
                (false, Some(true)) => t.c += 1,
                (false, Some(false)) => t.d += 1,
                (_, None) => {}
            }
        }
        t
    };
    for (name, t) in [
        ("graduación en 1 h", table(&|r| Some(r.graduated))),
        ("pump sostenido en 1 h", table(&|r| r.sustained)),
    ] {
        let rate = |x: u64, y: u64| if x + y == 0 { f64::NAN } else { 100.0 * x as f64 / (x + y) as f64 };
        let (or, lo, hi) = t.odds_ratio();
        println!("\nH1 × {name} (n = {}):", t.a + t.b + t.c + t.d);
        println!("              éxito  no-éxito   tasa");
        println!("  H1 = true   {:5}  {:8}  {:5.1}%", t.a, t.b, rate(t.a, t.b));
        println!("  H1 = false  {:5}  {:8}  {:5.1}%", t.c, t.d, rate(t.c, t.d));
        println!("  Fisher bilateral p = {:.4}   odds ratio = {or:.2} (IC95 {lo:.2}–{hi:.2})", t.fisher_p());
    }

    // Pump-y-caída (CLAUDE.md 8, H1c): pico ≥ 2× y caída > 30 % al cierre.
    let pump_dump = |r: &h1b::H1bResult| r.peak_multiple >= p.min_peak_multiple && r.drawdown > p.max_drawdown;
    println!("\npump-y-caída en 1 h por grupo de H1:");
    for g in [true, false] {
        let grp: Vec<_> = rows.iter().filter(|(_, h1, _)| *h1 == g).collect();
        let pumps = grp.iter().filter(|(_, _, r)| r.peak_multiple >= p.min_peak_multiple).count();
        let pd = grp.iter().filter(|(_, _, r)| pump_dump(r)).count();
        println!(
            "  H1 = {g:5}: {:2} tokens, {pumps:2} con pico ≥ 2×, {pd:2} pump-y-caída ({:.0}% del grupo)",
            grp.len(),
            if grp.is_empty() { f64::NAN } else { 100.0 * pd as f64 / grp.len() as f64 }
        );
    }

    // Criterio (a) de H1c: fracción de pares 1+1 rápidos por token, en
    // tokens pump-y-caída frente al resto. Independiente de la variante.
    let fast = h1::PARAMS_H1C.fast_pair_secs.unwrap();
    let mut by_group: [Vec<(usize, usize)>; 2] = [vec![], vec![]];
    for (mint, _, r) in &rows {
        let t0 = st.creation_time(mint)?;
        let trades = st.window_trades(mint, t0, h1::PARAMS_V1.window_secs)?;
        let dts = h1::pair_intervals(&trades, &st.creator_identities(mint)?);
        let k = dts.iter().filter(|&&d| d < fast).count();
        by_group[pump_dump(r) as usize].push((dts.len(), k));
    }
    println!("\nH1c (a): wallets 1+1 y pares rápidos (< {fast} s) por token:");
    for (name, g) in [("pump-y-caída", &by_group[1]), ("resto", &by_group[0])] {
        let pairs: usize = g.iter().map(|x| x.0).sum();
        let fastn: usize = g.iter().map(|x| x.1).sum();
        let mut n1: Vec<i64> = g.iter().map(|x| x.0 as i64).collect();
        n1.sort();
        let mut fr: Vec<i64> =
            g.iter().filter(|x| x.0 > 0).map(|x| (1000 * x.1 / x.0) as i64).collect();
        fr.sort();
        let med = |v: &[i64]| if v.is_empty() { f64::NAN } else { percentile(v, 0.5) as f64 };
        println!(
            "  {name:13} {:2} tokens: wallets 1+1 mediana {:.0} (total {pairs}); rápidos {fastn} = {:.1}% agregado, mediana por token {:.1}%",
            g.len(),
            med(&n1),
            if pairs == 0 { f64::NAN } else { 100.0 * fastn as f64 / pairs as f64 },
            med(&fr) / 10.0
        );
    }
    Ok(())
}

/// Percentil por el método del rango más cercano; `xs` ordenado.
fn percentile(xs: &[i64], q: f64) -> i64 {
    xs[((q * xs.len() as f64).ceil() as usize).clamp(1, xs.len()) - 1]
}

fn h1_report(st: &store::Store, json: bool, variant: &str) -> Result<()> {
    let p = h1::variant(variant).with_context(|| format!("variante desconocida: {variant} (v1, h1c)"))?;
    let params_json = serde_json::to_string(&p)?;
    let now = unix_now();
    let mut results = Vec::new();
    let mut intervals = Vec::new();
    for (mint, t0) in st.evaluable_mints()? {
        let trades = st.window_trades(&mint, t0, p.window_secs)?;
        let ids = st.creator_identities(&mint)?;
        let r = h1::compute(&trades, &ids, &p);
        st.save_h1(&mint, variant, &params_json, &r, now)?;
        intervals.extend(h1::pair_intervals(&trades, &ids));
        results.push((mint, r));
    }
    if json {
        let v: Vec<_> = results.iter().map(|(m, r)| serde_json::json!({"mint": m, "h1": r})).collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    let (created, closed, complete, incomplete) = st.window_coverage(now)?;
    println!("variante {variant}, parámetros [P]: {params_json}");
    println!("tokens indexados: {created}  ventana cerrada: {closed}  evaluables: {complete}  incompletos: {incomplete}");
    let n = results.len();
    let count = |f: &dyn Fn(&h1::H1Result) -> bool| results.iter().filter(|(_, r)| f(r)).count();
    let sum = |f: &dyn Fn(&h1::H1Result) -> usize| results.iter().map(|(_, r)| f(r)).sum::<usize>();
    println!("  sin ningún trade en la ventana:          {}", count(&|r| r.trades == 0));
    println!("  sin trades ajenos al creador:            {}", count(&|r| r.trades == r.creator_trades));
    println!("  con alguna wallet bot:                   {}", count(&|r| r.bot_wallets > 0));
    println!(
        "  wallets excluidas: {} bot (de ellas {} pares 1+1 rápidos), {} trades de ballena",
        sum(&|r| r.bot_wallets),
        sum(&|r| r.fast_pair_wallets),
        sum(&|r| r.whale_trades)
    );
    println!("  con algún trade de ballena excluido:     {}", count(&|r| r.whale_trades > 0));
    for k in 0..p.min_organic_wallets {
        println!("  wallets orgánicas = {k}:                   {}", count(&|r| r.organic_wallets == k));
    }
    let pos = count(&|r| r.signal);
    println!("  wallets orgánicas ≥ {} (H1 = true):       {pos} de {n}", p.min_organic_wallets);

    // Distribución de H1c: intervalo compra↔venta de las wallets 1+1 ajenas
    // al creador, antes de cualquier otra exclusión.
    intervals.sort();
    if !intervals.is_empty() {
        println!("\nintervalo compra↔venta de wallets 1+1 (n = {}), segundos:", intervals.len());
        let ps = [0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95];
        let line: Vec<String> =
            ps.iter().map(|&q| format!("p{:.0}={}", q * 100.0, percentile(&intervals, q))).collect();
        println!("  {}  máx={}", line.join("  "), intervals.last().unwrap());
        let edges = [0, 1, 3, 5, 10, 20, 30, 60, 120, 300];
        let total = intervals.len() as f64;
        for w in edges.windows(2).map(|w| (w[0], w[1])).chain([(300, i64::MAX)]) {
            let k = intervals.iter().filter(|&&x| x >= w.0 && x < w.1).count();
            let label = if w.1 == i64::MAX { format!("≥{}", w.0) } else { format!("[{}, {})", w.0, w.1) };
            let cum = intervals.iter().filter(|&&x| x < w.1).count() as f64 / total;
            println!("  {label:>10} {k:5}  {:5.1}%  acum {:5.1}%", 100.0 * k as f64 / total, 100.0 * cum);
        }
    }
    Ok(())
}

fn print_operator(r: &store::OperatorReport) {
    let completed = r.created.iter().filter(|t| t.completed_at.is_some()).count();
    let migrated = r.created.iter().filter(|t| t.migrated_at.is_some()).count();
    let foreign_payer = r.created.iter().filter(|t| t.payer != r.creator).count();
    println!("operador {}", r.creator);
    println!(
        "  tokens creados (creator original): {}  completados: {completed}  migrados: {migrated}  \
         pagados por otra wallet: {foreign_payer}",
        r.created.len()
    );
    for t in &r.created {
        println!(
            "  {} {:10} creado={} completado={:?} migrado={:?} cambios_creador={}{}",
            t.mint,
            t.symbol.as_deref().unwrap_or("?"),
            t.created_at,
            t.completed_at,
            t.migrated_at,
            t.creator_changes,
            if t.payer != r.creator { format!(" payer={}", t.payer) } else { String::new() }
        );
    }
    println!("  cambios de identidad que le afectan: {}", r.changes.len());
    for c in &r.changes {
        println!(
            "  [{}] {} {:?} -> {} sharing_config={:?} t={} {}",
            c.kind, c.mint, c.old_creator, c.new_creator, c.sharing_config, c.timestamp, c.signature
        );
    }
    println!("  creaciones pagadas para otro creator: {}", r.paid_for_others.len());
    for p in &r.paid_for_others {
        println!(
            "  {} {:10} creator={} t={}",
            p.mint,
            p.symbol.as_deref().unwrap_or("?"),
            p.creator,
            p.timestamp
        );
    }
    println!("  (solo cubre lo indexado en la base local; ver `marxol stats`)");
}

fn read_account(rpc: &RpcClient, idl: &Idl, address: &str) -> Result<Decoded> {
    let pk = Pubkey::from_str(address)?;
    let acc = rpc.get_account(&pk).with_context(|| format!("getAccountInfo {address}"))?;
    anyhow::ensure!(
        acc.owner == pump::program_id(),
        "la cuenta {address} no pertenece al programa pump.fun (owner {})",
        acc.owner
    );
    idl.decode_account(&acc.data)
}

fn verify(rpc: &RpcClient, idl: &Idl) -> Result<()> {
    let ok = |b: bool| if b { "OK  " } else { "FALLO" };

    let prog = rpc.get_account(&pump::program_id())?;
    println!(
        "[{}] program {} ejecutable={} owner={}",
        ok(prog.executable),
        pump::PROGRAM_ID,
        prog.executable,
        prog.owner
    );

    println!(
        "[{}] IDL embebido: address={} ({} instrucciones, {} cuentas, {} eventos, {} tipos)",
        ok(idl.address == pump::PROGRAM_ID),
        idl.address,
        idl.instructions.len(),
        idl.accounts.len(),
        idl.events.len(),
        idl.types.len()
    );

    let derived = pump::global_pda();
    println!("[{}] Global PDA derivada = {derived}", ok(derived.to_string() == pump::GLOBAL_PDA));
    println!("      mint-authority PDA = {} (para `index --address`)", pump::mint_authority_pda());

    let g = read_account(rpc, idl, pump::GLOBAL_PDA)?;
    println!(
        "[{}] Global decodificado ({} campos, faltan {:?}, {} bytes sobrantes)",
        ok(g.missing.is_empty()),
        g.fields.len(),
        g.missing,
        g.trailing_bytes
    );
    print_graduation(&Graduation::from_global(&g)?);
    Ok(())
}

fn print_decoded(d: &Decoded) {
    println!("{}:", d.name);
    for (k, v) in &d.fields {
        println!("  {k:34} {v}");
    }
    if !d.missing.is_empty() {
        println!("  (campos del IDL ausentes en los datos: {:?})", d.missing);
    }
}

fn print_graduation(g: &Graduation) {
    println!("graduación (derivada de Global):");
    println!(
        "  SOL real al completar la curva     {:.4} SOL",
        g.real_sol_at_completion as f64 / LAMPORTS_PER_SOL
    );
    println!("  precio inicial / final             {:.3e} / {:.3e} SOL/token", g.start_price_sol, g.final_price_sol);
    println!("  mcap inicial / al completar        {:.2} / {:.2} SOL", g.start_mcap_sol, g.final_mcap_sol);
}

fn print_tx(t: &tx::TxEvents, filter: Option<&[String]>, json: bool) {
    let shown: Vec<&Decoded> = t
        .events
        .iter()
        .filter(|e| filter.is_none_or(|f| f.iter().any(|n| n == &e.name)))
        .collect();
    if filter.is_some() && shown.is_empty() && t.decode_errors.is_empty() {
        return;
    }
    if json {
        let v = serde_json::json!({
            "signature": t.signature,
            "slot": t.slot,
            "block_time": t.block_time,
            "failed": t.failed,
            "instructions": t.instructions,
            "events": shown.iter().map(|e| e.to_json()).collect::<Vec<_>>(),
            "decode_errors": t.decode_errors,
        });
        println!("{v}");
        return;
    }
    println!(
        "{} slot={} t={:?}{} ix={:?}",
        t.signature,
        t.slot,
        t.block_time,
        if t.failed { " FALLIDA" } else { "" },
        t.instructions
    );
    for e in shown {
        print_decoded(e);
    }
    for err in &t.decode_errors {
        println!("  ERROR decodificando evento: {err}");
    }
}
