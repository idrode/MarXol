//! `marxol check <mint>`: comprobación previa de un token con el filtro de
//! descarte D2 (CLAUDE.md 8, validación 3). No define nada nuevo: T_entry2 y
//! los indicadores salen de `entry2::compute` y D2 de `v3::d2`, con los
//! parámetros congelados. Los datos de la cadena se descargan a una base en
//! memoria con el mismo `Store::ingest` que usa `windows`, y se leen con las
//! mismas consultas que `entry2_inputs`, así que el token se evalúa igual que
//! en las validaciones. Solo lectura sobre la cadena: ni wallets ni firmas.

use crate::idl::Idl;
use crate::store::{self, Store};
use crate::{entry2, pump, rpc, tx, v3};
use anyhow::{bail, Context, Result};
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use std::str::FromStr;

/// Tope de peticiones por llamada del comando: 400 × 40 CU = 16 000 CU en
/// Alchemy. Cuenta también las páginas de firmas y los reintentos.
pub const MAX_REQUESTS: u64 = 400;
/// Intentos para encontrar la tx de creación entre las firmas más antiguas.
const CREATION_TRIES: usize = 3;

pub const EVITAR: &str = "EVITAR";
pub const SIN_BANDERAS: &str = "SIN BANDERAS DE D2";
pub const NO_EVALUABLE: &str = "NO EVALUABLE";

/// Acepta la dirección del mint o una URL que la contenga en la ruta
/// (p. ej. `https://pump.fun/coin/<mint>`).
pub fn parse_mint(s: &str) -> Result<Pubkey> {
    let s = s.trim();
    if let Ok(p) = Pubkey::from_str(s) {
        return Ok(p);
    }
    let path = s.split(['?', '#']).next().unwrap_or(s);
    path.rsplit('/')
        .filter(|x| !x.is_empty())
        .find_map(|x| Pubkey::from_str(x).ok())
        .with_context(|| format!("no es una dirección de mint ni una URL que la contenga: {s}"))
}

#[derive(Debug, serde::Serialize)]
pub struct Report {
    pub mint: String,
    pub verdict: &'static str,
    /// Motivo de NO EVALUABLE.
    pub reason: Option<String>,
    pub bonding_curve: Option<String>,
    pub t0: Option<i64>,
    pub create_slot: Option<u64>,
    pub creator: Option<String>,
    pub payer: Option<String>,
    pub quote_mint: Option<String>,
    pub mayhem: Option<bool>,
    /// Tamaño de la unión de identidades de creador (paso 2 de H1).
    pub creator_ids: usize,
    /// La ventana temprana de 5 min aún no se había cerrado al descargar.
    pub window_open: bool,
    pub entry2: Option<entry2::Entry2>,
    pub h4: Option<bool>,
    pub h6: Option<bool>,
    pub h7: Option<bool>,
    pub d2: Option<bool>,
}

impl Report {
    pub fn not_evaluable(mint: &str, reason: String) -> Report {
        Report {
            mint: mint.to_string(),
            verdict: NO_EVALUABLE,
            reason: Some(reason),
            bonding_curve: None,
            t0: None,
            create_slot: None,
            creator: None,
            payer: None,
            quote_mint: None,
            mayhem: None,
            creator_ids: 0,
            window_open: false,
            entry2: None,
            h4: None,
            h6: None,
            h7: None,
            d2: None,
        }
    }
}

