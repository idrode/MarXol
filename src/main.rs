//! marXol — CLI de búsqueda y análisis on-chain de operadores de pump.fun.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use marxol::idl::Idl;
use marxol::ingest::{index, index_tramo, prices, unix_now, windows};
use marxol::pump::{self, Graduation};
use marxol::report::*;
use marxol::verify::{read_account, verify};
use marxol::{check, collect, historial, ledger, rpc, store, tx, vivo};
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
    /// Base de recolección: guarda el libro de CU diario compartido por todos
    /// los comandos que usan la red (y los datos de `collect`).
    #[arg(long, env = "MARXOL_VIVO_DB", default_value = "marxol_vivo.db", global = true)]
    vivo_db: String,
    /// Tope diario total de CU (día UTC), sumando collect y comandos manuales.
    #[arg(long, env = "MARXOL_DAILY_CU", default_value_t = ledger::DEFAULT_TOTAL, global = true)]
    daily_cu: u64,
    /// Cuota diaria de `collect` dentro del total; el resto es de los manuales.
    #[arg(long, env = "MARXOL_COLLECT_CU", default_value_t = ledger::DEFAULT_COLLECT, global = true)]
    collect_cu: u64,
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
    fn budget(&self, cli: &Cli, command: &str) -> Result<rpc::Budget> {
        let b = rpc::Budget::new(rpc::Provider::parse(&self.provider, &cli.rpc)?, self.max_rps, self.max_requests);
        with_ledger(b, cli, command, ledger::Pool::Manual)
    }
}

/// Añade al presupuesto el libro de CU diario compartido (`--vivo-db`).
fn with_ledger(b: rpc::Budget, cli: &Cli, command: &str, pool: ledger::Pool) -> Result<rpc::Budget> {
    let caps = ledger::Caps::new(cli.daily_cu, cli.collect_cu)?;
    let l = ledger::Ledger::open(&cli.vivo_db, command, pool, caps, b.provider.name, unix_now())?;
    Ok(b.with_ledger(l))
}

/// Presupuesto de los comandos manuales sin opciones de ritmo (`verify`,
/// `tx`, `scan`...): proveedor de `MARXOL_PROVIDER` o de la URL, sin tope
/// por ejecución, con el libro diario. `scan` e `index` llevan su propia
/// pausa (`--delay-ms`), así que no se les añade ritmo.
fn manual_budget(cli: &Cli, command: &str, paced: bool) -> Result<rpc::Budget> {
    let name = std::env::var("MARXOL_PROVIDER").unwrap_or_else(|_| "auto".into());
    let rps = (!paced).then_some(1e6);
    with_ledger(rpc::Budget::new(rpc::Provider::parse(&name, &cli.rpc)?, rps, None), cli, command, ledger::Pool::Manual)
}

