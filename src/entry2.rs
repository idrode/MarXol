//! Validación congelada (CLAUDE.md sección 8): punto de entrada alcanzable
//! T_entry2 e indicadores previos H4', H5, H6', H7', H8, más el retorno neto
//! desde T_entry2 (secundario). Cálculo puro sobre datos almacenados; no toca
//! RPC. Los umbrales están congelados: no se tocan tras ver resultados.

use crate::entry::{execution_order, Trade};
use std::collections::{BTreeMap, HashSet};

/// Último slot de creación del tramo de calibración (los 40 del piloto).
/// Validación = creaciones con slot mayor.
pub const PILOT_LAST_SLOT: u64 = 451_340_397;
/// Último slot de creación de la validación 1 (sus 337 creaciones, indexadas
/// el 2026-09-28). Acota el tramo para que las creaciones posteriores (p. ej.
/// la validación 2) no entren en él; el conjunto sigue siendo el mismo.
pub const VALIDATION1_LAST_SLOT: u64 = 451_342_089;
/// Validación 2 (réplica, CLAUDE.md 8): S2 y slot de la creación n.º 400,
/// fijados por `index-tramo` el 2026-09-30 (400 creaciones exactas).
pub const VALIDATION2_FIRST_SLOT: u64 = 451_810_013;
pub const VALIDATION2_LAST_SLOT: u64 = 451_811_800;
/// Por debajo de esto, los NO CONFIRMADA de la validación 2 son "baja potencia".
pub const VALIDATION2_MIN_ENTERED: usize = 250;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Params {
    pub window_secs: i64,
    /// T_entry2: primer trade en slot > creación y con t ≥ t0 + esto.
    pub min_delay_secs: i64,
    /// H6': llenado en T_entry2 ≥ esto (%), mediana del piloto.
    pub fill_threshold_pct: f64,
    /// H7': wallets 1+1 antes de T_entry2 ≥ esto, mediana del piloto (o 1).
    pub min_pairs: usize,
    /// H8: el creador compra ≥ este % del supply en el slot de creación.
    pub creator_buy_pct: f64,
    pub supply: u64,
    /// Llenado solo evaluable si las reservas virtuales iniciales son las
    /// estándar (las reales iniciales no se guardan por token).
    pub v0_std: u64,
    pub r0_std: u64,
    /// Horizontes del retorno neto desde T_entry2 (s).
    pub return_horizons: [i64; 3],
}

pub const PARAMS: Params = Params {
    window_secs: crate::h1::PARAMS_V1.window_secs,
    min_delay_secs: 10,
    // Medianas del piloto con T_entry2 (n = 28), sin cruzar con el resultado.
    fill_threshold_pct: 13.65, // media de 11.84 y 15.46
    min_pairs: 1,
    creator_buy_pct: 15.0,
    supply: 1_000_000_000_000_000,
    v0_std: 1_073_000_000_000_000,
    r0_std: 793_100_000_000_000,
    return_horizons: [600, 1800, 3600],
};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Entry2 {
    pub entry_secs: i64,
    pub entry_slot: u64,
    /// Timestamp absoluto del trade T_entry2 (para los retornos).
    pub entry_ts: i64,
    /// Trades de la ventana temprana (peso del token) y anteriores a T_entry2.
    pub window_trades: usize,
    pub trades_before: usize,
    pub order_complete: bool,
    /// Precio justo antes del trade T_entry2 (último estado observable).
    pub entry_price: f64,
    pub entry_multiple: f64,
    /// Comisión del trade T_entry2 (fee + creator fee, bps).
    pub fee_bps: Option<u64>,
    /// H4'.
    pub dev_sold: bool,
    pub dev_sold_pct: f64,
    /// H5 (sin cambios respecto a la ronda anterior).
    pub same_slot_wallets: usize,
    /// H6': % de las reservas reales de token vendidas antes de T_entry2.
    pub fill_pct: Option<f64>,
    /// H7'.
    pub pairs: usize,
    /// H8: % del supply comprado por el creador en el slot de creación.
    pub creator_slot_buy_pct: f64,
}

