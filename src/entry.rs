//! Punto de entrada hipotético T_entry e indicadores previos H4–H7
//! (CLAUDE.md sección 8). Cálculo puro sobre trades ya almacenados; no toca
//! RPC. Solo se usan trades hasta T_entry inclusive, para no filtrar
//! información posterior a la entrada.

use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Params {
    /// Ventana temprana en la que se busca la entrada [P].
    pub window_secs: i64,
    /// Se entra tras el primer trade con precio ≥ este múltiplo del inicial [P].
    pub entry_multiple: f64,
    /// H6: llenado "muy rápido" si T_entry llega en estos segundos o menos [P].
    pub fast_fill_secs: i64,
    /// H7: señal con al menos estas wallets 1 compra + 1 venta [P].
    pub min_pairs: usize,
    /// Supply para el % de H4/H5 (estándar; en mayhem no verificado).
    pub supply: u64,
}

pub const PARAMS: Params = Params {
    window_secs: crate::h1::PARAMS_V1.window_secs,
    entry_multiple: 1.5,
    fast_fill_secs: 10,
    min_pairs: 3,
    supply: 1_000_000_000_000_000,
};

/// Trade en orden de almacenamiento `(slot, timestamp, firma, índice)`, con
/// las reservas virtuales tras el trade.
#[derive(Debug, Clone)]
pub struct Trade {
    pub user: String,
    pub is_buy: bool,
    pub token_amount: u64,
    pub slot: u64,
    pub timestamp: i64,
    pub quote_reserves: u64,
    pub token_reserves: u64,
    /// `fee_basis_points + creator_fee_basis_points` del `TradeEvent`; `None`
    /// en trades descargados antes de guardar las comisiones (piloto).
    pub fee_bps: Option<u64>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Indicators {
    /// Segundos desde la creación hasta T_entry (H6).
    pub entry_secs: i64,
    /// Trades vistos hasta T_entry inclusive.
    pub trades_seen: usize,
    /// El orden de ejecución se reconstruyó entero por encadenado de reservas.
    pub order_complete: bool,
    /// Reserva real de quote al entrar, en unidades del quote_mint.
    pub entry_real_quote: u64,
    /// H4.
    pub dev_sold: bool,
    pub dev_first_sell_secs: Option<i64>,
    pub dev_sold_pct: f64,
    /// H5a (descriptivo): alguna identidad de creador compra en el slot de creación.
    pub creator_same_slot_buy: bool,
    /// H5: wallets ajenas al creador que compran en el slot de creación.
    pub same_slot_wallets: usize,
    pub same_slot_pct: f64,
    /// H7.
    pub pairs: usize,
}

impl Indicators {
    pub fn h4(&self) -> bool {
        self.dev_sold
    }
    pub fn h5(&self) -> bool {
        self.same_slot_wallets >= 1
    }
    pub fn h6(&self, p: &Params) -> bool {
        self.entry_secs <= p.fast_fill_secs
    }
    pub fn h7(&self, p: &Params) -> bool {
        self.pairs >= p.min_pairs
    }
}

fn price(q: u64, t: u64) -> f64 {
    q as f64 / t as f64
}

/// Orden de ejecución reconstruido encadenando reservas: las reservas de
/// token antes de cada trade (después ± `token_amount`) son las de después del
/// anterior, empezando por las del `CreateEvent`. El orden de almacenamiento
/// no sirve dentro de un slot (la firma no ordena la ejecución). Lo que no
/// encadena se añade al final en orden de almacenamiento; el booleano dice si
/// el encadenado fue completo.
pub fn execution_order<'a>(v0: u64, trades: &[&'a Trade]) -> (Vec<&'a Trade>, bool) {
    let before = |t: &Trade| {
        if t.is_buy {
            t.token_reserves.checked_add(t.token_amount)
        } else {
            t.token_reserves.checked_sub(t.token_amount)
        }
    };
    let mut used = vec![false; trades.len()];
    let mut out = Vec::with_capacity(trades.len());
    let mut cur = v0;
    while let Some(i) = (0..trades.len()).find(|&i| !used[i] && before(trades[i]) == Some(cur)) {
        used[i] = true;
        cur = trades[i].token_reserves;
        out.push(trades[i]);
    }
    let complete = out.len() == trades.len();
    out.extend((0..trades.len()).filter(|&i| !used[i]).map(|i| trades[i]));
    (out, complete)
}