/// Cierra la fila del proceso en el libro e imprime el consumo en stderr.
fn finish(b: &rpc::Budget) -> Result<()> {
    eprintln!("consumo: {}", b.summary());
    if let Some(l) = b.finish()? {
        eprintln!("{l}");
    }
    Ok(())
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
        /// Recorrer hacia atrás desde esta firma (excluida).
        #[arg(long)]
        before: Option<String>,
        /// Solo listar firmas (slot, hora, fallida), sin pedir las tx.
        #[arg(long)]
        solo_firmas: bool,
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
    /// Banco de evaluación (fase B) sobre D2 en la validación 3: modelo
    /// trivial, MCC, prevalencia, precisión/recall, AUPRC, IC, fragilidad,
    /// censura y bloques temporales. Comprueba que reproduce los criterios
    /// congelados. Sin red; abre `--db` en solo lectura.
    BancoD2 {
        /// Bloques temporales de igual duración en que se parte el tramo.
        #[arg(long, default_value_t = 2)]
        bloques: usize,
    },
    /// Fase C (exploratorio, post hoc): tenencia del creador y tx con varias
    /// cuentas en T_entry2 contra el retorno (a) a +30 min, en V1–V3, con el
    /// banco de evaluación y control por estratos de actividad. Sin red; `--db`
    /// en solo lectura.
    BancoFaseC {
        /// Añade el control de C1 por terciles de progreso de la curva en
        /// T_entry2 (docs/fase_c_control_progreso_2026-10-22.md).
        #[arg(long)]
        progreso: bool,
        /// Escribe tokens.tsv y compradores.tsv en este directorio (post hoc).
        #[arg(long)]
        volcado: Option<std::path::PathBuf>,
    },
    /// Recolección continua en `--vivo-db` (24/7): lista las creaciones desde
    /// el punto de reanudación, muestrea por hash de la firma (cubre las 24 h),
    /// descarga creación, ventana temprana y precio, sigue el estado de la
    /// curva a +1 h/+24 h/+7 d/+60 d y agrega con el código congelado. Gasta
    /// solo la bolsa `collect` del libro diario (`--collect-cu`).
    Collect {
        /// Tasa de muestreo en partes por millón de las creaciones listadas.
        #[arg(long, default_value_t = 3_000)]
        sample_ppm: u32,
        /// Segundos entre ciclos.
        #[arg(long, default_value_t = 300)]
        poll_secs: u64,
        /// Ciclos a ejecutar (por defecto, sin fin).
        #[arg(long)]
        cycles: Option<u64>,
        /// Páginas de 1 000 firmas para alcanzar el punto de reanudación; si no
        /// bastan, el hueco queda registrado.
        #[arg(long, default_value_t = 100)]
        max_backfill_pages: usize,
        /// Páginas de firmas por bonding curve (como `windows --max-pages`).
        #[arg(long, default_value_t = 5)]
        max_pages: usize,
        /// Tope de tx por token en cada ventana; con más, queda incompleta.
        #[arg(long, default_value_t = 400)]
        max_token_sigs: usize,
        /// Tokens por etapa y ciclo.
        #[arg(long, default_value_t = 50)]
        per_cycle: usize,
        /// Días de crudos que se guardan; 0 desactiva el borrado.
        #[arg(long, default_value_t = 30)]
        retention_days: i64,
        /// Carpeta de las exportaciones CSV comprimidas (verificadas antes de borrar).
        #[arg(long, default_value = "marxol_export")]
        export_dir: String,
        /// Mantenimiento cada N ciclos (exportar, borrar, vacuum, checkpoint).
        #[arg(long, default_value_t = 12)]
        maintenance_every: u64,
        /// Filas por transacción al borrar.
        #[arg(long, default_value_t = 500)]
        delete_batch: usize,
        #[command(flatten)]
        rate: RateArgs,
    },
    /// Estado de `--vivo-db`: punto de reanudación, tokens, CU medidos por
    /// ciclo y etapa, proyección diaria y cobertura por hora UTC. Sin red.
    CollectStatus {
        #[arg(long, default_value_t = 300)]
        poll_secs: u64,
        #[arg(long, default_value_t = 3_000)]
        sample_ppm: u32,
    },
    /// Fase 3 (opción C'): lanzamientos previos del pagador de tokens de
    /// `--db` (solo lectura), cruzando sus firmas con las de mint-authority,
    /// sin getTransaction. Escribe en `--out-db`. Es un mínimo (ver
    /// docs/propuesta_creadores_serie.md). Gasta la bolsa manual.
    PayerHistory {
        /// Creaciones con timestamp en [from, until) (por defecto, el tramo de V3).
        #[arg(long, default_value_t = marxol::entry2::VALIDATION3_FROM_TS)]
        from: i64,
        #[arg(long, default_value_t = marxol::entry2::VALIDATION3_UNTIL_TS)]
        until: i64,
        /// Tokens elegidos (los de menor fnv1a64(mint) del rango).
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Días de historial previo.
        #[arg(long, default_value_t = 7)]
        days: i64,
        /// Páginas de 1 000 firmas por pagador; si no bastan, queda truncado.
        #[arg(long, default_value_t = 5)]
        max_pages: usize,
        /// Páginas de mint-authority como máximo en esta ejecución.
        #[arg(long, default_value_t = 1_000)]
        max_ma_pages: usize,
        #[arg(long, default_value = "marxol_fase3.db")]
        out_db: String,
        /// Tx de creación muestreadas por pagador con ≥ --min-prior lanzamientos
        /// previos, para ver su `creator` (argumento); 0 = ninguna.
        #[arg(long, default_value_t = 0)]
        creator_samples: usize,
        #[arg(long, default_value_t = 50)]
        min_prior: i64,
        /// Tx de creación de --db muestreadas para el coste de lanzar; 0 = ninguna.
        #[arg(long, default_value_t = 0)]
        cost_samples: usize,
        #[command(flatten)]
        rate: RateArgs,
    },
    /// Informe de un operador (identidad = campo `creator`, no el payer).
    Operator { creator: String },
    /// Recuento de filas de la base local.
    Stats,
}