impl Entry2 {
    pub fn h4(&self) -> Option<bool> {
        Some(self.dev_sold)
    }
    pub fn h5(&self) -> Option<bool> {
        Some(self.same_slot_wallets >= 1)
    }
    pub fn h6(&self, p: &Params) -> Option<bool> {
        self.fill_pct.map(|f| f >= p.fill_threshold_pct)
    }
    pub fn h7(&self, p: &Params) -> Option<bool> {
        Some(self.pairs >= p.min_pairs)
    }
    pub fn h8(&self, p: &Params) -> Option<bool> {
        Some(self.creator_slot_buy_pct >= p.creator_buy_pct)
    }
}

fn price(q: u64, t: u64) -> f64 {
    q as f64 / t as f64
}

/// `None` si no hay ningún trade que cumpla T_entry2 dentro de la ventana.
/// `initial` = reservas virtuales (quote, token) del `CreateEvent`.
pub fn compute(
    t0: i64,
    create_slot: u64,
    initial: (u64, u64),
    trades: &[Trade],
    creator_ids: &HashSet<String>,
    p: &Params,
) -> Option<Entry2> {
    let window: Vec<&Trade> = trades.iter().filter(|t| (t0..t0 + p.window_secs).contains(&t.timestamp)).collect();
    let (ordered, order_complete) = execution_order(initial.1, &window);
    let k = ordered.iter().position(|t| t.slot > create_slot && t.timestamp >= t0 + p.min_delay_secs)?;
    let before = &ordered[..k];
    let entry = ordered[k];
    let (q, v) = before.last().map_or(initial, |t| (t.quote_reserves, t.token_reserves));
    let pct = |x: u64| 100.0 * x as f64 / p.supply as f64;
    let is_creator = |t: &Trade| creator_ids.contains(&t.user);

    let dev_sells = before.iter().filter(|t| !t.is_buy && is_creator(t));
    let mut sides: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for t in before.iter().filter(|t| !is_creator(t)) {
        let s = sides.entry(&t.user).or_default();
        if t.is_buy {
            s.0 += 1;
        } else {
            s.1 += 1;
        }
    }
    let slot_buys = || before.iter().filter(|t| t.is_buy && t.slot == create_slot);
    Some(Entry2 {
        entry_secs: entry.timestamp - t0,
        entry_slot: entry.slot,
        entry_ts: entry.timestamp,
        window_trades: window.len(),
        trades_before: before.len(),
        order_complete,
        entry_price: price(q, v),
        entry_multiple: price(q, v) / price(initial.0, initial.1),
        fee_bps: entry.fee_bps,
        dev_sold: before.iter().any(|t| !t.is_buy && is_creator(t)),
        dev_sold_pct: pct(dev_sells.map(|t| t.token_amount).sum()),
        same_slot_wallets: slot_buys().filter(|t| !is_creator(t)).map(|t| t.user.as_str()).collect::<HashSet<_>>().len(),
        fill_pct: (initial.1 == p.v0_std).then(|| 100.0 * initial.1.saturating_sub(v) as f64 / p.r0_std as f64),
        pairs: sides.values().filter(|&&s| s == (1, 1)).count(),
        creator_slot_buy_pct: pct(slot_buys().filter(|t| is_creator(t)).map(|t| t.token_amount).sum()),
    })
}

/// Retorno neto de comprar a `entry` y vender a `exit` pagando `fee_bps` a la
/// entrada y a la salida (sin impacto de precio: posición marginal).
pub fn net_return(entry: f64, exit: f64, fee_bps: u64) -> f64 {
    let f = fee_bps as f64 / 10_000.0;
    exit / entry * (1.0 - f) * (1.0 - f) - 1.0
}

