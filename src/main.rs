//! marXol — CLI de búsqueda y análisis on-chain de operadores de pump.fun.

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
            for (sig, failed, _) in sigs {
                if failed {
                    failed_txs += 1;
                    continue;
                }
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
        before = Some(last.0.clone());
        walked += page.len();
        for (sig, failed, slot) in page {
            if st.seen(&sig)? {
                skipped += 1;
                continue;
            }
            if failed {
                // Una tx fallida no emite eventos: no se gasta getTransaction en ella.
                st.mark_seen(&sig, slot)?;
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
                }
                // No se marca como vista: se reintentará en la próxima pasada.
                Err(e) => eprintln!("{sig}: {e:#}"),
            }
        }
        eprintln!("… {walked} firmas recorridas, {fetched} getTransaction, {skipped} ya vistas");
    }
    println!(
        "nuevas filas: {} creaciones, {} cambios de creador, {} completadas, {} migradas",
        total.creations, total.creator_changes, total.completions, total.migrations
    );
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
