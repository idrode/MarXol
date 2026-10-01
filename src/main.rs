//! marXol — CLI de búsqueda y análisis on-chain de operadores de pump.fun.

mod entry;
mod entry2;
mod h1;
mod h1b;
mod idl;
mod pump;
mod replica;
mod rpc;
mod store;
mod tx;
mod v3;

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

/// Ritmo y tope de peticiones de las descargas (`windows`, `prices`).
/// Ambas son reanudables: saltan tokens completos y firmas ya ingeridas.
#[derive(clap::Args)]
struct RateArgs {
    /// Proveedor RPC, para el ritmo por defecto y el coste por método:
    /// auto (según la URL), public, alchemy, helius.
    #[arg(long, env = "MARXOL_PROVIDER", default_value = "auto")]
    provider: String,
    /// Peticiones por segundo (por defecto, el del proveedor).
    #[arg(long)]
    max_rps: Option<f64>,
    /// Tope de peticiones de esta ejecución; al alcanzarlo se para limpiamente.
    #[arg(long)]
    max_requests: Option<u64>,
}

impl RateArgs {
    fn budget(&self, rpc_url: &str) -> Result<rpc::Budget> {
        Ok(rpc::Budget::new(rpc::Provider::parse(&self.provider, rpc_url)?, self.max_rps, self.max_requests))
    }
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
    /// Indexa un tramo por tiempo (CLAUDE.md 8, validación 2): recorre solo las
    /// listas de firmas de la PDA mint-authority hasta `--from` (blockTime) y
    /// pide la transacción solo de las firmas exitosas más antiguas con
    /// blockTime ≥ `--from`, slot a slot (cada slot entero), hasta reunir
    /// `--count` creaciones. Reanudable: salta firmas ya vistas.
    IndexTramo {
        /// Unix time de inicio del tramo; S2 = primer slot con blockTime ≥ esto.
        #[arg(long)]
        from: i64,
        #[arg(long, default_value_t = 400)]
        count: usize,
        /// Cierra el tramo por tiempo (validación 3): solo firmas con
        /// blockTime < esto, todas; `count` pasa a ser un tope holgado (si se
        /// alcanza antes, el tramo queda incompleto y se avisa). Sin esto, el
        /// comportamiento es el de la validación 2 (primeras `count` creaciones).
        #[arg(long)]
        until: Option<i64>,
        #[command(flatten)]
        rate: RateArgs,
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
        #[command(flatten)]
        rate: RateArgs,
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
        #[command(flatten)]
        rate: RateArgs,
    },
    /// Calcula H1b (1 h) y la cruza con H1: tablas 2×2 y test exacto de Fisher.
    H1b {
        /// Variante de H1 con la que se cruza (debe estar calculada con `h1`).
        #[arg(long, default_value = "v1")]
        variant: String,
    },
    /// Punto de entrada hipotético T_entry e indicadores previos H4–H7,
    /// cruzados con pump-y-caída en 1 h (CLAUDE.md 8). Sin RPC.
    Entry,
    /// Validación congelada (CLAUDE.md 8): T_entry2 e indicadores H4', H5,
    /// H6', H7', H8 contra pump-y-caída en 1 h, y retorno neto desde T_entry2.
    /// Sin RPC.
    Entry2 {
        /// Solo chequeo de no-degeneración: cuenta grupos y distribuciones sin
        /// calcular ni mirar el resultado.
        #[arg(long)]
        check: bool,
        /// Tramo: validacion (validación 1: slot en (último del piloto, último
        /// de la validación 1]), validacion2 (slots [S2, creación n.º 400]; sin
        /// --check calcula el veredicto de réplica), validacion3 (timestamp en
        /// [09:00, 09:08) UTC del 2026-10-01; filtro D2) o piloto. El piloto solo
        /// admite --check.
        #[arg(long, default_value = "validacion")]
        tramo: String,
        /// Criterio de réplica de la validación 2 sobre otro tramo (solo
        /// validacion: comprobación contra la robustez post hoc). En
        /// validacion2 es siempre el criterio.
        #[arg(long)]
        replica: bool,
        /// Prueba del código de la validación 3 sobre validacion2 (sin
        /// veredicto): % descartado por D2 y diferencia de medianas.
        #[arg(long)]
        d2_prueba: bool,
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
        Cmd::IndexTramo { from, count, until, rate } => {
            let mut st = store::Store::open(&cli.db)?;
            index_tramo(&rpc, &idl, &mut st, from, count, until, &rate.budget(&cli.rpc)?)
        }
        Cmd::Windows { limit, max_pages, rate } => {
            let mut st = store::Store::open(&cli.db)?;
            windows(&rpc, &idl, &mut st, limit, max_pages, &rate.budget(&cli.rpc)?)
        }
        Cmd::H1 { variant } => {
            let st = store::Store::open(&cli.db)?;
            h1_report(&st, cli.json, &variant)
        }
        Cmd::Prices { limit, max_pages, rate } => {
            let mut st = store::Store::open(&cli.db)?;
            prices(&rpc, &idl, &mut st, limit, max_pages, &rate.budget(&cli.rpc)?)
        }
        Cmd::H1b { variant } => {
            let st = store::Store::open(&cli.db)?;
            h1b_report(&st, cli.json, &variant)
        }
        Cmd::Entry => {
            let st = store::Store::open(&cli.db)?;
            entry_report(&st, cli.json)
        }
        Cmd::Entry2 { check, tramo, replica, d2_prueba } => {
            let st = store::Store::open(&cli.db)?;
            if tramo == "validacion3" {
                anyhow::ensure!(!replica && !d2_prueba, "validacion3 no admite --replica ni --d2-prueba");
                return entry2_v3_report(&st, cli.json, check);
            }
            if d2_prueba {
                anyhow::ensure!(tramo == "validacion2" && !check && !replica, "--d2-prueba solo con --tramo validacion2");
                return d2_prueba_report(&st, cli.json);
            }
            entry2_report(&st, cli.json, check, &tramo, replica)
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
            let t = rpc::with_retry(budget, rpc::Method::GetTransaction, || tx::fetch_events(rpc, idl, sig))
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
fn index_tramo(
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
            let page = rpc::with_retry(budget, rpc::Method::GetSignatures, || {
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
                let t = rpc::with_retry(budget, rpc::Method::GetTransaction, || tx::fetch_events(rpc, idl, sig))
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
        let page = rpc::with_retry(budget, rpc::Method::GetSignatures, || {
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
        match rpc::with_retry(budget, rpc::Method::GetTransaction, || tx::fetch_events(rpc, idl, sig)) {
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
    for (prov, gt, gs, units) in st.rpc_usage_totals()? {
        eprintln!("acumulado {prov}: {gt} getTransaction, {gs} getSignaturesForAddress, {units} unidades");
    }
    r
}

fn windows(rpc: &RpcClient, idl: &Idl, st: &mut store::Store, limit: usize, max_pages: usize, budget: &rpc::Budget) -> Result<()> {
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
            let Some((errors, n)) = fetch_token_sigs(rpc, idl, st, budget, "window", sigs, None)? else {
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
const PRICE_DOWNLOAD_SECS: i64 = h1b::PARAMS_1H.horizon_secs + store::EARLY_WINDOW_SECS;

fn prices(rpc: &RpcClient, idl: &Idl, st: &mut store::Store, limit: usize, max_pages: usize, budget: &rpc::Budget) -> Result<()> {
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
            let Some((errors, n)) = fetch_token_sigs(rpc, idl, st, budget, "prices", fetch, Some(horizon))? else {
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

/// Token de `CLAUDE.md` 8 que concentra la mitad de los pares 1+1 del piloto:
/// todo lo de H4–H7 se reporta con y sin él.
const CRH: &str = "CRHnzej9uJgwyiybSDqcndovAXGantb4v7e2hUihpump";

struct EntryRow {
    mint: String,
    mayhem: bool,
    h1b: h1b::H1bResult,
    pump_dump: bool,
    ind: Option<entry::Indicators>,
}

fn entry_report(st: &store::Store, json: bool) -> Result<()> {
    let (p, pb) = (entry::PARAMS, h1b::PARAMS_1H);
    let mut rows = Vec::new();
    for i in st.h1b_inputs(pb.horizon_secs, "v1")? {
        let r = h1b::compute(i.t0, i.initial, &i.points, i.completed_at, i.migrated_at, &pb);
        let trades = st.entry_trades(&i.mint, i.t0, p.window_secs)?;
        let ind = entry::compute(
            i.t0,
            st.creation_slot(&i.mint)?,
            (i.initial.quote_reserves, i.initial.token_reserves),
            &trades,
            &st.creator_identities(&i.mint)?,
            &p,
        );
        rows.push(EntryRow {
            mayhem: st.is_mayhem(&i.mint)?,
            pump_dump: r.peak_multiple >= pb.min_peak_multiple && r.drawdown > pb.max_drawdown,
            mint: i.mint,
            h1b: r,
            ind,
        });
    }
    if json {
        let v: Vec<_> = rows
            .iter()
            .map(|r| {
                serde_json::json!({"mint": r.mint, "mayhem": r.mayhem, "pump_dump": r.pump_dump,
                                   "h1b": r.h1b, "entry": r.ind})
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }

    println!("T_entry y H4–H7, parámetros [P]: {}", serde_json::to_string(&p)?);
    let entered: Vec<&EntryRow> = rows.iter().filter(|r| r.ind.is_some()).collect();
    let no_entry: Vec<&EntryRow> = rows.iter().filter(|r| r.ind.is_none()).collect();
    println!(
        "tokens: {}  con entrada: {} ({} pump-y-caída)  sin entrada: {} ({} pump-y-caída, {} con pico ≥ 2×)",
        rows.len(),
        entered.len(),
        entered.iter().filter(|r| r.pump_dump).count(),
        no_entry.len(),
        no_entry.iter().filter(|r| r.pump_dump).count(),
        no_entry.iter().filter(|r| r.h1b.peak_multiple >= pb.min_peak_multiple).count(),
    );

    println!(
        "\n{:12} {:>3} {:>4} {:>6} {:>5} {:>6} {:>8} {:>4} {:>5} {:>7} {:>5}",
        "mint", "p&c", "grad", "pico×", "t_ent", "trades", "dev_vta", "dev%", "slotC", "slot_w", "1+1"
    );
    for r in &entered {
        let i = r.ind.as_ref().unwrap();
        println!(
            "{:12} {:>3} {:>4} {:>6.2} {:>5} {:>6} {:>8} {:>4.1} {:>5} {:>3} {:>3.1} {:>5}{}",
            &r.mint[..12],
            if r.pump_dump { "sí" } else { "no" },
            if r.h1b.graduated { "sí" } else { "no" },
            r.h1b.peak_multiple,
            i.entry_secs,
            i.trades_seen,
            i.dev_first_sell_secs.map_or("-".into(), |s| format!("{s} s")),
            i.dev_sold_pct,
            if i.creator_same_slot_buy { "sí" } else { "no" },
            i.same_slot_wallets,
            i.same_slot_pct,
            i.pairs,
            if r.mayhem { "  (mayhem)" } else { "" },
        );
    }

    type Sig = fn(&entry::Indicators) -> bool;
    let sigs: [(&str, Sig); 5] = [
        ("H4 dev vendió antes de entrar", |i| i.h4()),
        ("H5 ≥1 wallet ajena compra en el slot de creación", |i| i.h5()),
        ("H5a (descriptivo) el creador compra en el slot de creación", |i| i.creator_same_slot_buy),
        ("H6 entrada en ≤ 10 s", |i| i.h6(&entry::PARAMS)),
        ("H7 ≥ 3 wallets 1+1 antes de entrar", |i| i.h7(&entry::PARAMS)),
    ];
    let table = |sub: &[&&EntryRow], sig: Sig, ok: &dyn Fn(&EntryRow) -> Option<bool>| {
        let mut t = h1b::Table::default();
        for r in sub {
            match (sig(r.ind.as_ref().unwrap()), ok(r)) {
                (true, Some(true)) => t.a += 1,
                (true, Some(false)) => t.b += 1,
                (false, Some(true)) => t.c += 1,
                (false, Some(false)) => t.d += 1,
                (_, None) => {}
            }
        }
        t
    };
    let rate = |x: u64, y: u64| if x + y == 0 { f64::NAN } else { x as f64 / (x + y) as f64 };
    let all: Vec<&&EntryRow> = entered.iter().collect();
    let sin_crh: Vec<&&EntryRow> = entered.iter().filter(|r| r.mint != CRH).collect();
    for (name, sig) in sigs {
        println!("\n{name}");
        let mut diffs = vec![];
        for (label, sub) in [("todos", &all), ("sin CRHnzej9", &sin_crh)] {
            let t = table(sub, sig, &|r| Some(r.pump_dump));
            let (p1, p0) = (rate(t.a, t.b), rate(t.c, t.d));
            let (or, lo, hi) = t.odds_ratio();
            println!(
                "  {label:13} señal: {}/{} p&c ({:.0}%)  sin señal: {}/{} ({:.0}%)  dif {:+.0} pts  Fisher p = {:.3}  OR {or:.2} ({lo:.2}–{hi:.2})",
                t.a,
                t.a + t.b,
                100.0 * p1,
                t.c,
                t.c + t.d,
                100.0 * p0,
                100.0 * (p1 - p0),
                t.fisher_p()
            );
            diffs.push((t.a + t.b, t.c + t.d, p1 - p0));
        }
        // Tolerancia de coma flotante: 0.7 − 0.5 = 0.1999… debe contar como 20 pts.
        const EPS: f64 = 1e-9;
        let [(n1, n0, d), (_, _, d_sin)] = [diffs[0], diffs[1]];
        let verdict = if n1 < 3 || n0 < 3 {
            "no concluyente (algún grupo < 3 tokens)"
        } else if d >= 0.20 - EPS && d_sin > EPS {
            "candidata a validar con tokens nuevos"
        } else if d <= EPS {
            "descartada en calibración"
        } else {
            "no concluyente"
        };
        println!("  criterio pre-registrado: {verdict}");
        for (label, ok) in [
            ("graduó 1 h", &(|r: &EntryRow| Some(r.h1b.graduated)) as &dyn Fn(&EntryRow) -> Option<bool>),
            ("sostenido 1 h", &|r: &EntryRow| r.h1b.sustained),
        ] {
            let t = table(&all, sig, ok);
            println!(
                "  secundario × {label}: señal {}/{}, sin señal {}/{}, Fisher p = {:.3}",
                t.a,
                t.a + t.b,
                t.c,
                t.c + t.d,
                t.fisher_p()
            );
        }
    }

    // Distribuciones completas por grupo de resultado (con y sin CRHnzej9 solo
    // cambia un valor: se marca con *).
    type Metric = fn(&entry::Indicators) -> Option<f64>;
    let metrics: [(&str, Metric); 6] = [
        ("H4 % supply vendido por el dev", |i| Some(i.dev_sold_pct)),
        ("H4 s hasta la primera venta del dev", |i| i.dev_first_sell_secs.map(|s| s as f64)),
        ("H5 wallets ajenas en el slot de creación", |i| Some(i.same_slot_wallets as f64)),
        ("H5 % supply comprado por ellas", |i| Some(i.same_slot_pct)),
        ("H6 s hasta T_entry", |i| Some(i.entry_secs as f64)),
        ("H7 wallets 1+1 antes de entrar", |i| Some(i.pairs as f64)),
    ];
    println!("\ndistribuciones (tokens con entrada; * = CRHnzej9):");
    for (name, m) in metrics {
        println!("  {name}");
        for (label, pd) in [("p&c  ", true), ("resto", false)] {
            let mut v: Vec<(f64, bool)> = entered
                .iter()
                .filter(|r| r.pump_dump == pd)
                .filter_map(|r| m(r.ind.as_ref().unwrap()).map(|x| (x, r.mint == CRH)))
                .collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            let s: Vec<String> =
                v.iter().map(|(x, c)| format!("{}{}", if x.fract() == 0.0 { format!("{x:.0}") } else { format!("{x:.2}") }, if *c { "*" } else { "" })).collect();
            let med = if v.is_empty() { f64::NAN } else { v[(v.len() - 1) / 2].0 };
            println!("    {label} n={:2} mediana {med:.2}: [{}]", v.len(), s.join(", "));
        }
    }
    Ok(())
}

type Signal = fn(&entry2::Entry2) -> Option<bool>;

/// Las cinco hipótesis congeladas, con la dirección esperada: señal = true
/// ⇒ más pump-y-caída en todas.
const ENTRY2_SIGNALS: [(&str, Signal); 5] = [
    ("H4' dev vendió antes de T_entry2", |e| e.h4()),
    ("H5 ≥1 wallet ajena compra en el slot de creación", |e| e.h5()),
    ("H6' llenado en T_entry2 ≥ umbral", |e| e.h6(&entry2::PARAMS)),
    ("H7' wallets 1+1 antes de T_entry2 ≥ umbral", |e| e.h7(&entry2::PARAMS)),
    ("H8 el creador compra ≥ 15 % del supply en el slot de creación", |e| e.h8(&entry2::PARAMS)),
];

/// Mediana convencional (media de los dos centrales si n es par).
fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

fn fmt_list(v: &[f64]) -> String {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    v.iter()
        .map(|x| if x.fract() == 0.0 { format!("{x:.0}") } else { format!("{x:.2}") })
        .collect::<Vec<_>>()
        .join(", ")
}

fn entry2_report(st: &store::Store, json: bool, check: bool, tramo: &str, replica: bool) -> Result<()> {
    let p = entry2::PARAMS;
    let (lo, hi) = match tramo {
        "piloto" => (0, entry2::PILOT_LAST_SLOT),
        "validacion" => (entry2::PILOT_LAST_SLOT + 1, entry2::VALIDATION1_LAST_SLOT),
        "validacion2" => (entry2::VALIDATION2_FIRST_SLOT, entry2::VALIDATION2_LAST_SLOT),
        _ => anyhow::bail!("tramo desconocido: {tramo} (validacion, validacion2, piloto)"),
    };
    if tramo == "piloto" && !check {
        anyhow::bail!("el piloto es tramo de calibración: en la validación congelada solo admite --check");
    }
    if replica && (check || tramo == "piloto") {
        anyhow::bail!("--replica no admite --check ni el piloto");
    }
    let inputs = st.entry2_inputs(lo, hi)?;
    let no_initial = inputs.iter().filter(|i| i.initial.is_none()).count();
    let rows: Vec<(&store::Entry2Input, Option<entry2::Entry2>)> = inputs
        .iter()
        .filter_map(|i| {
            let init = i.initial?;
            let e = entry2::compute(
                i.t0,
                i.create_slot,
                (init.quote_reserves, init.token_reserves),
                &i.trades,
                &i.creator_ids,
                &p,
            );
            Some((i, e))
        })
        .collect();
    let entered: Vec<(&store::Entry2Input, &entry2::Entry2)> =
        rows.iter().filter_map(|(i, e)| e.as_ref().map(|e| (*i, e))).collect();

    if check {
        if json {
            let v: Vec<_> = rows.iter().map(|(i, e)| serde_json::json!({"mint": i.mint, "entry2": e})).collect();
            println!("{}", serde_json::to_string_pretty(&v)?);
            return Ok(());
        }
        println!("CHEQUEO DE NO-DEGENERACIÓN, tramo {tramo} (sin resultado). Parámetros: {}", serde_json::to_string(&p)?);
        println!(
            "tokens con ventana completa: {}  sin precio inicial: {no_initial}  con T_entry2: {}  sin T_entry2: {}",
            inputs.len(),
            entered.len(),
            rows.len() - entered.len()
        );
        println!(
            "  orden de ejecución completo: {}/{}   comisión conocida en T_entry2: {}/{}",
            entered.iter().filter(|(_, e)| e.order_complete).count(),
            entered.len(),
            entered.iter().filter(|(_, e)| e.fee_bps.is_some()).count(),
            entered.len()
        );
        for (name, sig) in ENTRY2_SIGNALS {
            let c = |x: Option<bool>| entered.iter().filter(|(_, e)| sig(e) == x).count();
            println!("  {name}: true {}  false {}  n/e {}", c(Some(true)), c(Some(false)), c(None));
        }
        if tramo == "validacion2" {
            // Poblaciones del pre-registro (sin resultado): primaria = un token
            // por creator (menor slot, luego mint); secundaria 1 = todos con
            // T_entry2; secundaria 2 = sin mayhem.
            let mut tagged = Vec::new();
            for (i, e) in &entered {
                tagged.push((st.creation_creator(&i.mint)?, i.create_slot, i.mint.as_str(), st.is_mayhem(&i.mint)?, *e));
            }
            tagged.sort_by(|a, b| (a.1, a.2).cmp(&(b.1, b.2)));
            let mut seen = std::collections::HashSet::new();
            let primary: Vec<_> = tagged.iter().filter(|t| seen.insert(t.0.clone())).collect();
            let all: Vec<_> = tagged.iter().collect();
            let no_mayhem: Vec<_> = tagged.iter().filter(|t| !t.3).collect();
            let n = entered.len();
            println!(
                "  con T_entry2: {n} (umbral de baja potencia {}): {}",
                entry2::VALIDATION2_MIN_ENTERED,
                if n < entry2::VALIDATION2_MIN_ENTERED { "BAJA POTENCIA" } else { "no aplica" }
            );
            let mut small = Vec::new();
            for (label, pop) in [("primaria (1 por creator)", &primary), ("secundaria 1 (todos)", &all), ("secundaria 2 (sin mayhem)", &no_mayhem)] {
                println!("  población {label}: {} tokens, {} creators", pop.len(), pop.iter().map(|t| &t.0).collect::<std::collections::HashSet<_>>().len());
                for (name, sig) in ENTRY2_SIGNALS {
                    let c = |x: Option<bool>| pop.iter().filter(|t| sig(t.4) == x).count();
                    let (t, f) = (c(Some(true)), c(Some(false)));
                    if t < 3 || f < 3 {
                        small.push(format!("{label} / {name}"));
                    }
                    println!("    {name}: true {t}  false {f}  n/e {}", c(None));
                }
            }
            println!(
                "  grupos < 3 (parada; H10 usa los mismos grupos menos los n/e de supervivencia, que se ven al calcular): {}",
                if small.is_empty() { "ninguno".to_string() } else { small.join("; ") }
            );
        }
        let col = |f: &dyn Fn(&entry2::Entry2) -> Option<f64>| -> Vec<f64> {
            entered.iter().filter_map(|(_, e)| f(e)).collect()
        };
        for (name, v) in [
            ("s hasta T_entry2", col(&|e| Some(e.entry_secs as f64))),
            ("trades antes de T_entry2", col(&|e| Some(e.trades_before as f64))),
            ("múltiplo de precio en T_entry2", col(&|e| Some(e.entry_multiple))),
            ("H6' llenado % en T_entry2", col(&|e| e.fill_pct)),
            ("H7' wallets 1+1 antes de T_entry2", col(&|e| Some(e.pairs as f64))),
            ("H4' % supply vendido por el dev", col(&|e| Some(e.dev_sold_pct))),
            ("H8 % supply comprado por el creador en el slot", col(&|e| Some(e.creator_slot_buy_pct))),
        ] {
            let mut m = v.clone();
            println!("  {name} (n={}, mediana {:.3}): [{}]", v.len(), median(&mut m), fmt_list(&v));
        }
        return Ok(());
    }
    if replica || tramo == "validacion2" {
        return entry2_replica_report(st, json, tramo, &entered);
    }

    // Validación: resultado primario pump-y-caída en 1 h (definición vigente).
    let pb = h1b::PARAMS_1H;
    struct Row<'a> {
        mint: &'a str,
        e: &'a entry2::Entry2,
        pump_dump: bool,
        rets: [Option<f64>; 3],
        mayhem: bool,
    }
    let mut table_rows = Vec::new();
    let mut no_price = 0;
    for (i, e) in &entered {
        let Some(h) = i.price_horizon.filter(|&h| h >= pb.horizon_secs) else {
            no_price += 1;
            continue;
        };
        let init = i.initial.unwrap();
        let r = h1b::compute(i.t0, init, &i.points, i.completed_at, i.migrated_at, &pb);
        let pts: Vec<(i64, f64)> =
            i.points.iter().map(|x| (x.timestamp, x.quote_reserves as f64 / x.token_reserves as f64)).collect();
        table_rows.push(Row {
            mint: &i.mint,
            e,
            pump_dump: r.peak_multiple >= pb.min_peak_multiple && r.drawdown > pb.max_drawdown,
            rets: entry2::returns(e, &pts, Some(i.t0 + h - 1), i.completed_at, &p),
            mayhem: st.is_mayhem(&i.mint)?,
        });
    }
    let heaviest = table_rows.iter().max_by_key(|r| r.e.window_trades).map(|r| r.mint.to_string());
    if json {
        let v: Vec<_> = table_rows
            .iter()
            .map(|r| serde_json::json!({"mint": r.mint, "entry2": r.e, "pump_dump": r.pump_dump, "returns": r.rets}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!("VALIDACIÓN CONGELADA, tramo {tramo}. Parámetros: {}", serde_json::to_string(&p)?);
    println!(
        "tokens con ventana completa: {}  sin precio inicial: {no_initial}  sin T_entry2: {}  con T_entry2 sin ventana de precio: {no_price}  en tablas: {} ({} pump-y-caída)",
        inputs.len(),
        rows.len() - entered.len(),
        table_rows.len(),
        table_rows.iter().filter(|r| r.pump_dump).count()
    );
    println!("token de mayor peso (más trades en la ventana): {}", heaviest.as_deref().unwrap_or("-"));
    let rate = |x: u64, y: u64| if x + y == 0 { f64::NAN } else { x as f64 / (x + y) as f64 };
    const EPS: f64 = 1e-9;
    for (name, sig) in ENTRY2_SIGNALS {
        let table = |skip: Option<&str>| {
            let mut t = h1b::Table::default();
            for r in table_rows.iter().filter(|r| Some(r.mint) != skip) {
                match (sig(r.e), r.pump_dump) {
                    (Some(true), true) => t.a += 1,
                    (Some(true), false) => t.b += 1,
                    (Some(false), true) => t.c += 1,
                    (Some(false), false) => t.d += 1,
                    (None, _) => {}
                }
            }
            t
        };
        let (t, ts) = (table(None), table(heaviest.as_deref()));
        let d = rate(t.a, t.b) - rate(t.c, t.d);
        let ds = rate(ts.a, ts.b) - rate(ts.c, ts.d);
        let fp = t.fisher_p();
        let (or, olo, ohi) = t.odds_ratio();
        println!("\n{name}");
        println!("  n señal = {}  n sin señal = {}  n/e = {}", t.a + t.b, t.c + t.d, table_rows.len() as u64 - (t.a + t.b + t.c + t.d));
        println!(
            "  señal: {}/{} p&c ({:.0}%)  sin señal: {}/{} ({:.0}%)  dif {:+.0} pts  Fisher p = {fp:.4}  OR {or:.2} ({olo:.2}–{ohi:.2})",
            t.a,
            t.a + t.b,
            100.0 * rate(t.a, t.b),
            t.c,
            t.c + t.d,
            100.0 * rate(t.c, t.d),
            100.0 * d
        );
        println!("  sin el token de mayor peso: dif {:+.0} pts", 100.0 * ds);
        let verdict = if t.a + t.b < 3 || t.c + t.d < 3 {
            "NO EVALUABLE (algún grupo < 3 tokens)"
        } else if d >= 0.20 - EPS && fp < 0.01 && ds > EPS {
            "CONFIRMADA"
        } else if d < -EPS {
            "NO CONFIRMADA (dirección opuesta)"
        } else {
            "NO CONFIRMADA"
        };
        println!("  criterio congelado: {verdict}");
        for (k, h) in p.return_horizons.iter().enumerate() {
            let grp = |g: bool| -> Vec<f64> {
                table_rows.iter().filter(|r| sig(r.e) == Some(g)).filter_map(|r| r.rets[k]).collect()
            };
            let (mut x, mut y) = (grp(true), grp(false));
            let mw = entry2::mann_whitney(&x, &y).map_or(f64::NAN, |m| m.1);
            let (nx, ny) = (x.len(), y.len());
            println!(
                "  retorno neto +{} min: mediana señal {:+.1}% (n={nx}) vs sin señal {:+.1}% (n={ny}), Mann-Whitney p = {mw:.3}",
                h / 60,
                100.0 * median(&mut x),
                100.0 * median(&mut y)
            );
        }
    }

    // A partir de aquí nada decide: descriptivo añadido tras congelar.
    println!("\n==== POST HOC, NO PRE-REGISTRADA, SIN VEREDICTO ====");
    type Filter<'b> = (&'b str, &'b dyn Fn(&Row) -> bool);
    println!("\n(a) cada hipótesis con y sin tokens mayhem (pump-y-caída por grupo)");
    let strata_a: [Filter; 3] =
        [("todos", &|_| true), ("sin mayhem", &|r| !r.mayhem), ("solo mayhem", &|r| r.mayhem)];
    for (name, sig) in ENTRY2_SIGNALS {
        println!("{name}");
        for (label, keep) in strata_a {
            let mut t = h1b::Table::default();
            for r in table_rows.iter().filter(|r| keep(r)) {
                match (sig(r.e), r.pump_dump) {
                    (Some(true), true) => t.a += 1,
                    (Some(true), false) => t.b += 1,
                    (Some(false), true) => t.c += 1,
                    (Some(false), false) => t.d += 1,
                    (None, _) => {}
                }
            }
            println!(
                "  {label:12} señal {}/{} ({:.0}%)  sin señal {}/{} ({:.0}%)  dif {:+.0} pts  Fisher p = {:.4}",
                t.a,
                t.a + t.b,
                100.0 * rate(t.a, t.b),
                t.c,
                t.c + t.d,
                100.0 * rate(t.c, t.d),
                100.0 * (rate(t.a, t.b) - rate(t.c, t.d)),
                t.fisher_p()
            );
        }
    }
    println!("\n(b) retorno neto estratificado por mayhem y por comisión en T_entry2");
    let strata_b: [Filter; 5] = [
        ("mayhem no", &|r| !r.mayhem),
        ("mayhem sí", &|r| r.mayhem),
        ("fee 125 bps", &|r| r.e.fee_bps == Some(125)),
        ("fee 0 bps", &|r| r.e.fee_bps == Some(0)),
        ("fee otra", &|r| !matches!(r.e.fee_bps, Some(125) | Some(0))),
    ];
    for (label, keep) in strata_b {
        let sub: Vec<&Row> = table_rows.iter().filter(|r| keep(r)).collect();
        println!("[{label}] tokens = {}", sub.len());
        for (k, h) in p.return_horizons.iter().enumerate() {
            let mut all: Vec<f64> = sub.iter().filter_map(|r| r.rets[k]).collect();
            let n = all.len();
            println!("  +{} min, todos: mediana {:+.1}% (n={n})", h / 60, 100.0 * median(&mut all));
        }
        for (name, sig) in ENTRY2_SIGNALS {
            let short = name.split(' ').next().unwrap_or(name);
            let cells: Vec<String> = p
                .return_horizons
                .iter()
                .enumerate()
                .map(|(k, h)| {
                    let grp = |g: bool| -> Vec<f64> {
                        sub.iter().filter(|r| sig(r.e) == Some(g)).filter_map(|r| r.rets[k]).collect()
                    };
                    let (mut x, mut y) = (grp(true), grp(false));
                    let mw = entry2::mann_whitney(&x, &y).map_or(f64::NAN, |m| m.1);
                    let (nx, ny) = (x.len(), y.len());
                    format!(
                        "+{}m {:+.0}% (n={nx}) vs {:+.0}% (n={ny}) MW p={mw:.3}",
                        h / 60,
                        100.0 * median(&mut x),
                        100.0 * median(&mut y)
                    )
                })
                .collect();
            println!("  {short:4} {}", cells.join(" | "));
        }
    }
    Ok(())
}

/// Horizontes del descriptivo de la validación 2 (s desde T_entry2).
const REPLICA_HORIZONS: [(i64, &str); 5] = [(5, "+5 s"), (30, "+30 s"), (300, "+5 min"), (1800, "+30 min"), (3600, "+60 min")];

fn pct1(x: Option<f64>) -> String {
    match x {
        Some(v) if !v.is_nan() => format!("{:+.1}", 100.0 * v),
        _ => "n/a".into(),
    }
}

fn ci_str(c: (f64, f64, usize)) -> String {
    format!("[{}, {}; descartadas {}]", pct1(Some(c.0)), pct1(Some(c.1)), c.2)
}

/// Validación 2 (réplica, CLAUDE.md 8): poblaciones primaria (un token por
/// creator), secundaria 1 (todos) y secundaria 2 (sin mayhem); veredicto de
/// réplica de H4'–H8 (pump-y-caída) y H10 (supervivencia) y descriptivo.
fn entry2_replica_report(
    st: &store::Store,
    json: bool,
    tramo: &str,
    entered: &[(&store::Entry2Input, &entry2::Entry2)],
) -> Result<()> {
    use entry2::ExitB;
    use replica::Tok;
    use std::collections::{HashMap, HashSet};
    let (p, pb) = (entry2::PARAMS, h1b::PARAMS_1H);
    let low_power = entered.len() < entry2::VALIDATION2_MIN_ENTERED;

    // Primaria: un token por creator, menor slot; el orden de ejecución entre
    // creaciones no se guarda, así que el empate se resuelve por mint.
    let mut by_slot: Vec<(u64, &str, String)> = Vec::new();
    for (i, _) in entered {
        by_slot.push((i.create_slot, i.mint.as_str(), st.creation_creator(&i.mint)?));
    }
    by_slot.sort();
    let mut first: HashMap<&str, (u64, &str)> = HashMap::new();
    let mut slot_ties = 0;
    for (slot, mint, c) in &by_slot {
        match first.get(c.as_str()) {
            None => {
                first.insert(c, (*slot, mint));
            }
            Some((s, _)) if s == slot => slot_ties += 1,
            _ => {}
        }
    }
    let primary_mints: HashSet<&str> = first.values().map(|x| x.1).collect();

    struct Full {
        tok: Tok,
        ret_a: [Option<f64>; 5],
        ret_b: [ExitB; 5],
        exit_slot: [Option<u64>; 5],
    }
    let mut full = Vec::new();
    let mut no_price = 0;
    for (i, e) in entered {
        let Some(h) = i.price_horizon.filter(|&h| h >= pb.horizon_secs) else {
            no_price += 1;
            continue;
        };
        let init = i.initial.unwrap();
        let r = h1b::compute(i.t0, init, &i.points, i.completed_at, i.migrated_at, &pb);
        let price = |x: &h1b::Point| x.quote_reserves as f64 / x.token_reserves as f64;
        let flat: Vec<(i64, f64)> = i.points.iter().map(|x| (x.timestamp, price(x))).collect();
        let pts3: Vec<(i64, u64, f64)> =
            i.points.iter().zip(&i.point_slots).map(|(x, &s)| (x.timestamp, s, price(x))).collect();
        let window: Vec<&entry::Trade> =
            i.trades.iter().filter(|t| (i.t0..i.t0 + p.window_secs).contains(&t.timestamp)).collect();
        let chain = entry2::chained(init.token_reserves, &window);
        let covered = Some(i.t0 + h - 1);
        let ret_a = REPLICA_HORIZONS.map(|(h, _)| entry2::return_at(e, &flat, covered, i.completed_at, h));
        let ret_b = REPLICA_HORIZONS.map(|(h, _)| entry2::return_b(e, &pts3, &chain, covered, i.completed_at, h));
        let exit_slot = REPLICA_HORIZONS
            .map(|(h, _)| pts3.iter().filter(|x| x.0 <= e.entry_ts + h).max_by_key(|x| x.0).map(|x| x.1));
        let ts: Vec<i64> = i.points.iter().map(|x| x.timestamp).collect();
        full.push(Full {
            tok: Tok {
                mint: i.mint.clone(),
                creator: st.creation_creator(&i.mint)?,
                slot: i.create_slot,
                mayhem: st.is_mayhem(&i.mint)?,
                window_trades: e.window_trades,
                sigs: ENTRY2_SIGNALS.map(|(_, s)| s(e)),
                pump_dump: r.peak_multiple >= pb.min_peak_multiple && r.drawdown > pb.max_drawdown,
                surv: entry2::survives(e, &ts, i.completed_at),
                ret30: [ret_a[3], ret_b[3].value()],
            },
            ret_a,
            ret_b,
            exit_slot,
        });
    }
    let mut primary: Vec<&Tok> = full.iter().map(|f| &f.tok).filter(|t| primary_mints.contains(t.mint.as_str())).collect();
    primary.sort_by(|a, b| (a.slot, &a.mint).cmp(&(b.slot, &b.mint)));
    let all: Vec<&Tok> = full.iter().map(|f| &f.tok).collect();
    let no_mayhem: Vec<&Tok> = all.iter().copied().filter(|t| !t.mayhem).collect();
    let pops: [(&str, &[&Tok], bool); 3] = [
        ("primaria (1 por creator)", &primary, false),
        ("secundaria 1 (todos)", &all, true),
        ("secundaria 2 (sin mayhem)", &no_mayhem, true),
    ];
    let creators = |pop: &[&Tok]| pop.iter().map(|t| t.creator.as_str()).collect::<HashSet<_>>().len();
    let names: Vec<&str> = ENTRY2_SIGNALS.iter().map(|(n, _)| n.split(' ').next().unwrap()).collect();

    let mut out = String::new();
    macro_rules! say { ($($a:tt)*) => { out.push_str(&format!($($a)*)); out.push('\n'); } }
    say!("VALIDACIÓN 2 — CRITERIO DE RÉPLICA, tramo {tramo}. Parámetros: {}", serde_json::to_string(&p)?);
    say!(
        "con T_entry2: {} (umbral de baja potencia {}: {})  con T_entry2 sin ventana de precio: {no_price}  empates de slot en la primaria (resueltos por mint): {slot_ties}",
        entered.len(),
        entry2::VALIDATION2_MIN_ENTERED,
        if low_power { "BAJA POTENCIA" } else { "no aplica" }
    );
    for (label, pop, _) in &pops {
        say!("población {label}: {} tokens, {} creators", pop.len(), creators(pop));
    }

    // Condición de parada: algún grupo < 3 (H4'–H8; H10 sin los n/e de supervivencia).
    let mut small = Vec::new();
    for (label, pop, _) in &pops {
        for (k, name) in names.iter().enumerate() {
            let c = |g: bool, surv_only: bool| {
                pop.iter().filter(|t| t.sigs[k] == Some(g) && (!surv_only || t.surv.is_some())).count() as u64
            };
            if c(true, false) < replica::MIN_GROUP || c(false, false) < replica::MIN_GROUP {
                small.push(format!("{label} / {name}"));
            }
            if k >= 1 && (c(true, true) < replica::MIN_GROUP || c(false, true) < replica::MIN_GROUP) {
                small.push(format!("{label} / H10 {name}"));
            }
        }
    }
    if !small.is_empty() {
        say!("\nCONDICIÓN DE PARADA: grupos con < {} tokens: {}", replica::MIN_GROUP, small.join("; "));
        say!("No se calcula ningún veredicto.");
        print!("{out}");
        return Ok(());
    }
    say!("condición de parada (grupo < 3 en H4'–H8 y H10, tres poblaciones): no se cumple");
    say!(
        "supervivencia n/e: primaria {}, secundaria 1 {}, secundaria 2 {}",
        primary.iter().filter(|t| t.surv.is_none()).count(),
        all.iter().filter(|t| t.surv.is_none()).count(),
        no_mayhem.iter().filter(|t| t.surv.is_none()).count()
    );

    let heaviest = primary.iter().max_by_key(|t| t.window_trades).map(|t| (t.mint.clone(), t.window_trades));
    let (hmint, htrades) = heaviest.clone().unwrap_or_default();
    let heavy_ties = primary.iter().filter(|t| t.window_trades == htrades).count();
    say!("token de más trades en la primaria: {hmint} ({htrades} trades en 5 min; tokens con ese máximo: {heavy_ties})");

    let mut hyps_json = Vec::new();
    let outcomes: [(&str, replica::Outcome, usize); 2] =
        [("pump-y-caída en 1 h", replica::pump_dump, 0), ("H10 supervivencia (> 10 min de T_entry2)", replica::survival, 1)];
    for (oname, outcome, first_sig) in outcomes {
        say!("\n==== Resultado: {oname} ====");
        for k in first_sig..5 {
            let name = if first_sig == 0 { ENTRY2_SIGNALS[k].0.to_string() } else { format!("H10 × {}", names[k]) };
            let res: Vec<replica::PopResult> =
                pops.iter().map(|(_, pop, cl)| replica::pop_result(pop, k, outcome, *cl)).collect();
            let wo: Vec<&Tok> = primary.iter().copied().filter(|t| t.mint != hmint).collect();
            let primary_wo = replica::diff(&replica::table(&wo, k, outcome));
            let v = replica::verdict(&res[0], primary_wo, [&res[1], &res[2]], low_power);
            say!("\n{name}");
            for ((label, _, cl), r) in pops.iter().zip(&res) {
                let t = r.table;
                say!(
                    "  {label}: n {}/{}  señal {}/{} ({}%)  sin señal {}/{} ({}%)  dif {} pts  boot {} {}  Fisher p = {:.4}  OR {:.2} ({:.2}–{:.2})",
                    r.n1,
                    r.n0,
                    t.a,
                    r.n1,
                    pct1(Some(t.a as f64 / r.n1 as f64)),
                    t.c,
                    r.n0,
                    pct1(Some(t.c as f64 / r.n0 as f64)),
                    pct1(r.diff),
                    if *cl { "clúster" } else { "simple" },
                    ci_str(r.boot),
                    r.fisher_p,
                    r.or.0,
                    r.or.1,
                    r.or.2
                );
            }
            say!("  primaria sin el token de más trades: dif {} pts", pct1(primary_wo));
            say!("  VEREDICTO: {v}");
            hyps_json.push(serde_json::json!({
                "name": name, "outcome": oname, "verdict": v, "primary_wo_heaviest": primary_wo,
                "primaria": res[0], "secundaria1": res[1], "secundaria2": res[2],
            }));
        }
    }

    say!("\n==== DESCRIPTIVO, SIN VEREDICTO ====");
    // Retorno (a) y (b), secundaria 2 (todos con T_entry2 sin mayhem).
    let nm: Vec<&Full> = full.iter().filter(|f| !f.tok.mayhem).collect();
    say!("\nRetorno neto desde T_entry2, sin mayhem (n = {}). Celda: mediana · fracción > 0 (n)", nm.len());
    let mut single_mismatch = 0;
    let mut single_total = 0;
    for f in &full {
        for k in 0..5 {
            if let ExitB::Single(v) = f.ret_b[k] {
                single_total += 1;
                if f.ret_a[k] != Some(v) {
                    single_mismatch += 1;
                }
            }
        }
    }
    say!(
        "control: (a) = (b) en todo token-horizonte con slot de salida de un solo trade: {} de {} (discrepancias: {single_mismatch})",
        single_total - single_mismatch,
        single_total
    );
    let mut rets_json = Vec::new();
    for (k, (_, hl)) in REPLICA_HORIZONS.iter().enumerate() {
        let (mut single, mut rebuilt, mut unresolved, mut ne) = (0, 0, 0, 0);
        let mut slots = HashSet::new();
        for f in &nm {
            match f.ret_b[k] {
                ExitB::Single(_) => single += 1,
                ExitB::Rebuilt(_) => rebuilt += 1,
                ExitB::Unresolved => {
                    unresolved += 1;
                    slots.insert(f.exit_slot[k]);
                }
                ExitB::NotEvaluable => ne += 1,
            }
        }
        say!(
            "\n{hl}: (b) slot de salida con un trade {single}, varios trades reconstruidos {rebuilt}, NO reconstruidos {unresolved} tokens ({} slots), n/e como (a) {ne}",
            slots.len()
        );
        let cell = |v: &mut Vec<f64>| {
            let n = v.len();
            let pos = v.iter().filter(|&&x| x > 0.0).count();
            let m = replica::median(v);
            (format!("{} · {} ({n})", pct1(m), if n == 0 { "n/a".into() } else { format!("{:.0}%", 100.0 * pos as f64 / n as f64) }), m, pos, n)
        };
        let mut groups: Vec<(String, Vec<&Full>)> = vec![("todos".into(), nm.clone())];
        for (s, name) in names.iter().enumerate() {
            for g in [true, false] {
                groups.push((
                    format!("{name} {}", if g { "sí" } else { "no" }),
                    nm.iter().copied().filter(|f| f.tok.sigs[s] == Some(g)).collect(),
                ));
            }
        }
        for (label, grp) in groups {
            let mut a: Vec<f64> = grp.iter().filter_map(|f| f.ret_a[k]).collect();
            let mut b: Vec<f64> = grp.iter().filter_map(|f| f.ret_b[k].value()).collect();
            let (sa, ma, pa, na) = cell(&mut a);
            let (sb, mb, pbp, nb) = cell(&mut b);
            say!("  {label:9} (a) {sa:28} (b) {sb}");
            rets_json.push(serde_json::json!({"horizon": hl, "group": label,
                "a": {"median": ma, "pos": pa, "n": na}, "b": {"median": mb, "pos": pbp, "n": nb}}));
        }
    }

    say!("\nFiltros \"evitar\" (A = cualquiera de H5/H6'/H7'/H8; B = ≥ 2; C = H5 o H7')");
    let mut filt_json = Vec::new();
    for (label, pop, cl) in &pops {
        say!("\n[{label}] bootstrap {}", if *cl { "por clúster de creator" } else { "simple" });
        for f in ['A', 'B', 'C'] {
            say!("  filtro {f}");
            let bads: [(&str, &dyn Fn(&Tok) -> Option<bool>); 3] = [
                ("malo principal = pump-y-caída", &|t| Some(t.pump_dump)),
                ("malo secundario (a) = ret +30 < 0, sin mayhem", &|t| if t.mayhem { None } else { t.ret30[0].map(|r| r < 0.0) }),
                ("malo secundario (b) = ret +30 < 0, sin mayhem", &|t| if t.mayhem { None } else { t.ret30[1].map(|r| r < 0.0) }),
            ];
            let mut fj = serde_json::json!({"population": label, "filter": f.to_string()});
            for (bl, bad) in bads {
                let m = replica::filter_metrics(pop, f, bad);
                say!(
                    "    {bl:46} evaluables {}  evitados {}  no evitados {}  precisión {}%  cobertura {}%  sacrificio {}%",
                    m.evaluable,
                    m.avoided,
                    m.kept,
                    pct1(Some(m.precision)),
                    pct1(Some(m.coverage)),
                    pct1(Some(m.sacrifice))
                );
                fj[bl] = serde_json::to_value(&m)?;
            }
            for v in 0..2 {
                let r = replica::rest_return(pop, f, v);
                let c = replica::ci(pop, *cl, &|rs| replica::rest_return(rs, f, v));
                let n = pop.iter().filter(|t| !t.mayhem && !replica::filter(t, f) && t.ret30[v].is_some()).count();
                say!(
                    "    retorno +30 de los no evitados, sin mayhem, ({}) n = {n}: {}% {}",
                    ["a", "b"][v],
                    pct1(r),
                    ci_str(c)
                );
                fj[format!("rest_{}", ["a", "b"][v])] = serde_json::json!({"median": r, "ci": c, "n": n});
            }
            filt_json.push(fj);
        }
    }

    if json {
        let v = serde_json::json!({
            "entered": entered.len(), "low_power": low_power, "no_price": no_price, "heaviest": heaviest,
            "populations": pops.iter().map(|(l, pop, _)| serde_json::json!({"label": l, "tokens": pop.len(), "creators": creators(pop),
                "mints": pop.iter().map(|t| &t.mint).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            "tokens": all,
            "hypotheses": hyps_json, "returns": rets_json, "filters": filt_json,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        print!("{out}");
    }
    Ok(())
}

/// Fila por token con T_entry2 para la validación 3 (y su prueba sobre V2).
struct V3Row<'a> {
    i: &'a store::Entry2Input,
    e: entry2::Entry2,
    creator: String,
    mayhem: bool,
    /// H4', H6', H7' con las definiciones congeladas.
    h4: bool,
    h6: Option<bool>,
    h7: bool,
    d2: Option<bool>,
    /// Pump-y-caída en 1 h; `None` sin ventana de precio de 1 h.
    pd: Option<bool>,
    ret_a: [Option<f64>; 5],
    ret_b: [entry2::ExitB; 5],
    /// Slot del primer punto con precio ≥ 2× el inicial en la primera hora.
    first_2x: Option<u64>,
    /// H4' y H7' con identidades de creador limitadas a lo anterior a T_entry2.
    strict: (bool, bool),
}

/// Filas de los tokens con T_entry2; devuelve también cuántos no tienen
/// precio inicial y cuántos no tienen T_entry2.
fn v3_rows<'a>(st: &store::Store, inputs: &'a [store::Entry2Input]) -> Result<(Vec<V3Row<'a>>, usize, usize)> {
    let (p, pb) = (entry2::PARAMS, h1b::PARAMS_1H);
    let (mut rows, mut no_init, mut no_entry) = (Vec::new(), 0, 0);
    for i in inputs {
        let Some(init) = i.initial else {
            no_init += 1;
            continue;
        };
        let compute = |ids: &std::collections::HashSet<String>| {
            entry2::compute(i.t0, i.create_slot, (init.quote_reserves, init.token_reserves), &i.trades, ids, &p)
        };
        let Some(e) = compute(&i.creator_ids) else {
            no_entry += 1;
            continue;
        };
        let strict_e = compute(&st.creator_identities_before(&i.mint, e.entry_slot)?)
            .context("T_entry2 no depende de las identidades")?;
        let price = |x: &h1b::Point| x.quote_reserves as f64 / x.token_reserves as f64;
        let flat: Vec<(i64, f64)> = i.points.iter().map(|x| (x.timestamp, price(x))).collect();
        let pts3: Vec<(i64, u64, f64)> =
            i.points.iter().zip(&i.point_slots).map(|(x, &s)| (x.timestamp, s, price(x))).collect();
        let window: Vec<&entry::Trade> =
            i.trades.iter().filter(|t| (i.t0..i.t0 + p.window_secs).contains(&t.timestamp)).collect();
        let chain = entry2::chained(init.token_reserves, &window);
        let covered = i.price_horizon.map(|h| i.t0 + h - 1);
        let hour = i.price_horizon.is_some_and(|h| h >= pb.horizon_secs);
        let pd = hour.then(|| {
            let r = h1b::compute(i.t0, init, &i.points, i.completed_at, i.migrated_at, &pb);
            r.peak_multiple >= pb.min_peak_multiple && r.drawdown > pb.max_drawdown
        });
        let p0 = price(&init);
        let first_2x = hour
            .then(|| pts3.iter().find(|x| (i.t0..i.t0 + pb.horizon_secs).contains(&x.0) && x.2 >= 2.0 * p0).map(|x| x.1))
            .flatten();
        let (h4, h6, h7) = (e.h4() == Some(true), e.h6(&p), e.h7(&p) == Some(true));
        rows.push(V3Row {
            i,
            creator: st.creation_creator(&i.mint)?,
            mayhem: st.is_mayhem(&i.mint)?,
            h4,
            h6,
            h7,
            d2: v3::d2(h4, h6, h7),
            pd,
            ret_a: v3::HORIZONS.map(|(h, _)| entry2::return_at(&e, &flat, covered, i.completed_at, h)),
            ret_b: v3::HORIZONS.map(|(h, _)| entry2::return_b(&e, &pts3, &chain, covered, i.completed_at, h)),
            first_2x,
            strict: (strict_e.h4() == Some(true), strict_e.h7(&p) == Some(true)),
            e,
        });
    }
    Ok((rows, no_init, no_entry))
}

fn v3_cands<'a>(rows: &'a [V3Row]) -> Vec<v3::Cand<'a>> {
    rows.iter()
        .map(|r| v3::Cand { mint: &r.i.mint, creator: &r.creator, slot: r.i.create_slot, mayhem: r.mayhem })
        .collect()
}

/// Mediana · fracción > 0 (n) de los valores.
fn med_frac(v: &[f64]) -> String {
    let n = v.len();
    let pos = v.iter().filter(|&&x| x > 0.0).count();
    let m = replica::median(&mut v.to_vec());
    format!("{} · {} ({n})", pct1(m), if n == 0 { "n/a".into() } else { format!("{:.0}%", 100.0 * pos as f64 / n as f64) })
}

/// Tablas descriptivas por grupo D2 y horizonte (versiones (a) y (b)).
fn v3_return_tables(out: &mut String, rows: &[&V3Row]) {
    for (k, (_, hl)) in v3::HORIZONS.iter().enumerate() {
        let (mut single, mut rebuilt, mut unresolved) = (0, 0, 0);
        for r in rows {
            match r.ret_b[k] {
                entry2::ExitB::Single(_) => single += 1,
                entry2::ExitB::Rebuilt(_) => rebuilt += 1,
                entry2::ExitB::Unresolved => unresolved += 1,
                entry2::ExitB::NotEvaluable => {}
            }
        }
        out.push_str(&format!(
            "  {hl}: (b) slot de salida con un trade {single}, varios reconstruidos {rebuilt}, no reconstruidos (n/e en (b)) {unresolved}\n"
        ));
        for (label, g) in [("pasan", Some(false)), ("descartados", Some(true)), ("D2 n/e", None)] {
            let grp: Vec<&&V3Row> = rows.iter().filter(|r| r.d2 == g).collect();
            if g.is_none() && grp.is_empty() {
                continue;
            }
            let a: Vec<f64> = grp.iter().filter_map(|r| r.ret_a[k]).collect();
            let b: Vec<f64> = grp.iter().filter_map(|r| r.ret_b[k].value()).collect();
            out.push_str(&format!("    {label:11} (a) {:28} (b) {}\n", med_frac(&a), med_frac(&b)));
        }
    }
}

/// Validación 3 (CLAUDE.md 8, pre-registro 2026-10-01 16:39:58 UTC y adenda
/// operativa 17:00:21 UTC): filtro D2 contra el retorno neto (a) a +30 min.
/// Con `check`, solo señales, poblaciones y estado de los datos, sin ningún
/// retorno.
fn entry2_v3_report(st: &store::Store, json: bool, check: bool) -> Result<()> {
    let (from, until) = (entry2::VALIDATION3_FROM_TS, entry2::VALIDATION3_UNTIL_TS);
    let indexed = st.tramo_complete(from, until + v3::UNTIL_MARGIN_SECS)?;
    let created = st.creations_in_ts(from, until)?;
    let inputs = st.entry2_inputs_ts(from, until)?;
    let (rows, no_init, no_entry) = v3_rows(st, &inputs)?;

    // Estado de los datos (adenda 3).
    let mut no_window: Vec<(String, String, u64, bool, v3::Data)> = Vec::new();
    for (slot, mint, _, complete) in &created {
        if !complete {
            let d = v3::data_status(false, st.attempts("window", mint)?);
            no_window.push((mint.clone(), st.creation_creator(mint)?, *slot, st.is_mayhem(mint)?, d));
        }
    }
    let mut price_data = std::collections::HashMap::new();
    for r in &rows {
        let ok = r.i.price_horizon.is_some_and(|h| h >= PRICE_DOWNLOAD_SECS);
        price_data.insert(r.i.mint.as_str(), v3::data_status(ok, st.attempts("prices", &r.i.mint)?));
    }
    let window_pending = no_window.iter().filter(|x| x.4 == v3::Data::Pending).count();
    let window_sin_datos = no_window.iter().filter(|x| x.4 == v3::Data::SinDatos).count();
    let price_pending = price_data.values().filter(|&&d| d == v3::Data::Pending).count();

    let cands = v3_cands(&rows);
    let (pop_idx, ties) = v3::population(&cands, false);
    let (may_idx, may_ties) = v3::population(&cands, true);
    let pop: Vec<&V3Row> = pop_idx.iter().map(|&i| &rows[i]).collect();
    let mayhem_all: Vec<&V3Row> = rows.iter().filter(|r| r.mayhem).collect();
    let mayhem_one: Vec<&V3Row> = may_idx.iter().map(|&i| &rows[i]).collect();
    let nd_cands: Vec<v3::Cand> = no_window
        .iter()
        .filter(|x| x.4 == v3::Data::SinDatos)
        .map(|x| v3::Cand { mint: &x.0, creator: &x.1, slot: x.2, mayhem: x.3 })
        .collect();
    let chosen: Vec<&v3::Cand> = pop_idx.iter().map(|&i| &cands[i]).collect();
    let creators_out = v3::creators_sin_datos(&chosen, &nd_cands);
    let pop_sin_datos = pop.iter().filter(|r| price_data[r.i.mint.as_str()] == v3::Data::SinDatos).count();
    let count = |g: &[&V3Row], d: Option<bool>| g.iter().filter(|r| r.d2 == d).count();
    let strict = |g: &[&V3Row]| {
        v3::identity_changes(&g.iter().map(|r| ((r.h4, r.h7), r.strict)).collect::<Vec<_>>())
    };

    let mut out = String::new();
    macro_rules! say { ($($a:tt)*) => { out.push_str(&format!($($a)*)); out.push('\n'); } }
    say!(
        "VALIDACIÓN 3 — FILTRO D2{}. Tramo: timestamp del CreateEvent en [{from}, {until}). Parámetros: {}",
        if check { " — CHEQUEO (sin ningún retorno)" } else { "" },
        serde_json::to_string(&entry2::PARAMS)?
    );
    say!(
        "indexado del tramo (index-tramo --until ≥ {}): {}",
        until + v3::UNTIL_MARGIN_SECS,
        match indexed {
            Some((u, m)) => format!("COMPLETO hasta blockTime {u}; creaciones con timestamp ≠ blockTime (borde, se reportan, no cambian el tramo): {m}"),
            None => "NO REGISTRADO COMO COMPLETO".into(),
        }
    );
    say!(
        "creaciones en el tramo: {} (slots [{}, {}])  sin precio inicial: {no_init}  sin T_entry2: {no_entry}  con T_entry2: {} (sin mayhem {}, mayhem {})",
        created.len(),
        created.first().map_or("-".into(), |c| c.0.to_string()),
        created.last().map_or("-".into(), |c| c.0.to_string()),
        rows.len(),
        rows.iter().filter(|r| !r.mayhem).count(),
        mayhem_all.len()
    );
    say!(
        "ventana temprana incompleta: {} (sin datos tras {} intentos: {window_sin_datos}; pendientes: {window_pending})  \
         precio de 65 min incompleto entre los de T_entry2: {} (sin datos: {}; pendientes: {price_pending})",
        no_window.len(),
        v3::MAX_ATTEMPTS,
        price_data.values().filter(|&&d| d != v3::Data::Complete).count(),
        price_data.values().filter(|&&d| d == v3::Data::SinDatos).count()
    );
    say!(
        "población (sin mayhem, un token por creator): {} tokens, {} creators; empates de slot resueltos por mint: {ties}",
        pop.len(),
        v3::creators(&cands, &pop_idx)
    );
    say!(
        "D2 en la población: descartados {}  pasan {}  no evaluables (H6' n/e y una sola de H4'/H7'; adenda 1): {}  sin datos de precio (adenda 3): {pop_sin_datos}",
        count(&pop, Some(true)),
        count(&pop, Some(false)),
        count(&pop, None)
    );
    let (c4, c7) = strict(&pop);
    say!("identidades limitadas a lo anterior a T_entry2 (descriptivo): cambiarían H4' en {c4} y H7' en {c7} tokens de la población");
    say!(
        "mayhem (aparte, sin criterio): {} tokens con T_entry2 ({} un token por creator; empates {may_ties}); D2 descartados {} pasan {} no evaluables {}",
        mayhem_all.len(),
        mayhem_one.len(),
        count(&mayhem_all, Some(true)),
        count(&mayhem_all, Some(false)),
        count(&mayhem_all, None)
    );
    say!(
        "creators fuera de la comparación por un token sin ventana temprana anterior a su token elegido (adenda 7, 'sin datos'): {}{}",
        creators_out.len(),
        if creators_out.is_empty() { String::new() } else { format!(" ({})", creators_out.join(", ")) }
    );
    if check {
        if json {
            let v: Vec<_> = rows
                .iter()
                .map(|r| serde_json::json!({"mint": r.i.mint, "creator": r.creator, "mayhem": r.mayhem, "entry2": r.e,
                    "h4": r.h4, "h6": r.h6, "h7": r.h7, "d2": r.d2, "strict_h4_h7": r.strict,
                    "price_data": price_data[r.i.mint.as_str()],
                    "in_population": pop.iter().any(|p| p.i.mint == r.i.mint)}))
                .collect();
            println!("{}", serde_json::to_string_pretty(&v)?);
        } else {
            print!("{out}");
        }
        return Ok(());
    }

    // Ejecución única: solo con el indexado completo y sin descargas pendientes (adenda 3).
    if indexed.is_none() || created.is_empty() {
        anyhow::bail!("validacion3: indexado del tramo incompleto o no registrado; no se calcula el veredicto. Revisa con --check");
    }
    if window_pending > 0 || price_pending > 0 {
        anyhow::bail!(
            "validacion3: descargas pendientes (ventanas {window_pending}, precios {price_pending}, con menos de {} intentos); \
             no se calcula el veredicto. Revisa con --check",
            v3::MAX_ATTEMPTS
        );
    }
    let items: Vec<v3::Item> = pop
        .iter()
        .map(|r| v3::Item {
            mint: &r.i.mint,
            slot: r.i.create_slot,
            window_trades: r.e.window_trades,
            d2: r.d2,
            creator_sin_datos: creators_out.contains(&r.creator.as_str()),
            sin_datos: price_data[r.i.mint.as_str()] == v3::Data::SinDatos,
            ret: r.ret_a[v3::PRIMARY],
        })
        .collect();
    let (obs, ex) = v3::comparison(&items);
    let heaviest = v3::heaviest(&obs);
    let c = v3::criteria(&obs, heaviest);
    let (verdict, ok) = v3::decide(&c);
    say!(
        "excluidos de la comparación: creators sin datos (adenda 7) {} · sin datos de precio {} · no evaluables {} · retorno (a) +30 min n/e {}  (comparación: {} tokens)",
        ex.creator_sin_datos,
        ex.sin_datos,
        ex.no_evaluable,
        ex.ret_ne,
        obs.len()
    );
    let mark = |b: bool| if b { "cumple" } else { "NO cumple" };
    say!("\n==== Resultado principal: retorno neto (a) desde T_entry2 a +30 min, pasan frente a descartados ====");
    say!(
        "1. tokens: pasan {}, descartados {} (mínimo {} en cada grupo): {}",
        c.n_pass,
        c.n_disc,
        v3::MIN_GROUP,
        mark(ok[0])
    );
    say!(
        "2. Mann-Whitney unilateral (pasan > descartados): U = {}, p = {} (umbral < {}): {}",
        c.mw_u.map_or("n/a".into(), |u| format!("{u:.1}")),
        c.mw_p.map_or("n/a".into(), |p| format!("{p:.6}")),
        v3::MAX_P,
        mark(ok[1])
    );
    say!(
        "3. medianas: pasan {}%, descartados {}%; diferencia {} pts (umbral ≥ {:.0}, tolerancia 1e-9): {}",
        pct1(c.median_pass),
        pct1(c.median_disc),
        pct1(c.diff),
        100.0 * v3::MIN_DIFF,
        mark(ok[2])
    );
    let hw = heaviest.map(|i| obs[i].window_trades).unwrap_or(0);
    let ties_h = obs.iter().filter(|o| o.window_trades == hw).count();
    say!(
        "4. sin el token de mayor peso ({}, {hw} trades en 5 min; tokens con ese máximo: {ties_h}, desempate por slot y mint): diferencia {} pts (> 0): {}",
        c.heaviest.as_deref().unwrap_or("-"),
        pct1(c.diff_without_heaviest),
        mark(ok[3])
    );
    say!(
        "5. bootstrap ({} réplicas, semilla {}, por tokens): IC 95 % [{}, {}] (excluye 0, estricto): {}; réplicas descartadas por un grupo vacío: {}",
        replica::REPLICATES,
        replica::SEED,
        pct1(Some(c.ci.0)),
        pct1(Some(c.ci.1)),
        mark(ok[4]),
        c.ci.2
    );
    say!("VEREDICTO: {verdict}");

    say!("\n==== SECUNDARIO, DESCRIPTIVO, SIN CRITERIO ====");
    say!(
        "% descartado: población {} de {} ({}); comparación {} de {} ({})",
        count(&pop, Some(true)),
        pop.len(),
        pct1(Some(count(&pop, Some(true)) as f64 / pop.len().max(1) as f64)),
        c.n_disc,
        obs.len(),
        pct1(Some(c.n_disc as f64 / obs.len().max(1) as f64))
    );
    say!("\nRetorno neto por horizonte, población (celda: mediana · fracción > 0 (n))");
    v3_return_tables(&mut out, &pop);
    say!("\nPump-y-caída en 1 h (población) y momento del primer 2x respecto a T_entry2");
    for (label, g) in [("descartados", Some(true)), ("pasan", Some(false)), ("no evaluables", None)] {
        let grp: Vec<&&V3Row> = pop.iter().filter(|r| r.d2 == g).collect();
        if g.is_none() && grp.is_empty() {
            continue;
        }
        let pd: Vec<&&&V3Row> = grp.iter().filter(|r| r.pd == Some(true)).collect();
        let mut t: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for r in &pd {
            *t.entry(v3::first_2x_timing(r.first_2x, r.e.entry_slot)).or_default() += 1;
        }
        say!(
            "  {label:13} p&c {}/{} evaluables ({}%)  momento del primer 2x: {}",
            pd.len(),
            grp.iter().filter(|r| r.pd.is_some()).count(),
            pct1(Some(pd.len() as f64 / grp.iter().filter(|r| r.pd.is_some()).count().max(1) as f64)),
            t.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")
        );
    }
    let total_pd = pop.iter().filter(|r| r.pd == Some(true)).count();
    let cap = pop.iter().filter(|r| r.pd == Some(true) && r.d2 == Some(true)).count();
    say!("  capturados por D2: {cap} de {total_pd} p&c");
    say!("\nIdentidades de creador limitadas a lo anterior a T_entry2: cambiarían H4' en {c4} y H7' en {c7} tokens de la población");
    for (label, g) in [("MAYHEM, todos con T_entry2", &mayhem_all), ("MAYHEM, un token por creator", &mayhem_one)] {
        say!(
            "\n[{label}] n = {}  D2 descartados {}  pasan {}  no evaluables {}  p&c {}/{}",
            g.len(),
            count(g, Some(true)),
            count(g, Some(false)),
            count(g, None),
            g.iter().filter(|r| r.pd == Some(true)).count(),
            g.iter().filter(|r| r.pd.is_some()).count()
        );
        v3_return_tables(&mut out, g);
    }

    if json {
        let v = serde_json::json!({
            "verdict": verdict, "criteria_ok": ok, "criteria": c, "excluded": ex, "obs": obs,
            "population": pop.iter().map(|r| &r.i.mint).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        print!("{out}");
    }
    Ok(())
}

/// Prueba del código de la validación 3 sobre la validación 2 (sin veredicto):
/// % descartado por D2 y diferencia de medianas del retorno (a) a +30 min en
/// la población de `cribado_posthoc` (un token por creator con mayhem y luego
/// sin mayhem) y en la de la validación 3 (sin mayhem primero).
fn d2_prueba_report(st: &store::Store, json: bool) -> Result<()> {
    let inputs = st.entry2_inputs(entry2::VALIDATION2_FIRST_SLOT, entry2::VALIDATION2_LAST_SLOT)?;
    let (rows, _, _) = v3_rows(st, &inputs)?;
    let cands = v3_cands(&rows);
    let all_as_one: Vec<v3::Cand> =
        cands.iter().map(|c| v3::Cand { mint: c.mint, creator: c.creator, slot: c.slot, mayhem: false }).collect();
    let cribado: Vec<usize> = v3::population(&all_as_one, false).0.into_iter().filter(|&i| !rows[i].mayhem).collect();
    let v3pop = v3::population(&cands, false).0;
    let mut res = Vec::new();
    for (label, idx) in [("cribado_posthoc (V2 primaria, sin mayhem)", &cribado), ("regla de la validación 3", &v3pop)] {
        let pop: Vec<&V3Row> = idx.iter().map(|&i| &rows[i]).collect();
        let disc = pop.iter().filter(|r| r.d2 == Some(true)).count();
        let obs: Vec<v3::Obs> = pop
            .iter()
            .filter_map(|r| {
                Some(v3::Obs {
                    mint: r.i.mint.clone(),
                    slot: r.i.create_slot,
                    discarded: r.d2?,
                    ret: r.ret_a[v3::PRIMARY]?,
                    window_trades: 0,
                })
            })
            .collect();
        let refs: Vec<&v3::Obs> = obs.iter().collect();
        let d = v3::median_diff(&refs);
        if !json {
            println!(
                "PRUEBA DEL CÓDIGO SOBRE V2 (sin veredicto) — {label}: n {}  descartados {disc} ({})  comparación {}  diferencia de medianas {} pts",
                pop.len(),
                pct1(Some(disc as f64 / pop.len() as f64)),
                obs.len(),
                pct1(d)
            );
        }
        res.push(serde_json::json!({"label": label, "n": pop.len(), "discarded": disc, "comparison": obs.len(), "median_diff": d,
            "mints": pop.iter().map(|r| &r.i.mint).collect::<Vec<_>>()}));
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&res)?);
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
