//! marXol — CLI de búsqueda y análisis on-chain de operadores de pump.fun.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use marxol::idl::Idl;
use marxol::ingest::{index, index_tramo, prices, unix_now, windows};
use marxol::pump::{self, Graduation};
use marxol::report::*;
use marxol::verify::{read_account, verify};
use marxol::{check, rpc, store, tx};
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
    /// Peticiones por segundo (por defecto, el del proveedor: 12 en Alchemy,
    /// 8 en Helius, 4 en el RPC público).
    #[arg(long, env = "MARXOL_MAX_RPS")]
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
    /// Comprobación previa de un token con el filtro de descarte D2
    /// (validación 3): T_entry2, H4', H6', H7' con las definiciones congeladas.
    /// Lee la cadena (solo lectura), con un tope de 400 peticiones. Salida 0
    /// con veredicto, 3 si no es evaluable, 1 si falla la lectura.
    Check {
        /// Dirección del mint o URL de pump.fun que la contenga.
        mint: String,
        /// No usa la red: evalúa con los datos ya descargados en --db
        /// (ventana temprana completa), en solo lectura.
        #[arg(long)]
        offline: bool,
        /// Páginas de 1 000 firmas que se listan como máximo para llegar a la
        /// creación; si no bastan, NO EVALUABLE.
        #[arg(long, default_value_t = check::MAX_SIG_PAGES)]
        max_pages: usize,
        #[command(flatten)]
        rate: RateArgs,
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
        Cmd::Check { mint, offline, max_pages, rate } => {
            check_cmd(&rpc, &idl, &cli.db, &cli.rpc, &mint, offline, max_pages, &rate, cli.json)
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


/// `marxol check`: imprime el informe y sale con 3 si no hay veredicto.
#[allow(clippy::too_many_arguments)]
fn check_cmd(
    rpc: &RpcClient,
    idl: &Idl,
    db: &str,
    rpc_url: &str,
    mint: &str,
    offline: bool,
    max_pages: usize,
    rate: &RateArgs,
    json: bool,
) -> Result<()> {
    let mint = check::parse_mint(mint)?.to_string();
    let report = if offline {
        let st = store::Store::open_readonly(db)?;
        anyhow::ensure!(
            st.window_complete(&mint)?,
            "{mint}: sin ventana temprana completa en {db} (--offline solo usa datos ya descargados)"
        );
        check::evaluate(&st, &mint, None)?
    } else {
        let max = rate.max_requests.unwrap_or(check::MAX_REQUESTS);
        anyhow::ensure!(max <= check::MAX_REQUESTS, "--max-requests no puede pasar de {}", check::MAX_REQUESTS);
        let budget = rpc::Budget::new(rpc::Provider::parse(&rate.provider, rpc_url)?, rate.max_rps, Some(max));
        let now = unix_now();
        let fetched = check::fetch(rpc, idl, &budget, &Pubkey::from_str(&mint)?, max_pages);
        // El consumo se registra también si la lectura falla.
        store::Store::open(db)?.record_rpc_usage("check", &budget, 1, now)?;
        eprintln!("consumo: {}", budget.summary());
        for w in budget.retry_summary().into_iter().chain(tx::TRADE_EXTRA_BYTES.summary()) {
            eprintln!("{w}");
        }
        match fetched? {
            check::Fetched::Data(st) => check::evaluate(&st, &mint, Some(now))?,
            check::Fetched::TooCostly(reason) => check::Report::not_evaluable(&mint, reason),
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", check::render(&report));
    }
    if report.verdict == check::NO_EVALUABLE {
        std::process::exit(3);
    }
    Ok(())
}
