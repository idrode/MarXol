//! Variables de la fase C (2026-10-22, exploratorio y post hoc): tenencia del
//! creador y tx con compras de varias cuentas, solo con lo anterior a T_entry2.
//! Definiciones: docs/fase_c_declaracion_2026-10-22.md. Cálculo puro, sin RPC.

use crate::entry::Trade;
use std::collections::HashSet;

/// C1: tenencia del creador en T_entry2 por encima de este % del supply.
pub const UMBRAL_CREADOR_PCT: f64 = 5.0;
/// Supply estándar (fuera de mayhem).
pub const SUPPLY: u64 = 1_000_000_000_000_000;

/// Índices de `trades` en orden de ejecución: `entry::execution_order` sobre
/// índices (lo que no encadena va al final en orden de almacenamiento).
pub fn execution_order_idx(v0: u64, trades: &[&Trade]) -> Vec<usize> {
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
        out.push(i);
    }
    out.extend((0..trades.len()).filter(|&i| !used[i]));
    out
}

/// Partición de `entry2::compute`: (índices en `trades` de los trades
/// anteriores a T_entry2 en orden de ejecución, índice del trade T_entry2).
/// `None` si no hay T_entry2 en la ventana.
pub fn antes_de_entry2(
    t0: i64,
    create_slot: u64,
    v0: u64,
    trades: &[Trade],
    window_secs: i64,
    min_delay_secs: i64,
) -> Option<(Vec<usize>, usize)> {
    let window: Vec<usize> = (0..trades.len()).filter(|&i| (t0..t0 + window_secs).contains(&trades[i].timestamp)).collect();
    let refs: Vec<&Trade> = window.iter().map(|&i| &trades[i]).collect();
    let ordered: Vec<usize> = execution_order_idx(v0, &refs).into_iter().map(|j| window[j]).collect();
    let k = ordered.iter().position(|&i| trades[i].slot > create_slot && trades[i].timestamp >= t0 + min_delay_secs)?;
    Some((ordered[..k].to_vec(), ordered[k]))
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Vars {
    /// Trades anteriores a T_entry2.
    pub trades_antes: usize,
    /// Tenencia de las identidades de creador en T_entry2 (% del supply, ≥ 0).
    pub creador_pct: f64,
    /// Tx anteriores a T_entry2 con compras de ≥ 2 cuentas distintas.
    pub tx_varias: usize,
}

impl Vars {
    pub fn c1(&self) -> bool {
        self.creador_pct > UMBRAL_CREADOR_PCT
    }
    pub fn c2(&self) -> bool {
        self.tx_varias >= 1
    }
}

/// `trades`, `tx` (firma, creator embebido) alineados; `antes` = índices de
/// `antes_de_entry2`; `ids_base` = creator, payer y cambios anteriores a
/// T_entry2. Se añade el creator embebido de los trades anteriores.
pub fn variables(trades: &[Trade], tx: &[(String, Option<String>)], antes: &[usize], ids_base: &HashSet<String>) -> Vars {
    let mut ids: HashSet<&str> = ids_base.iter().map(String::as_str).collect();
    ids.extend(antes.iter().filter_map(|&i| tx[i].1.as_deref()));
    let neto: i128 = antes
        .iter()
        .filter(|&&i| ids.contains(trades[i].user.as_str()))
        .map(|&i| if trades[i].is_buy { trades[i].token_amount as i128 } else { -(trades[i].token_amount as i128) })
        .sum();
    let mut compradores: std::collections::BTreeMap<&str, HashSet<&str>> = Default::default();
    for &i in antes.iter().filter(|&&i| trades[i].is_buy) {
        compradores.entry(tx[i].0.as_str()).or_default().insert(trades[i].user.as_str());
    }
    Vars {
        trades_antes: antes.len(),
        creador_pct: 100.0 * neto.max(0) as f64 / SUPPLY as f64,
        tx_varias: compradores.values().filter(|u| u.len() >= 2).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(user: &str, is_buy: bool, amount: u64, slot: u64, ts: i64, after: u64) -> Trade {
        Trade {
            user: user.into(),
            is_buy,
            token_amount: amount,
            slot,
            timestamp: ts,
            quote_reserves: 0,
            token_reserves: after,
            fee_bps: Some(125),
        }
    }

    #[test]
    fn orden_y_variables() {
        let v0 = 1_000;
        // Almacenados fuera de orden dentro del slot 10: "b" se ejecuta antes que "dev".
        let trades = vec![
            tr("dev", true, 100, 10, 0, 850), // 950 → 850
            tr("b", true, 50, 10, 0, 950),    // 1000 → 950
            tr("c", true, 10, 10, 0, 840),    // 850 → 840 (misma tx que "dev")
            tr("dev", false, 20, 11, 5, 860), // 840 → 860
            tr("x", true, 5, 12, 12, 855),    // T_entry2: slot > 10 y t ≥ 10
        ];
        let refs: Vec<&Trade> = trades.iter().collect();
        let (orden, _) = crate::entry::execution_order(v0, &refs);
        let idx = execution_order_idx(v0, &refs);
        assert_eq!(idx, vec![1, 0, 2, 3, 4]);
        assert!(idx.iter().zip(&orden).all(|(&i, t)| std::ptr::eq(&trades[i], *t)));

        let (antes, entry) = antes_de_entry2(0, 10, v0, &trades, 300, 10).unwrap();
        assert_eq!((antes.clone(), entry), (vec![1, 0, 2, 3], 4));
        let tx: Vec<(String, Option<String>)> = ["s1", "s0", "s1", "s2", "s3"].iter().map(|s| (s.to_string(), None)).collect();
        let ids: HashSet<String> = ["dev".to_string()].into();
        let v = variables(&trades, &tx, &antes, &ids);
        assert_eq!(v.trades_antes, 4);
        assert!((v.creador_pct - 100.0 * 80.0 / SUPPLY as f64).abs() < 1e-18);
        assert_eq!(v.tx_varias, 1);
        assert!(!v.c1() && v.c2());
    }

    #[test]
    fn creator_embebido_cuenta_solo_antes() {
        let trades = vec![tr("a", true, 10, 5, 0, 990), tr("y", true, 1, 6, 20, 989)];
        let tx = vec![("s0".to_string(), Some("a".to_string())), ("s1".to_string(), Some("y".to_string()))];
        let (antes, _) = antes_de_entry2(0, 5, 1_000, &trades, 300, 10).unwrap();
        let v = variables(&trades, &tx, &antes, &HashSet::new());
        assert!(v.creador_pct > 0.0);
    }
}