/// Evalúa un mint con los datos de `st` (la base en memoria de `fetch`, o
/// `marxol.db` con `--offline`). `now` = momento de la descarga, para saber
/// si la ventana temprana sigue abierta (`None`: cerrada).
pub fn evaluate(st: &Store, mint: &str, now: Option<i64>) -> Result<Report> {
    let p = entry2::PARAMS;
    let c = st.creation_info(mint)?.with_context(|| format!("{mint}: sin CreateEvent en los datos"))?;
    let initial = st.creation_initial(mint)?.with_context(|| format!("{mint}: sin precio inicial del CreateEvent"))?;
    let trades = st.entry_trades(mint, c.t0, store::EARLY_WINDOW_SECS)?;
    let ids = st.creator_identities(mint)?;
    let window_open = now.is_some_and(|n| n < c.t0 + p.window_secs + crate::BLOCK_TIME_SLACK);
    let mut r = Report::not_evaluable(mint, String::new());
    r.reason = None;
    r.bonding_curve = Some(c.bonding_curve);
    r.t0 = Some(c.t0);
    r.create_slot = Some(c.slot);
    r.creator = Some(c.creator);
    r.payer = Some(c.payer);
    r.quote_mint = c.quote_mint;
    r.mayhem = Some(c.mayhem);
    r.creator_ids = ids.len();
    r.window_open = window_open;
    let Some(e) = entry2::compute(c.t0, c.slot, initial, &trades, &ids, &p) else {
        r.reason = Some(if window_open {
            format!(
                "aún no hay T_entry2 y la ventana temprana sigue abierta: vuelve a comprobarlo a partir de t0 + {} s",
                p.window_secs + crate::BLOCK_TIME_SLACK
            )
        } else {
            format!(
                "sin T_entry2: ningún trade en un slot posterior al de creación con t ≥ t0 + {} s dentro de los {} primeros segundos",
                p.min_delay_secs, p.window_secs
            )
        });
        return Ok(r);
    };
    // Igual que las filas de la validación 3 (main.rs, `v3_rows`).
    let (h4, h6, h7) = (e.h4() == Some(true), e.h6(&p), e.h7(&p) == Some(true));
    let d2 = v3::d2(h4, h6, h7);
    r.verdict = match d2 {
        Some(true) => EVITAR,
        Some(false) => SIN_BANDERAS,
        None => {
            r.reason = Some(
                "H6' no es evaluable (reservas virtuales iniciales no estándar) y solo una de H4'/H7' se cumple: \
                 D2 no decide (adenda 1 de la validación 3)"
                    .into(),
            );
            NO_EVALUABLE
        }
    };
    (r.h4, r.h6, r.h7, r.d2) = (Some(h4), h6, Some(h7), d2);
    r.entry2 = Some(e);
    Ok(r)
}

/// Fecha UTC `AAAA-MM-DD hh:mm:ss` de un unix time.
fn utc(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // Algoritmo civil_from_days (H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", secs / 3600, secs % 3600 / 60, secs % 60)
}

fn yes(b: bool) -> &'static str {
    if b {
        "SÍ"
    } else {
        "no"
    }
}

pub fn render(r: &Report) -> String {
    let p = entry2::PARAMS;
    let mut o = String::new();
    let mut line = |s: String| {
        o.push_str(&s);
        o.push('\n');
    };
    line(format!("marxol check {}", r.mint));
    if let (Some(t0), Some(slot)) = (r.t0, r.create_slot) {
        line(format!("  bonding curve {}", r.bonding_curve.as_deref().unwrap_or("?")));
        line(format!("  creación {} (slot {slot}); quote {}", utc(t0), r.quote_mint.as_deref().unwrap_or("?")));
        line(format!(
            "  creator {}; payer {}; identidades de creador: {}",
            r.creator.as_deref().unwrap_or("?"),
            r.payer.as_deref().unwrap_or("?"),
            r.creator_ids
        ));
    }
    if let Some(e) = &r.entry2 {
        line(format!(
            "  T_entry2 a +{} s (slot {}), {} trades antes, precio {:.3}× el inicial",
            e.entry_secs, e.entry_slot, e.trades_before, e.entry_multiple
        ));
    }
    line(String::new());
    let flags = [r.h4, r.h6, r.h7].iter().filter(|&&h| h == Some(true)).count();
    match r.verdict {
        NO_EVALUABLE => line(format!("VEREDICTO: {NO_EVALUABLE} — {}", r.reason.as_deref().unwrap_or(""))),
        v => line(format!("VEREDICTO: {v}  (D2: {flags} de 3 banderas; descarta con 2 o más)")),
    }
    if let Some(e) = &r.entry2 {
        line("Banderas (definiciones congeladas, CLAUDE.md 8), con trades anteriores a T_entry2:".into());
        line(format!(
            "  H4' el creador vendió:            {:<3} ({:.2} % del supply vendido por identidades de creador)",
            yes(r.h4 == Some(true)),
            e.dev_sold_pct
        ));
        line(match (r.h6, e.fill_pct) {
            (Some(h), Some(f)) => format!(
                "  H6' curva llenada ≥ {:.2} %:      {:<3} ({f:.2} % de las reservas reales de token vendidas)",
                p.fill_threshold_pct,
                yes(h)
            ),
            _ => "  H6' curva llenada:                n/e (reservas virtuales iniciales no estándar)".into(),
        });
        line(format!(
            "  H7' wallets 1 compra + 1 venta ≥ {}: {:<3} (wallets ajenas al creador con 1 + 1: {})",
            p.min_pairs,
            yes(r.h7 == Some(true)),
            e.pairs
        ));
        if !e.order_complete {
            line("  aviso: el orden de ejecución no se pudo reconstruir entero por encadenado de reservas".into());
        }
    }
    if r.window_open && r.entry2.is_some() {
        line("  nota: la ventana de 5 min seguía abierta; las banderas solo usan trades anteriores a T_entry2".into());
    }
    if r.mayhem == Some(true) {
        line(String::new());
        line("TOKEN MAYHEM: D2 casi no actúa sobre estos tokens (en la validación 3 descartó 1 de 75) y su retorno".into());
        line("  medido es mucho peor (mediana −55 % a +30 min en los que pasan D2; su precio no es comparable).".into());
    }
    line(String::new());
    line("Avisos:".into());
    line("  - D2 solo descarta: \"SIN BANDERAS DE D2\" no es una señal de compra.".into());
    line("  - En las validaciones, los tokens que pasan D2 siguieron con retorno mediano negativo".into());
    line("    (−5.6 % a +30 min en la validación 3; solo un 2 % con retorno positivo).".into());
    line("  - Validado en tres franjas de 8 minutos; no generaliza a otros regímenes de mercado.".into());
    line("  - No es consejo financiero.".into());
    o
}