/// Retornos netos a cada horizonte desde T_entry2. `points` = (timestamp,
/// precio) de la curva. `None` si no es evaluable: sin comisión conocida,
/// fuera del rango de precio descargado (`covered_until`), o curva completada
/// antes de la salida (el precio sigue en PumpSwap, no descargado).
pub fn returns(
    e: &Entry2,
    points: &[(i64, f64)],
    covered_until: Option<i64>,
    completed_at: Option<i64>,
    p: &Params,
) -> [Option<f64>; 3] {
    p.return_horizons.map(|h| {
        let exit_t = e.entry_ts + h;
        let fee = e.fee_bps?;
        if covered_until? < exit_t || completed_at.is_some_and(|c| c <= exit_t) {
            return None;
        }
        let exit = points.iter().filter(|(t, _)| *t <= exit_t).max_by_key(|(t, _)| *t).map(|x| x.1)?;
        Some(net_return(e.entry_price, exit, fee))
    })
}

/// Test U de Mann-Whitney bilateral, aproximación normal con corrección de
/// empates y de continuidad. Devuelve (U de `x`, p).
pub fn mann_whitney(x: &[f64], y: &[f64]) -> Option<(f64, f64)> {
    let (n1, n2) = (x.len() as f64, y.len() as f64);
    if x.is_empty() || y.is_empty() {
        return None;
    }
    let mut all: Vec<(f64, bool)> = x.iter().map(|&v| (v, true)).chain(y.iter().map(|&v| (v, false))).collect();
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let n = all.len();
    let (mut r1, mut ties, mut i) = (0.0, 0.0, 0);
    while i < n {
        let mut j = i;
        while j + 1 < n && all[j + 1].0 == all[i].0 {
            j += 1;
        }
        let rank = (i + j) as f64 / 2.0 + 1.0;
        let t = (j - i + 1) as f64;
        ties += t * t * t - t;
        r1 += all[i..=j].iter().filter(|a| a.1).count() as f64 * rank;
        i = j + 1;
    }
    let u = r1 - n1 * (n1 + 1.0) / 2.0;
    let nn = n1 + n2;
    let var = n1 * n2 / 12.0 * ((nn + 1.0) - ties / (nn * (nn - 1.0)));
    if var <= 0.0 {
        return Some((u, 1.0));
    }
    let d = (u - n1 * n2 / 2.0).abs() - 0.5;
    let z = d.max(0.0) / var.sqrt();
    Some((u, erfc(z / std::f64::consts::SQRT_2).min(1.0)))
}