fn main() {
    // Ningún error sale sin pasar por `redact`: el del cliente HTTP lleva la URL con la clave.
    if let Err(e) = run() {
        eprintln!("Error: {}", rpc::redact(&format!("{e:?}")));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let rpc = RpcClient::new_with_timeout_and_commitment(
        cli.rpc.clone(),
        Duration::from_secs(30),
        CommitmentConfig::confirmed(),
    );
    let idl = Idl::pump()?;
    match cli.cmd {
        Cmd::Verify => {
            let b = manual_budget(&cli, "verify", true)?;
            let r = verify(&rpc, &b, &idl);
            finish(&b)?;
            r
        }
        Cmd::Global => {
            let b = manual_budget(&cli, "global", true)?;
            let g = read_account(&rpc, &b, &idl, pump::GLOBAL_PDA);
            finish(&b)?;
            let g = g?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&g.to_json())?);
            } else {
                print_decoded(&g);
                print_graduation(&Graduation::from_global(&g)?);
            }
            Ok(())
        }
        Cmd::Curve { ref mint } => {
            let mint = Pubkey::from_str(mint).context("mint inválido")?;
            let pda = pump::bonding_curve_pda(&mint);
            let b = manual_budget(&cli, "curve", true)?;
            let c = read_account(&rpc, &b, &idl, &pda.to_string());
            finish(&b)?;
            let c = c?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&c.to_json())?);
            } else {
                println!("bonding_curve: {pda}");
                print_decoded(&c);
            }
            Ok(())
        }
        Cmd::Index { ref address, limit, delay_ms } => {
            let b = manual_budget(&cli, "index", false)?;
            let mut st = store::Store::open(&cli.db)?;
            let r = index(&rpc, &b, &idl, &mut st, address, limit, delay_ms);
            finish(&b)?;
            r
        }
        Cmd::IndexTramo { from, count, until, ref rate } => {
            let b = rate.budget(&cli, "index-tramo")?;
            let mut st = store::Store::open(&cli.db)?;
            index_tramo(&rpc, &idl, &mut st, from, count, until, &b)
        }
        Cmd::Windows { limit, max_pages, ref rate } => {
            let b = rate.budget(&cli, "windows")?;
            let mut st = store::Store::open(&cli.db)?;
            windows(&rpc, &idl, &mut st, limit, max_pages, &b)
        }
        Cmd::H1 { variant } => {
            let st = store::Store::open(&cli.db)?;
            h1_report(&st, cli.json, &variant)
        }
        Cmd::Prices { limit, max_pages, ref rate } => {
            let b = rate.budget(&cli, "prices")?;
            let mut st = store::Store::open(&cli.db)?;
            prices(&rpc, &idl, &mut st, limit, max_pages, &b)
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
        Cmd::Check { ref mint, offline, max_pages, ref rate } => check_cmd(&rpc, &idl, &cli, mint, offline, max_pages, rate),
        Cmd::Collect {
            sample_ppm,
            poll_secs,
            cycles,
            max_backfill_pages,
            max_pages,
            max_token_sigs,
            per_cycle,
            retention_days,
            ref export_dir,
            maintenance_every,
            delete_batch,
            ref rate,
        } => {
            let cfg = collect::Config {
                sample_ppm,
                poll_secs,
                cycles,
                max_backfill_pages,
                max_pages,
                max_token_sigs,
                per_cycle,
                retention_days: (retention_days > 0).then_some(retention_days),
                export_dir: export_dir.clone(),
                maintenance_every,
                delete_batch,
            };
            let b = rpc::Budget::new(rpc::Provider::parse(&rate.provider, &cli.rpc)?, rate.max_rps, rate.max_requests);
            // Ritmo horario de la bolsa collect (desde 2026-10-26): ~29 000 CU/h con traspaso.
            let caps = ledger::Caps::new(cli.daily_cu, cli.collect_cu)?;
            let l = ledger::Ledger::open(&cli.vivo_db, "collect", ledger::Pool::Collect, caps, b.provider.name, unix_now())?;
            let b = b.with_ledger(l.with_hourly_pacing());
            let mut st = vivo::open(&cli.vivo_db, unix_now())?;
            let r = collect::run(&rpc, &idl, &mut st, &b, &cfg);
            finish(&b)?;
            r
        }
        Cmd::CollectStatus { poll_secs, sample_ppm } => {
            let st = vivo::open(&cli.vivo_db, unix_now())?;
            print!("{}", collect::status(&st, poll_secs, sample_ppm)?);
            Ok(())
        }
        Cmd::PayerHistory {
            from,
            until,
            limit,
            days,
            max_pages,
            max_ma_pages,
            ref out_db,
            creator_samples,
            min_prior,
            cost_samples,
            ref rate,
        } => {
            let cfg = historial::Config { days, max_pages_payer: max_pages, max_pages_ma: max_ma_pages };
            let b = rate.budget(&cli, "payer-history")?;
            // Regla del dueño (2026-10-22): no en paralelo con collect (se reparten el ritmo del plan y hubo 429).
            let running = b.ledger().map(|l| l.others_running("collect")).transpose()?.unwrap_or_default();
            if !running.is_empty() {
                finish(&b)?;
                anyhow::bail!("collect está corriendo (pid {running:?}): payer-history no se ejecuta en paralelo; páralo antes");
            }
            let src = store::Store::open_readonly(&cli.db)?;
            let db = historial::open(out_db)?;
            let n = historial::select_tokens(&db, &src, from, until, limit, &format!("fnv1a64(mint) menor, [{from}, {until}), {limit}"))?;
            eprintln!("{n} tokens en {out_db}");
            let r = historial::list_mint_authority(&rpc, &db, &b, &cfg)
                .and_then(|_| historial::scan_payers(&rpc, &db, &b, &cfg).map(|p| eprintln!("{p} pagadores recorridos")))
                .and_then(|_| {
                    if creator_samples + cost_samples == 0 {
                        return Ok(());
                    }
                    let sigs = historial::sample_signatures(&db, &src, days, min_prior, creator_samples, cost_samples)?;
                    let n = historial::sample_creates(&rpc, &idl, &db, &b, &sigs)?;
                    eprintln!("{n} tx de creación descargadas ({} pedidas)", sigs.len());
                    Ok(())
                });
            finish(&b)?;
            r?;
            let rows = historial::prior_launches(&db, days)?;
            let ok: Vec<_> = rows.iter().filter(|r| r.3 && r.4).collect();
            let bucket = |lo: i64, hi: i64| ok.iter().filter(|r| (lo..=hi).contains(&r.2)).count();
            println!("tokens con pagador recorrido: {} (completos y cubiertos: {}, truncados o sin cubrir: {})", rows.len(), ok.len(), rows.len() - ok.len());
            println!("lanzamientos previos del pagador en {days} d (mínimo): 0 → {}, 1 → {}, 2–4 → {}, 5–9 → {}, ≥10 → {}",
                bucket(0, 0), bucket(1, 1), bucket(2, 4), bucket(5, 9), bucket(10, i64::MAX));
            let trunc: Vec<_> = rows.iter().filter(|r| !r.3).collect();
            println!("truncados por el tope de páginas: {} (sus previos, mínimo: {:?})", trunc.len(), trunc.iter().map(|r| r.2).collect::<Vec<_>>());
            Ok(())
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
        Cmd::BancoD2 { bloques } => {
            let st = store::Store::open_readonly(&cli.db)?;
            print!("{}", banco_d2_v3(&st, bloques)?);
            Ok(())
        }
        Cmd::BancoFaseC { progreso, ref volcado } => {
            let st = store::Store::open_readonly(&cli.db)?;
            print!("{}", banco_fase_c(&st, progreso, volcado.as_deref())?);
            Ok(())
        }
        Cmd::Stats => {
            let st = store::Store::open(&cli.db)?;
            for (t, n) in st.counts()? {
                println!("{t:18} {n}");
            }
            Ok(())
        }
        Cmd::Tx { ref signature } => {
            let b = manual_budget(&cli, "tx", true)?;
            let t = b.acquire(rpc::Method::GetTransaction).and_then(|_| tx::fetch_events(&rpc, &idl, signature));
            finish(&b)?;
            print_tx(&t?, None, cli.json);
            Ok(())
        }
        Cmd::Scan { ref address, limit, ref events, all, delay_ms, ref before, solo_firmas } => {
            let b = manual_budget(&cli, "scan", false)?;
            let filter: Option<Vec<String>> = if all {
                None
            } else {
                Some(events.clone().unwrap_or_else(|| {
                    pump::IDENTITY_EVENTS
                        .iter()
                        .chain(pump::LIFECYCLE_EVENTS)
                        .map(|s| s.to_string())
                        .collect()
                }))
            };
            let sigs = b.acquire(rpc::Method::GetSignatures).and_then(|_| tx::recent_signatures(&rpc, address, limit, before.as_deref()));
            let sigs = match sigs {
                Ok(s) => s,
                Err(e) => {
                    finish(&b)?;
                    return Err(e);
                }
            };
            if solo_firmas {
                for s in &sigs {
                    println!("{} slot={} t={:?}{}", s.signature, s.slot, s.block_time, if s.failed { " FALLIDA" } else { "" });
                }
                return finish(&b);
            }
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
                if let Err(e) = b.acquire(rpc::Method::GetTransaction) {
                    eprintln!("{}", rpc::redact(&format!("{e:#}")));
                    break;
                }
                match tx::fetch_events(&rpc, &idl, &sig) {
                    Ok(t) => {
                        for e in &t.events {
                            *counts.entry(e.name.clone()).or_default() += 1;
                        }
                        errors += t.decode_errors.len();
                        print_tx(&t, filter.as_deref(), cli.json);
                    }
                    Err(e) => eprintln!("{sig}: {}", rpc::redact(&format!("{e:#}"))),
                }
            }
            if !cli.json {
                eprintln!(
                    "\n{fetched} tx leídas, {failed_txs} fallidas omitidas; eventos: {counts:?}; \
                     errores de decodificación: {errors}"
                );
            }
            finish(&b)
        }
    }
}


/// `marxol check`: imprime el informe y sale con 3 si no hay veredicto.
#[allow(clippy::too_many_arguments)]
fn check_cmd(rpc: &RpcClient, idl: &Idl, cli: &Cli, mint: &str, offline: bool, max_pages: usize, rate: &RateArgs) -> Result<()> {
    let (db, json) = (cli.db.as_str(), cli.json);
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
        let budget = rpc::Budget::new(rpc::Provider::parse(&rate.provider, &cli.rpc)?, rate.max_rps, Some(max));
        let budget = with_ledger(budget, cli, "check", ledger::Pool::Manual)?;
        let now = unix_now();
        let fetched = check::fetch(rpc, idl, &budget, &Pubkey::from_str(&mint)?, max_pages);
        // El consumo se registra también si la lectura falla.
        store::Store::open(db)?.record_rpc_usage("check", &budget, 1, now)?;
        finish(&budget)?;
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