/// Páginas de 1 000 firmas que se listan como máximo para llegar a la
/// creación (`--max-pages`): un token más antiguo o activo no es evaluable.
pub const MAX_SIG_PAGES: usize = 25;

/// Lista las firmas de la bonding curve página a página (`page(before)`)
/// hasta llegar a la primera. `Err(motivo)` si pasaría de `max_pages` o del
/// tope de peticiones: el token no es evaluable y no se gasta más.
fn list_signatures(
    budget: &rpc::Budget,
    max_pages: usize,
    mut page: impl FnMut(Option<&str>) -> Result<Vec<tx::SigInfo>>,
) -> Result<std::result::Result<Vec<tx::SigInfo>, String>> {
    let mut sigs: Vec<tx::SigInfo> = Vec::new();
    for pages in 0.. {
        if pages == max_pages || !budget.has_room(1) {
            return Ok(Err(format!(
                "listadas {} firmas en {pages} páginas ({} CU) sin llegar a la creación: el token es demasiado \
                 antiguo o activo para el límite de {max_pages} páginas y {MAX_REQUESTS} peticiones",
                sigs.len(),
                budget.units()
            )));
        }
        let before = sigs.last().map(|s| s.signature.clone());
        let p = page(before.as_deref())?;
        let last = p.len() < 1000;
        sigs.extend(p);
        if last {
            break;
        }
    }
    Ok(Ok(sigs))
}

/// Resultado de la descarga: la base en memoria con el token, o el motivo
/// por el que no se puede evaluar dentro del tope de coste.
pub enum Fetched {
    Data(Store),
    TooCostly(String),
}