/// erfc con la aproximación de Numerical Recipes (error relativo < 1.2e-7).
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let r = t * (-z * z - 1.26551223
        + t * (1.00002368
            + t * (0.37409196
                + t * (0.09678418
                    + t * (-0.18628806
                        + t * (0.27886807 + t * (-1.13520398 + t * (1.48851587 + t * (-0.82215223 + t * 0.17087277)))))))))
        .exp();
    if x >= 0.0 { r } else { 2.0 - r }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V0: u64 = 1_073_000_000_000_000;
    const Q0: u64 = 30_000_000_000;

    /// Compra o venta de `amount` tokens que encadena con `*v` (reservas de
    /// token antes); el quote sigue la curva de producto constante.
    fn t(v: &mut u64, user: &str, is_buy: bool, amount: u64, slot: u64, ts: i64) -> Trade {
        *v = if is_buy { *v - amount } else { *v + amount };
        let q = ((Q0 as u128 * V0 as u128) / *v as u128) as u64;
        Trade {
            user: user.into(),
            is_buy,
            token_amount: amount,
            slot,
            timestamp: ts,
            quote_reserves: q,
            token_reserves: *v,
            fee_bps: Some(100),
        }
    }

    fn ids(xs: &[&str]) -> HashSet<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    fn params() -> Params {
        Params { fill_threshold_pct: 10.0, min_pairs: 1, ..PARAMS }
    }

    #[test]
    fn t_entry2_exige_slot_posterior_y_diez_segundos() {
        let mut v = V0;
        let tr = vec![
            t(&mut v, "dev", true, 200_000_000_000_000, 7, 0), // 20 % del supply
            t(&mut v, "x", true, 10_000_000_000_000, 7, 0),
            t(&mut v, "a", true, 5_000_000_000_000, 8, 4),
            t(&mut v, "a", false, 5_000_000_000_000, 9, 6),
            t(&mut v, "dev", false, 1_000_000_000_000, 9, 9),
            t(&mut v, "b", true, 1_000_000_000_000, 12, 11), // T_entry2
            t(&mut v, "dev", false, 50_000_000_000_000, 13, 12),
        ];
        let p = params();
        let e = compute(0, 7, (Q0, V0), &tr, &ids(&["dev"]), &p).unwrap();
        assert_eq!((e.entry_secs, e.trades_before, e.window_trades), (11, 5, 7));
        assert!(e.order_complete);
        assert_eq!(e.h4(), Some(true));
        assert!((e.dev_sold_pct - 0.1).abs() < 1e-9, "solo la venta anterior a T_entry2");
        assert_eq!(e.same_slot_wallets, 1);
        assert_eq!(e.pairs, 1);
        assert!((e.creator_slot_buy_pct - 20.0).abs() < 1e-9);
        assert_eq!(e.h8(&p), Some(true));
        // Vendidos antes de entrar: 200 + 10 + 5 − 5 − 1 = 209 (×10^12) de 793.1.
        assert!((e.fill_pct.unwrap() - 100.0 * 209.0 / 793.1).abs() < 1e-6);
        assert_eq!(e.h6(&p), Some(true));
    }

    #[test]
    fn sin_trade_valido_no_hay_entrada_y_llenado_no_evaluable_fuera_de_estandar() {
        let mut v = V0;
        let tr = vec![t(&mut v, "a", true, 1_000, 7, 0), t(&mut v, "b", true, 1_000, 8, 5)];
        assert!(compute(0, 7, (Q0, V0), &tr, &ids(&[]), &params()).is_none());
        // Mismo slot que la creación aunque pasen 10 s: no vale.
        let mut v = V0;
        let tr = vec![t(&mut v, "a", true, 1_000, 7, 12)];
        assert!(compute(0, 7, (Q0, V0), &tr, &ids(&[]), &params()).is_none());
        // Reservas iniciales no estándar: llenado n/e.
        let mut v = V0;
        let tr = vec![t(&mut v, "a", true, 1_000, 8, 12)];
        let e = compute(0, 7, (Q0, V0 + 1), &tr, &ids(&[]), &params());
        assert!(e.is_none_or(|e| e.fill_pct.is_none()));
    }

    #[test]
    fn retorno_neto_descuenta_comisiones_y_respeta_cobertura_y_graduacion() {
        let e = Entry2 { entry_ts: 100, entry_price: 1.0, fee_bps: Some(100), ..Default::default() };
        let pts = [(100, 1.0), (650, 2.0), (1500, 3.0), (3000, 0.5)];
        let r = returns(&e, &pts, Some(4000), None, &PARAMS);
        assert!((r[0].unwrap() - (2.0 * 0.99 * 0.99 - 1.0)).abs() < 1e-12);
        assert!((r[1].unwrap() - (3.0 * 0.99 * 0.99 - 1.0)).abs() < 1e-12);
        assert!((r[2].unwrap() - (0.5 * 0.99 * 0.99 - 1.0)).abs() < 1e-12);
        let r = returns(&e, &pts, Some(2000), Some(1000), &PARAMS);
        assert!(r[0].is_some() && r[1].is_none() && r[2].is_none());
        let sin_fee = Entry2 { fee_bps: None, ..e };
        assert_eq!(returns(&sin_fee, &pts, Some(4000), None, &PARAMS), [None, None, None]);
    }

    #[test]
    fn mann_whitney_coincide_con_valores_de_referencia() {
        // U=0; varianza 3·3·7/12, z = 4/√5.25 → p = 0.0809 (igual que scipy asymptotic)
        let (u, p) = mann_whitney(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]).unwrap();
        assert_eq!(u, 0.0);
        assert!((p - 0.0809).abs() < 1e-3, "{p}");
        // Con empates: [1,2,2,3] vs [2,3,3,4] → rangos medios, U=3; varianza
        // con corrección de empates 16/12·(9 − 48/56) → p = 0.1720.
        let (u, p) = mann_whitney(&[1.0, 2.0, 2.0, 3.0], &[2.0, 3.0, 3.0, 4.0]).unwrap();
        assert_eq!(u, 3.0);
        assert!((p - 0.1720).abs() < 1e-3, "{p}");
    }
}
