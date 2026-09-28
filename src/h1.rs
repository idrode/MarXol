//! Señal temprana H1 (CLAUDE.md sección 8): actividad orgánica en la ventana
//! temprana de un token. Cálculo puro sobre trades ya almacenados; no toca RPC.
//!
//! Todos los umbrales son provisionales [P] y viajan en `Params`, que se
//! persiste junto al resultado para saber con qué valores se calculó.

use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Params {
    /// Ventana desde `CreateEvent` [P].
    pub window_secs: i64,
    /// Mínimo de trades de una wallet para evaluar regularidad.
    pub bot_min_trades: usize,
    /// Bot si el CV de intervalos entre trades está por debajo [P].
    pub bot_cv_interval: f64,
    /// Bot si el CV del tamaño de trade está por debajo [P].
    pub bot_cv_size: f64,
    /// Se excluye un trade que supera esta fracción del volumen de la ventana [P].
    pub whale_fraction: f64,
    /// Mínimo de wallets orgánicas para `signal = true` [P].
    pub min_organic_wallets: usize,
    /// H1c: excluir wallets de exactamente 1 compra + 1 venta separadas por
    /// menos de estos segundos [P]. `None` en la variante v1.
    pub fast_pair_secs: Option<i64>,
}

pub const PARAMS_V1: Params = Params {
    window_secs: 300,
    bot_min_trades: 3,
    bot_cv_interval: 0.10,
    bot_cv_size: 0.05,
    whale_fraction: 0.15,
    min_organic_wallets: 3,
    fast_pair_secs: None,
};

/// Variante H1c (CLAUDE.md 8): v1 más la exclusión de pares rápidos.
pub const PARAMS_H1C: Params = Params { fast_pair_secs: Some(20), ..PARAMS_V1 };