/// Descarga de la cadena lo que necesita `evaluate`: las firmas de la
/// bonding curve hasta la creación y las transacciones de la ventana
/// temprana (como `windows`). No pasa de `budget`; antes de pedir las
/// transacciones comprueba que caben.
pub fn fetch(rpc: &RpcClient, idl: &Idl, budget: &rpc::Budget, mint: &Pubkey, max_pages: usize) -> Result<Fetched> {
    let mint_s = mint.to_string();
    let curve = pump::bonding_curve_pda(mint).to_string();
    let too_costly = |what: &str| {
        Fetched::TooCostly(format!(
            "{what}: el token es demasiado antiguo o activo para el tope de {} peticiones ({} CU en Alchemy)",
            MAX_REQUESTS,
            MAX_REQUESTS * rpc::ALCHEMY.cost_get_transaction
        ))
    };

    // Firmas de la bonding curve, de la más nueva a la más antigua.
    let page = |before: Option<&str>| {
        rpc::with_retry(budget, rpc::Method::GetSignatures, || tx::recent_signatures(rpc, &curve, 1000, before))
    };
    let sigs = match list_signatures(budget, max_pages, page)? {
        Ok(s) => s,
        Err(why) => return Ok(Fetched::TooCostly(why)),
    };
    if sigs.is_empty() {
        bail!("{mint_s}: la bonding curve {curve} no tiene firmas (¿es un token de pump.fun?)");
    }

    // La creación es la tx exitosa más antigua de la curva.
    let mut st = Store::open(":memory:")?;
    let mut creation = None;
    for s in sigs.iter().rev().filter(|s| !s.failed).take(CREATION_TRIES) {
        if !budget.has_room(1) {
            return Ok(too_costly("sin cupo para leer la creación"));
        }
        let t = rpc::with_retry(budget, rpc::Method::GetTransaction, || tx::fetch_events(rpc, idl, &s.signature))?;
        if t.events.iter().any(|e| e.name == "CreateEvent" && e.str("mint") == Some(&mint_s)) {
            anyhow::ensure!(
                t.decode_errors.is_empty(),
                "{}: errores de decodificación en la creación: {}",
                s.signature,
                t.decode_errors.join("; ")
            );
            st.ingest(&t)?;
            st.ingest_prices(&t, store::EARLY_WINDOW_SECS)?;
            creation = Some(s.signature.clone());
            break;
        }
    }
    let creation = creation.with_context(|| format!("{mint_s}: no se encontró el CreateEvent entre las firmas más antiguas de la curva"))?;
    let t0 = st.creation_info(&mint_s)?.context("creación no guardada")?.t0;

    // Transacciones de la ventana temprana (mismo filtro por blockTime que `windows`).
    let slack = crate::BLOCK_TIME_SLACK;
    let window: Vec<&str> = sigs
        .iter()
        .filter(|s| !s.failed && s.signature != creation)
        .filter(|s| s.block_time.is_none_or(|bt| (t0 - slack..=t0 + store::EARLY_WINDOW_SECS + slack).contains(&bt)))
        .map(|s| s.signature.as_str())
        .collect();
    if !budget.has_room(window.len() as u64) {
        return Ok(too_costly(&format!(
            "la ventana temprana tiene {} transacciones y ya se han usado {} peticiones",
            window.len(),
            budget.requests()
        )));
    }
    let get = |sig: &str| rpc::with_retry(budget, rpc::Method::GetTransaction, || tx::fetch_events(rpc, idl, sig));
    if let Some(why) = ingest_window(&mut st, budget, &window, get)? {
        return Ok(too_costly(&why));
    }
    Ok(Fetched::Data(st))
}