/// `None` si el token no llega a la entrada dentro de la ventana.
/// `initial` = reservas virtuales (quote, token) del `CreateEvent`.
pub fn compute(
    t0: i64,
    create_slot: u64,
    initial: (u64, u64),
    trades: &[Trade],
    creator_ids: &HashSet<String>,
    p: &Params,
) -> Option<Indicators> {
    let p0 = price(initial.0, initial.1);
    let window: Vec<&Trade> = trades.iter().filter(|t| (t0..t0 + p.window_secs).contains(&t.timestamp)).collect();
    let (seen, order_complete) = execution_order(initial.1, &window);
    let k = seen
        .iter()
        .position(|t| t.token_reserves > 0 && price(t.quote_reserves, t.token_reserves) >= p.entry_multiple * p0)?;
    let seen = &seen[..=k];
    let entry = seen[k];
    let pct = |x: u64| 100.0 * x as f64 / p.supply as f64;
    let mut r = Indicators {
        entry_secs: entry.timestamp - t0,
        trades_seen: seen.len(),
        order_complete,
        entry_real_quote: entry.quote_reserves.saturating_sub(initial.0),
        ..Default::default()
    };

    let dev_sells: Vec<&&Trade> = seen.iter().filter(|t| !t.is_buy && creator_ids.contains(&t.user)).collect();
    r.dev_sold = !dev_sells.is_empty();
    r.dev_first_sell_secs = dev_sells.first().map(|t| t.timestamp - t0);
    r.dev_sold_pct = pct(dev_sells.iter().map(|t| t.token_amount).sum());

    let slot_buys: Vec<&&Trade> = seen.iter().filter(|t| t.is_buy && t.slot == create_slot).collect();
    r.creator_same_slot_buy = slot_buys.iter().any(|t| creator_ids.contains(&t.user));
    let others: Vec<&&&Trade> = slot_buys.iter().filter(|t| !creator_ids.contains(&t.user)).collect();
    r.same_slot_wallets = others.iter().map(|t| t.user.as_str()).collect::<HashSet<_>>().len();
    r.same_slot_pct = pct(others.iter().map(|t| t.token_amount).sum());

    let mut sides: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for t in seen.iter().filter(|t| !creator_ids.contains(&t.user)) {
        let s = sides.entry(&t.user).or_default();
        if t.is_buy {
            s.0 += 1;
        } else {
            s.1 += 1;
        }
    }
    r.pairs = sides.values().filter(|&&s| s == (1, 1)).count();
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    const V0: u64 = 1000;

    /// Trade con el precio indicado como múltiplo del inicial (quote = m·V0,
    /// token = V0).
    fn t(user: &str, is_buy: bool, m: f64, slot: u64, ts: i64) -> Trade {
        Trade {
            user: user.into(),
            is_buy,
            token_amount: 10,
            slot,
            timestamp: ts,
            quote_reserves: (m * V0 as f64) as u64,
            token_reserves: V0,
            fee_bps: None,
        }
    }

    fn ids(xs: &[&str]) -> HashSet<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    fn run(trades: &[Trade], creators: &[&str]) -> Option<Indicators> {
        compute(0, 7, (V0, V0), trades, &ids(creators), &PARAMS)
    }

    #[test]
    fn el_orden_de_ejecucion_sale_del_encadenado_de_reservas() {
        let mk = |user: &str, is_buy, token_amount, token_reserves| Trade {
            user: String::from(user),
            is_buy,
            token_amount,
            slot: 7,
            timestamp: 0,
            quote_reserves: 0,
            token_reserves,
            fee_bps: None,
        };
        // Ejecución real: a compra 100 (1000→900), c vende 30 (900→930),
        // b compra 50 (930→880). Almacenados en otro orden.
        let v = [mk("b", true, 50, 880), mk("c", false, 30, 930), mk("a", true, 100, 900)];
        let refs: Vec<&Trade> = v.iter().collect();
        let (o, ok) = execution_order(1000, &refs);
        assert!(ok);
        assert_eq!(o.iter().map(|t| t.user.as_str()).collect::<Vec<_>>(), ["a", "c", "b"]);
        let (o, ok) = execution_order(999, &refs);
        assert!(!ok, "sin enlace con el inicial no se inventa orden");
        assert_eq!(o.len(), 3);
    }

    #[test]
    fn sin_llegar_a_1_5x_no_hay_entrada() {
        assert!(run(&[t("a", true, 1.4, 7, 0), t("a", false, 1.2, 9, 30)], &[]).is_none());
        // 1.6× pero fuera de la ventana de 5 min.
        assert!(run(&[t("a", true, 1.6, 9, 300)], &[]).is_none());
    }

    #[test]
    fn solo_cuentan_los_trades_hasta_la_entrada() {
        let v = [
            t("dev", true, 1.1, 7, 0),
            t("a", true, 1.2, 8, 2),
            t("a", false, 1.1, 9, 4),
            t("b", true, 1.6, 10, 12), // entrada
            t("dev", false, 1.3, 11, 13),
            t("b", false, 1.2, 12, 14),
        ];
        let r = run(&v, &["dev"]).unwrap();
        assert_eq!(r.entry_secs, 12);
        assert_eq!(r.trades_seen, 4);
        assert!(!r.dev_sold, "la venta del dev es posterior a la entrada");
        assert_eq!(r.pairs, 1, "b solo tiene la compra antes de entrar");
        assert!(r.creator_same_slot_buy);
        assert_eq!(r.same_slot_wallets, 0);
        assert_eq!(r.entry_real_quote, 600);
    }

    #[test]
    fn venta_del_dev_y_compras_ajenas_en_el_slot_de_creacion() {
        let v = [
            t("dev", true, 1.1, 7, 0),
            t("x", true, 1.2, 7, 0),
            t("y", true, 1.3, 7, 0),
            t("dev", false, 1.2, 8, 3),
            t("z", true, 1.5, 9, 5),
        ];
        let r = run(&v, &["dev"]).unwrap();
        assert!(r.h4());
        assert_eq!(r.dev_first_sell_secs, Some(3));
        assert!((r.dev_sold_pct - 1e-12).abs() < 1e-15);
        assert!(r.h5());
        assert_eq!(r.same_slot_wallets, 2);
        assert!(r.h6(&PARAMS));
        assert!(!r.h7(&PARAMS));
    }
}