pub fn variant(name: &str) -> Option<Params> {
    match name {
        "v1" => Some(PARAMS_V1),
        "h1c" => Some(PARAMS_H1C),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct Trade {
    pub user: String,
    pub is_buy: bool,
    /// Tamaño en unidades del `quote_mint`.
    pub size: u64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct H1Result {
    pub trades: usize,
    pub creator_trades: usize,
    pub bot_wallets: usize,
    pub bot_trades: usize,
    /// Wallets 1 compra + 1 venta excluidas por rapidez (H1c); incluidas
    /// también en `bot_wallets`/`bot_trades`.
    pub fast_pair_wallets: usize,
    pub whale_trades: usize,
    /// Wallets con algún trade tras todas las exclusiones.
    pub wallets_remaining: usize,
    pub organic_wallets: usize,
    pub signal: bool,
}

/// Coeficiente de variación (desviación típica poblacional / media).
/// Media 0 ⇒ todos los valores son 0 ⇒ regularidad total ⇒ 0.
fn cv(xs: &[f64]) -> f64 {
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    if mean == 0.0 {
        return 0.0;
    }
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    var.sqrt() / mean
}

/// Si la wallet hizo exactamente 1 compra y 1 venta, los segundos entre ambas.
fn pair_interval(ts: &[&Trade]) -> Option<i64> {
    match ts {
        [x, y] if x.is_buy != y.is_buy => Some((x.timestamp - y.timestamp).abs()),
        _ => None,
    }
}

/// Intervalos de todas las wallets 1 compra + 1 venta ajenas al creador de la
/// ventana (distribución de H1c, antes de cualquier otra exclusión).
pub fn pair_intervals(trades: &[Trade], creator_ids: &HashSet<String>) -> Vec<i64> {
    let mut by_user: BTreeMap<&str, Vec<&Trade>> = BTreeMap::new();
    for t in trades.iter().filter(|t| !creator_ids.contains(&t.user)) {
        by_user.entry(&t.user).or_default().push(t);
    }
    by_user.values().filter_map(|ts| pair_interval(ts)).collect()
}

/// `trades` deben ser ya los de la ventana. `creator_ids` es la unión de todas
/// las identidades de creador del mint (original, payer, cambios, embebidas).
pub fn compute(trades: &[Trade], creator_ids: &HashSet<String>, p: &Params) -> H1Result {
    let mut r = H1Result { trades: trades.len(), ..Default::default() };

    // Paso 2: creador.
    let non_creator: Vec<&Trade> = trades.iter().filter(|t| !creator_ids.contains(&t.user)).collect();
    r.creator_trades = trades.len() - non_creator.len();

    // Paso 3: bots, por wallet.
    let mut by_user: BTreeMap<&str, Vec<&Trade>> = BTreeMap::new();
    for t in &non_creator {
        by_user.entry(&t.user).or_default().push(t);
    }
    let mut bots = HashSet::new();
    for (user, ts) in &mut by_user {
        if let (Some(max), Some(dt)) = (p.fast_pair_secs, pair_interval(ts)) {
            if dt < max {
                bots.insert(*user);
                r.fast_pair_wallets += 1;
            }
            continue;
        }
        if ts.len() < p.bot_min_trades {
            continue;
        }
        ts.sort_by_key(|t| t.timestamp);
        let intervals: Vec<f64> =
            ts.windows(2).map(|w| (w[1].timestamp - w[0].timestamp) as f64).collect();
        let sizes: Vec<f64> = ts.iter().map(|t| t.size as f64).collect();
        if cv(&intervals) < p.bot_cv_interval || cv(&sizes) < p.bot_cv_size {
            bots.insert(*user);
        }
    }
    r.bot_wallets = bots.len();
    let human: Vec<&Trade> = non_creator.into_iter().filter(|t| !bots.contains(t.user.as_str())).collect();
    r.bot_trades = trades.len() - r.creator_trades - human.len();

    // Paso 4: ballenas, contra el volumen de la ventana cerrada (una pasada).
    let volume: u128 = human.iter().map(|t| t.size as u128).sum();
    let cap = volume as f64 * p.whale_fraction;
    let kept: Vec<&Trade> = human.iter().copied().filter(|t| t.size as f64 <= cap).collect();
    r.whale_trades = human.len() - kept.len();

    // Paso 5: wallets con compra y venta.
    let mut sides: BTreeMap<&str, (bool, bool)> = BTreeMap::new();
    for t in &kept {
        let s = sides.entry(&t.user).or_default();
        if t.is_buy {
            s.0 = true;
        } else {
            s.1 = true;
        }
    }
    r.wallets_remaining = sides.len();
    r.organic_wallets = sides.values().filter(|(b, s)| *b && *s).count();
    r.signal = r.organic_wallets >= p.min_organic_wallets;
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(user: &str, is_buy: bool, size: u64, timestamp: i64) -> Trade {
        Trade { user: user.into(), is_buy, size, timestamp }
    }

    fn ids(xs: &[&str]) -> HashSet<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    /// Cuatro wallets que compran y venden, más cuatro que solo compran (volumen
    /// de fondo), con tamaños parecidos: ningún trade supera el 15 % aunque se
    /// excluyan dos de las orgánicas.
    fn organicos() -> Vec<Trade> {
        let mut v = vec![];
        for (i, u) in ["a", "b", "c", "d"].iter().enumerate() {
            let i = i as u64;
            v.push(t(u, true, 100 + i, 10 + i as i64));
            v.push(t(u, false, 95 + 2 * i, 100 + 13 * i as i64));
        }
        for (i, u) in ["p1", "p2", "p3", "p4"].iter().enumerate() {
            v.push(t(u, true, 98 + i as u64, 40 + i as i64));
        }
        v
    }

    #[test]
    fn cuatro_wallets_organicas_dan_senal() {
        let r = compute(&organicos(), &ids(&[]), &PARAMS_V1);
        assert_eq!(r.organic_wallets, 4);
        assert!(r.signal);
    }

    #[test]
    fn el_creador_no_cuenta_aunque_sea_una_identidad_historica() {
        let r = compute(&organicos(), &ids(&["a", "b"]), &PARAMS_V1);
        assert_eq!(r.creator_trades, 4);
        assert_eq!(r.organic_wallets, 2);
        assert!(!r.signal);
    }

    #[test]
    fn wallet_regular_se_descarta_como_bot() {
        let mut v = organicos();
        // Cada 10 s exactos, tamaños variados: CV de intervalos = 0.
        for k in 0..4 {
            v.push(t("bot", k % 2 == 0, 50 + 20 * k as u64, 5 + 10 * k));
        }
        let r = compute(&v, &ids(&[]), &PARAMS_V1);
        assert_eq!(r.bot_wallets, 1);
        assert_eq!(r.bot_trades, 4);
        assert_eq!(r.organic_wallets, 4);
    }

    #[test]
    fn wallet_de_tamano_fijo_se_descarta_como_bot() {
        let mut v = organicos();
        for (k, ts) in [3, 20, 22, 90].iter().enumerate() {
            v.push(t("bot", k % 2 == 0, 77, *ts));
        }
        assert_eq!(compute(&v, &ids(&[]), &PARAMS_V1).bot_wallets, 1);
    }

    #[test]
    fn h1c_excluye_pares_rapidos_y_conserva_los_lentos() {
        let mut v = organicos(); // pares a..d separados 90–126 s
        v.push(t("rapida", true, 99, 50));
        v.push(t("rapida", false, 97, 55));
        let v1 = compute(&v, &ids(&[]), &PARAMS_V1);
        let c = compute(&v, &ids(&[]), &PARAMS_H1C);
        assert_eq!(v1.organic_wallets, 5);
        assert_eq!(c.organic_wallets, 4);
        assert_eq!(c.fast_pair_wallets, 1);
        assert_eq!(c.bot_trades, 2);
        let mut dts = pair_intervals(&v, &ids(&[]));
        dts.sort();
        assert_eq!(dts, vec![5, 90, 102, 114, 126]);
    }

    #[test]
    fn con_menos_de_tres_trades_no_se_evalua_regularidad() {
        let v = vec![t("x", true, 10, 1), t("x", false, 10, 1)];
        assert_eq!(compute(&v, &ids(&[]), &PARAMS_V1).bot_wallets, 0);
    }

    #[test]
    fn la_ballena_se_excluye_por_trade_no_por_wallet() {
        let mut v = organicos();
        v.push(t("e", true, 5_000, 30));
        v.push(t("e", false, 40, 60));
        let r = compute(&v, &ids(&[]), &PARAMS_V1);
        assert_eq!(r.whale_trades, 1);
        // "e" conserva su venta, pero ya no tiene compra: no es orgánica.
        assert_eq!(r.organic_wallets, 4);
        assert_eq!(r.wallets_remaining, 9);
    }

    #[test]
    fn seis_trades_iguales_quedan_todos_por_encima_del_15_por_ciento() {
        // Interacción entre umbrales: con 3 wallets × (compra + venta) de
        // igual tamaño, cada trade es 1/6 ≈ 16.7 % del volumen.
        let v: Vec<Trade> = ["a", "b", "c"]
            .iter()
            .flat_map(|u| [t(u, true, 100, 1), t(u, false, 100, 50)])
            .collect();
        let r = compute(&v, &ids(&[]), &PARAMS_V1);
        assert_eq!(r.whale_trades, 6);
        assert!(!r.signal);
    }
}