/// Pide e ingiere las transacciones de la ventana temprana con `get`, en el
/// orden de `window`. Para en la primera con eventos que el IDL no decodifica
/// (sin pedir ninguna más) y devuelve el error con sus discriminadores: un
/// evento desconocido podría ser un trade, y sin él la lectura no es completa.
/// `Some(motivo)` si se agota el tope de peticiones con reintentos.
fn ingest_window(
    st: &mut Store,
    budget: &rpc::Budget,
    window: &[&str],
    mut get: impl FnMut(&str) -> Result<tx::TxEvents>,
) -> Result<Option<String>> {
    for (i, &sig) in window.iter().enumerate() {
        let t = match get(sig) {
            Ok(t) => t,
            Err(e) if !budget.has_room(1) => return Ok(Some(format!("tope alcanzado con reintentos ({e:#})"))),
            Err(e) => return Err(e),
        };
        if !t.decode_errors.is_empty() {
            bail!(
                "{sig}: lectura incompleta, se para sin pedir las {} transacciones restantes de la ventana \
                 ({} peticiones usadas): {}",
                window.len() - i - 1,
                budget.requests(),
                t.decode_errors.join("; ")
            );
        }
        st.ingest(&t)?;
        st.ingest_prices(&t, store::EARLY_WINDOW_SECS)?;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idl::Decoded;
    use crate::tx::TxEvents;
    use serde_json::{json, Map, Value};
    use std::collections::BTreeMap;

    fn ev(name: &str, fields: Value) -> Decoded {
        let Value::Object(fields) = fields else { unreachable!() };
        Decoded { name: name.into(), fields, missing: vec![], trailing_bytes: 0 }
    }

    fn txe(sig: &str, slot: u64, events: Vec<Decoded>) -> TxEvents {
        TxEvents {
            signature: sig.into(),
            slot,
            block_time: None,
            failed: false,
            instructions: vec![],
            events,
            decode_errors: vec![],
        }
    }

    fn ingest(st: &mut Store, t: &TxEvents) {
        st.ingest(t).unwrap();
        st.ingest_prices(t, store::EARLY_WINDOW_SECS).unwrap();
    }

    const V0: u64 = 1_073_000_000_000_000;
    const Q0: u64 = 30_000_000_000;

    /// Token sintético con reservas encadenadas (producto constante):
    /// `(usuario, compra, tokens, slot, segundos desde t0)`.
    fn token(mayhem: bool, v0: u64, trades: &[(&str, bool, u64, u64, i64)]) -> Store {
        let mut st = Store::open(":memory:").unwrap();
        let t0 = 1_000;
        ingest(
            &mut st,
            &txe(
                "crea",
                100,
                vec![ev(
                    "CreateEvent",
                    json!({"mint":"M","bonding_curve":"B","creator":"C","user":"C","timestamp":t0,
                           "is_mayhem_mode":mayhem,"virtual_quote_reserves":Q0,"virtual_token_reserves":v0}),
                )],
            ),
        );
        let k = Q0 as u128 * v0 as u128;
        let mut v = v0;
        for (i, &(user, buy, amount, slot, dt)) in trades.iter().enumerate() {
            v = if buy { v - amount } else { v + amount };
            ingest(
                &mut st,
                &txe(
                    &format!("t{i}"),
                    slot,
                    vec![ev(
                        "TradeEvent",
                        json!({"mint":"M","user":user,"is_buy":buy,"token_amount":amount,"creator":"C",
                               "virtual_quote_reserves":(k / v as u128) as u64,"virtual_token_reserves":v,
                               "timestamp":t0 + dt,"fee_basis_points":95,"creator_fee_basis_points":30}),
                    )],
                ),
            );
        }
        st
    }

    const G: u64 = 1_000_000_000_000; // 0.1 % del supply

    #[test]
    fn ventana_para_en_el_primer_evento_no_decodificable() {
        use crate::tx::tests::{decode, old_idl, BUY_V2, SELL_V2, SWEEPS};
        let window = [BUY_V2, SWEEPS, SELL_V2];
        let budget = rpc::Budget::new(rpc::ALCHEMY, Some(1e6), Some(MAX_REQUESTS));
        for (idl, decodes) in [(old_idl(), false), (Idl::pump().unwrap(), true)] {
            let mut st = Store::open(":memory:").unwrap();
            let mut asked = Vec::new();
            let r = ingest_window(&mut st, &budget, &window, |sig| {
                asked.push(sig.to_string());
                Ok(decode(&idl, sig))
            });
            if decodes {
                assert!(r.unwrap().is_none());
                assert_eq!(asked, window);
            } else {
                // Con el IDL anterior, el barrido no se decodifica: para ahí y no pide el sell.
                let e = format!("{:#}", r.unwrap_err());
                assert_eq!(asked, [BUY_V2, SWEEPS]);
                assert!(e.starts_with(SWEEPS) && e.contains("1 transacciones restantes"), "{e}");
                assert!(e.contains("[74, 2b, 4d, bd, 11, 7a, 48, 2b]"), "{e}");
            }
        }
    }

    #[test]
    fn acepta_mint_y_url() {
        let m = "2Avh9SfjB6Fpk8EsfcA8HWGVzY5Edt5J4gqE1CzBxsdA";
        assert_eq!(parse_mint(m).unwrap().to_string(), m);
        assert_eq!(parse_mint(&format!("https://pump.fun/coin/{m}?ref=x")).unwrap().to_string(), m);
        assert_eq!(parse_mint(&format!("pump.fun/{m}/")).unwrap().to_string(), m);
        assert!(parse_mint("https://pump.fun/coin/xyz").is_err());
    }

    #[test]
    fn evitar_con_dev_vende_y_un_par() {
        let st = token(
            false,
            V0,
            &[
                ("C", true, 50 * G, 100, 0),
                ("W1", true, 5 * G, 101, 3),
                ("W1", false, 5 * G, 102, 5),
                ("C", false, 10 * G, 103, 6),
                ("W2", true, G, 104, 12), // T_entry2
            ],
        );
        let r = evaluate(&st, "M", None).unwrap();
        assert_eq!(r.verdict, EVITAR);
        assert_eq!((r.h4, r.h7), (Some(true), Some(true)));
        let e = r.entry2.as_ref().unwrap();
        assert_eq!((e.entry_secs, e.trades_before, e.pairs), (12, 4, 1));
        assert!((e.dev_sold_pct - 1.0).abs() < 1e-9);
        // 40 G vendidos de 793.1e12 reales → 5.04 % < 13.65 %.
        assert_eq!(r.h6, Some(false));
        assert!(render(&r).contains("VEREDICTO: EVITAR"));
    }

    #[test]
    fn sin_banderas_y_no_evaluables() {
        let st = token(false, V0, &[("C", true, 50 * G, 100, 0), ("W1", true, G, 101, 4), ("W2", true, G, 105, 11)]);
        let r = evaluate(&st, "M", None).unwrap();
        assert_eq!(r.verdict, SIN_BANDERAS);
        assert!(render(&r).contains("VEREDICTO: SIN BANDERAS DE D2") && !render(&r).to_lowercase().contains("comprar"));

        // Sin trades en un slot posterior a t0 + 10 s.
        let st = token(false, V0, &[("C", true, 50 * G, 100, 0), ("W1", true, G, 101, 4)]);
        let r = evaluate(&st, "M", None).unwrap();
        assert_eq!(r.verdict, NO_EVALUABLE);
        assert!(r.reason.as_deref().unwrap().contains("sin T_entry2"));
        let r = evaluate(&st, "M", Some(1_100)).unwrap();
        assert!(r.window_open && r.reason.as_deref().unwrap().contains("sigue abierta"));

        // Mayhem con reservas no estándar: H6' n/e y solo H7' → adenda 1.
        let st = token(
            true,
            V0 + 1,
            &[("W1", true, G, 101, 3), ("W1", false, G, 102, 5), ("W2", true, G, 104, 12)],
        );
        let r = evaluate(&st, "M", None).unwrap();
        assert_eq!((r.verdict, r.h6, r.mayhem), (NO_EVALUABLE, None, Some(true)));
        assert!(render(&r).contains("TOKEN MAYHEM"));
    }

    #[test]
    fn listado_para_al_limite_de_paginas_sin_gastar_mas() {
        let b = rpc::Budget::new(rpc::ALCHEMY, Some(1000.0), Some(MAX_REQUESTS));
        let full = |before: Option<&str>| {
            b.acquire(rpc::Method::GetSignatures)?;
            let k = before.map_or(0, |s| s[1..].parse::<usize>().unwrap() + 1);
            Ok((k..k + 1000)
                .map(|i| tx::SigInfo { signature: format!("s{i}"), failed: false, slot: 0, block_time: None })
                .collect())
        };
        let Err(why) = list_signatures(&b, 3, full).unwrap() else { panic!("debía parar") };
        assert_eq!(b.get_signatures.get(), 3, "no pide la cuarta página");
        assert!(why.contains("listadas 3000 firmas en 3 páginas (120 CU)"), "{why}");

        let b = rpc::Budget::new(rpc::ALCHEMY, Some(1000.0), Some(MAX_REQUESTS));
        let mut n = 0;
        let short = |_: Option<&str>| {
            n += 1;
            let len = if n < 3 { 1000 } else { 7 };
            Ok((0..len).map(|i| tx::SigInfo { signature: format!("s{n}-{i}"), failed: false, slot: 0, block_time: None }).collect())
        };
        let Ok(all) = list_signatures(&b, 3, short).unwrap() else { panic!("la tercera página llega a la creación") };
        assert_eq!(all.len(), 2007);
    }

    #[test]
    fn fecha_utc() {
        assert_eq!(utc(1_790_845_200), "2026-10-01 09:00:00 UTC");
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
    }

    // ---- Contra marxol.db (solo lectura; se saltan si no está) ----

    fn db() -> Option<Store> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/marxol.db");
        std::path::Path::new(path).exists().then(|| Store::open_readonly(path).unwrap())
    }

    /// Tramos V1, V2 y V3 con las mismas consultas que `entry2`.
    fn tramos(st: &Store) -> Vec<(&'static str, Vec<store::Entry2Input>)> {
        vec![
            ("V1", st.entry2_inputs(entry2::PILOT_LAST_SLOT + 1, entry2::VALIDATION1_LAST_SLOT).unwrap()),
            ("V2", st.entry2_inputs(entry2::VALIDATION2_FIRST_SLOT, entry2::VALIDATION2_LAST_SLOT).unwrap()),
            ("V3", st.entry2_inputs_ts(entry2::VALIDATION3_FROM_TS, entry2::VALIDATION3_UNTIL_TS).unwrap()),
        ]
    }

    /// D2 de referencia, calculado como `v3_rows` en main.rs.
    fn reference(i: &store::Entry2Input) -> Option<Option<bool>> {
        let p = entry2::PARAMS;
        let init = i.initial?;
        let e = entry2::compute(i.t0, i.create_slot, (init.quote_reserves, init.token_reserves), &i.trades, &i.creator_ids, &p)?;
        Some(v3::d2(e.h4() == Some(true), e.h6(&p), e.h7(&p) == Some(true)))
    }

    fn verdict_of(d2: Option<Option<bool>>) -> &'static str {
        match d2 {
            Some(Some(true)) => EVITAR,
            Some(Some(false)) => SIN_BANDERAS,
            _ => NO_EVALUABLE,
        }
    }

    #[test]
    fn coincide_con_entry2_en_v1_v2_v3() {
        let Some(st) = db() else { return };
        let mut seen = BTreeMap::<(&str, &str, bool), usize>::new();
        for (tramo, inputs) in tramos(&st) {
            assert!(!inputs.is_empty(), "{tramo} vacío");
            for i in &inputs {
                let want = verdict_of(reference(i));
                let r = evaluate(&st, &i.mint, None).unwrap();
                assert_eq!(r.verdict, want, "{tramo} {}", i.mint);
                *seen.entry((tramo, want, r.mayhem.unwrap())).or_default() += 1;
            }
        }
        for tramo in ["V1", "V2", "V3"] {
            for v in [EVITAR, SIN_BANDERAS] {
                assert!(seen.keys().any(|k| k.0 == tramo && k.1 == v), "{tramo} sin casos {v}: {seen:?}");
            }
            assert!(seen.keys().any(|k| k.0 == tramo && k.2), "{tramo} sin mayhem: {seen:?}");
        }
        eprintln!("veredictos por tramo/veredicto/mayhem: {seen:?}");
    }

    /// Reconstruye desde `marxol.db` las transacciones del token como las
    /// devolvería `getTransaction` (eventos en su índice original) y las pasa
    /// por el mismo camino que `fetch`: ingesta en memoria y `evaluate`.
    fn fixture(src: &Store, mint: &str) -> Store {
        let c = src.conn();
        let mut txs: BTreeMap<String, (u64, BTreeMap<usize, Decoded>)> = BTreeMap::new();
        let mut put = |sig: String, slot: i64, idx: i64, d: Decoded| {
            txs.entry(sig).or_insert_with(|| (slot as u64, BTreeMap::new())).1.insert(idx as usize, d);
        };
        let row_obj = |r: &rusqlite::Row, names: &[&str]| -> Map<String, Value> {
            let mut m = Map::new();
            for (k, n) in names.iter().enumerate() {
                let v: rusqlite::types::Value = r.get(k).unwrap();
                let v = match v {
                    rusqlite::types::Value::Integer(i) if *n == "is_buy" || *n == "is_mayhem_mode" => json!(i != 0),
                    rusqlite::types::Value::Integer(i) => json!(i),
                    rusqlite::types::Value::Text(s) => json!(s),
                    _ => continue,
                };
                m.insert(n.to_string(), v);
            }
            m
        };
        // CreateEvent con las reservas de su punto de precio.
        let create = ["mint", "bonding_curve", "creator", "user", "quote_mint", "is_mayhem_mode", "timestamp",
            "virtual_quote_reserves", "virtual_token_reserves"];
        let mut q = c
            .prepare(
                "SELECT c.mint, c.bonding_curve, c.creator, c.user, c.quote_mint, c.is_mayhem_mode, c.timestamp,
                        p.quote_reserves, p.token_reserves, c.signature, c.slot, p.ev_index
                 FROM creations c JOIN price_points p ON p.mint = c.mint AND p.kind = 'create' WHERE c.mint = ?1",
            )
            .unwrap();
        let mut rows = q.query([mint]).unwrap();
        let mut creation = None;
        while let Some(r) = rows.next().unwrap() {
            let sig: String = r.get(9).unwrap();
            creation = Some(sig.clone());
            put(sig, r.get(10).unwrap(), r.get(11).unwrap(), Decoded {
                name: "CreateEvent".into(),
                fields: row_obj(r, &create),
                missing: vec![],
                trailing_bytes: 0,
            });
        }
        let changes = ["mint", "bonding_curve", "old_creator", "new_creator", "sharing_config", "timestamp"];
        let mut q = c
            .prepare(
                "SELECT mint, bonding_curve, old_creator, new_creator, sharing_config, timestamp, kind, signature, slot, ev_index
                 FROM creator_changes WHERE mint = ?1",
            )
            .unwrap();
        let mut rows = q.query([mint]).unwrap();
        while let Some(r) = rows.next().unwrap() {
            let mut f = row_obj(r, &changes);
            let kind: String = r.get(6).unwrap();
            let name = if kind == "set_creator" {
                let new = f.remove("new_creator").unwrap();
                f.insert("creator".into(), new);
                "SetCreatorEvent"
            } else {
                "MigrateBondingCurveCreatorEvent"
            };
            put(r.get(7).unwrap(), r.get(8).unwrap(), r.get(9).unwrap(), Decoded {
                name: name.into(),
                fields: f,
                missing: vec![],
                trailing_bytes: 0,
            });
        }
        let trade = ["mint", "user", "is_buy", "sol_amount", "quote_amount", "token_amount", "quote_mint", "creator",
            "ix_name", "virtual_sol_reserves", "virtual_token_reserves", "virtual_quote_reserves", "timestamp",
            "fee_basis_points", "creator_fee_basis_points"];
        let mut q = c
            .prepare(&format!("SELECT {}, signature, slot, ev_index FROM early_trades WHERE mint = ?1", trade.join(", ")))
            .unwrap();
        let mut rows = q.query([mint]).unwrap();
        while let Some(r) = rows.next().unwrap() {
            let n = trade.len();
            put(r.get(n).unwrap(), r.get(n + 1).unwrap(), r.get(n + 2).unwrap(), Decoded {
                name: "TradeEvent".into(),
                fields: row_obj(r, &trade),
                missing: vec![],
                trailing_bytes: 0,
            });
        }
        let creation = creation.expect("creación con precio inicial");
        let to_tx = |sig: &str, (slot, evs): &(u64, BTreeMap<usize, Decoded>)| {
            let n = evs.keys().max().map_or(0, |m| m + 1);
            let events = (0..n)
                .map(|k| evs.get(&k).cloned().unwrap_or_else(|| ev("Relleno", json!({}))))
                .collect();
            txe(sig, *slot, events)
        };
        let mut st = Store::open(":memory:").unwrap();
        ingest(&mut st, &to_tx(&creation, &txs[&creation]));
        for (sig, t) in &txs {
            if *sig != creation {
                ingest(&mut st, &to_tx(sig, t));
            }
        }
        st
    }

    #[test]
    fn camino_de_descarga_reproduce_entry2_con_fixtures() {
        let Some(src) = db() else { return };
        let mut n = 0;
        for (tramo, inputs) in tramos(&src) {
            for i in &inputs {
                let want = evaluate(&src, &i.mint, None).unwrap();
                let got = evaluate(&fixture(&src, &i.mint), &i.mint, None).unwrap();
                assert_eq!(got.verdict, verdict_of(reference(i)), "{tramo} {}", i.mint);
                assert_eq!(
                    serde_json::to_string(&got).unwrap(),
                    serde_json::to_string(&want).unwrap(),
                    "{tramo} {}",
                    i.mint
                );
                n += 1;
            }
        }
        eprintln!("{n} tokens reconstruidos desde fixtures");
    }
}
