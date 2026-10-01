//! Validación 2, criterio de réplica (CLAUDE.md sección 8, pre-registro del
//! 2026-09-30, commit 4de934b). Cálculo puro: poblaciones, tablas 2×2,
//! veredicto, bootstrap y métricas de filtro. No toca RPC ni la base.
//!
//! El bootstrap usa un Mersenne Twister sembrado igual que `random.Random`
//! de Python, para reproducir exactamente los intervalos de
//! `scripts/robustez_posthoc.py` con la misma semilla.

use crate::h1b::Table;
use std::collections::BTreeMap;

pub const SEED: u32 = 20260930;
pub const REPLICATES: usize = 2000;
/// Criterio congelado, común a H4'–H8 y H10.
pub const MIN_GROUP: u64 = 3;
pub const MIN_DIFF: f64 = 0.20;
pub const MAX_FISHER_P: f64 = 0.01;
const EPS: f64 = 1e-9;

/// MT19937 con la siembra de `random.seed(int)` de CPython (`init_by_array`
/// con la semilla en palabras de 32 bits) y su `randrange(n)`
/// (`getrandbits(n.bit_length())` con rechazo).
pub struct PyRandom {
    mt: [u32; 624],
    idx: usize,
}

impl PyRandom {
    pub fn new(seed: u32) -> Self {
        let mut mt = [0u32; 624];
        mt[0] = 19_650_218;
        for i in 1..624 {
            mt[i] = 1_812_433_253u32.wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_add(i as u32);
        }
        let (mut i, key) = (1usize, [seed]);
        for k in 0..624usize {
            let j = k % key.len();
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            if i >= 624 {
                mt[0] = mt[623];
                i = 1;
            }
        }
        for _ in 0..623 {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1_566_083_941)).wrapping_sub(i as u32);
            i += 1;
            if i >= 624 {
                mt[0] = mt[623];
                i = 1;
            }
        }
        mt[0] = 0x8000_0000;
        PyRandom { mt, idx: 624 }
    }

    fn next_u32(&mut self) -> u32 {
        const UPPER: u32 = 0x8000_0000;
        const LOWER: u32 = 0x7fff_ffff;
        let mag = |y: u32| if y & 1 == 1 { 0x9908_b0df } else { 0 };
        if self.idx >= 624 {
            for k in 0..624 {
                let y = (self.mt[k] & UPPER) | (self.mt[(k + 1) % 624] & LOWER);
                self.mt[k] = self.mt[(k + 397) % 624] ^ (y >> 1) ^ mag(y);
            }
            self.idx = 0;
        }
        let mut y = self.mt[self.idx];
        self.idx += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `random.randrange(n)` para 1 ≤ n < 2^32.
    pub fn randrange(&mut self, n: usize) -> usize {
        let k = usize::BITS - n.leading_zeros();
        loop {
            let r = (self.next_u32() >> (32 - k)) as usize;
            if r < n {
                return r;
            }
        }
    }
}

/// Un token de las tablas con todo lo que usa el veredicto y el descriptivo.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Tok {
    pub mint: String,
    pub creator: String,
    pub slot: u64,
    pub mayhem: bool,
    pub window_trades: usize,
    /// H4', H5, H6', H7', H8 (`None` = n/e).
    pub sigs: [Option<bool>; 5],
    pub pump_dump: bool,
    /// H10: ≥ 1 trade a más de 10 min de T_entry2; `None` = n/e.
    pub surv: Option<bool>,
    /// Retorno neto a +30 min, versiones (a) y (b).
    pub ret30: [Option<f64>; 2],
}

/// Resultado de una tabla 2×2 señal × resultado.
pub type Outcome = fn(&Tok) -> Option<bool>;
pub fn pump_dump(t: &Tok) -> Option<bool> {
    Some(t.pump_dump)
}
pub fn survival(t: &Tok) -> Option<bool> {
    t.surv
}

pub fn table(pop: &[&Tok], sig: usize, out: Outcome) -> Table {
    let mut t = Table::default();
    for k in pop {
        match (k.sigs[sig], out(k)) {
            (Some(true), Some(true)) => t.a += 1,
            (Some(true), Some(false)) => t.b += 1,
            (Some(false), Some(true)) => t.c += 1,
            (Some(false), Some(false)) => t.d += 1,
            _ => {}
        }
    }
    t
}

fn rate(x: u64, n: u64) -> f64 {
    if n == 0 { f64::NAN } else { x as f64 / n as f64 }
}

/// p₁ − p₀; `None` si algún grupo está vacío (como `diff_pd` en Python).
pub fn diff(t: &Table) -> Option<f64> {
    let (n1, n0) = (t.a + t.b, t.c + t.d);
    (n1 > 0 && n0 > 0).then(|| rate(t.a, n1) - rate(t.c, n0))
}

/// Intervalo bootstrap percentil 2.5–97.5 de `stat`, con una secuencia
/// nueva de la semilla por estadístico (como `ci` en Python). `cluster`:
/// remuestreo de creators con todos sus tokens (claves ordenadas); si no,
/// remuestreo simple de tokens en el orden de `pop`. Devuelve (lo, hi,
/// réplicas descartadas por estadístico no evaluable).
pub fn ci(pop: &[&Tok], cluster: bool, stat: &dyn Fn(&[&Tok]) -> Option<f64>) -> (f64, f64, usize) {
    fn key<'x>(t: &'x &Tok) -> &'x str {
        t.creator.as_str()
    }
    let k: &dyn for<'x> Fn(&'x &Tok) -> &'x str = &key;
    bootstrap(pop, cluster.then_some(k), &|rs: &[&&Tok]| {
        let v: Vec<&Tok> = rs.iter().map(|t| **t).collect();
        stat(&v)
    })
}

/// Bootstrap percentil 2.5–97.5 genérico con la semilla `SEED` (una
/// secuencia nueva por llamada). `cluster`: clave de clúster (se remuestrean
/// claves ordenadas con todos sus elementos); si no, remuestreo simple en el
/// orden de `pop`. Devuelve (lo, hi, réplicas descartadas).
pub fn bootstrap<'a, T>(
    pop: &'a [T],
    cluster: Option<&dyn Fn(&T) -> &str>,
    stat: &dyn Fn(&[&'a T]) -> Option<f64>,
) -> (f64, f64, usize) {
    let mut rng = PyRandom::new(SEED);
    let mut groups: BTreeMap<&str, Vec<&T>> = BTreeMap::new();
    if let Some(key) = cluster {
        for t in pop {
            groups.entry(key(t)).or_default().push(t);
        }
    }
    let groups: Vec<Vec<&T>> = groups.into_values().collect();
    let (mut vals, mut drop) = (Vec::with_capacity(REPLICATES), 0);
    for _ in 0..REPLICATES {
        let rs: Vec<&T> = if cluster.is_some() {
            (0..groups.len()).flat_map(|_| groups[rng.randrange(groups.len())].iter().copied()).collect()
        } else {
            (0..pop.len()).map(|_| &pop[rng.randrange(pop.len())]).collect()
        };
        match stat(&rs) {
            Some(v) if !v.is_nan() => vals.push(v),
            _ => drop += 1,
        }
    }
    if vals.is_empty() {
        return (f64::NAN, f64::NAN, drop);
    }
    vals.sort_by(f64::total_cmp);
    let n = vals.len();
    (vals[(0.025 * n as f64) as usize], vals[((0.975 * n as f64) as usize).min(n - 1)], drop)
}

/// Mediana convencional; `None` si vacía.
pub fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PopResult {
    pub n1: u64,
    pub n0: u64,
    pub table: Table,
    pub diff: Option<f64>,
    pub fisher_p: f64,
    pub or: (f64, f64, f64),
    pub boot: (f64, f64, usize),
}

pub fn pop_result(pop: &[&Tok], sig: usize, out: Outcome, cluster: bool) -> PopResult {
    let t = table(pop, sig, out);
    PopResult {
        n1: t.a + t.b,
        n0: t.c + t.d,
        table: t,
        diff: diff(&t),
        fisher_p: t.fisher_p(),
        or: t.odds_ratio(),
        boot: ci(pop, cluster, &|rs| diff(&table(rs, sig, out))),
    }
}

/// Criterio de confirmación en réplica (congelado). `primary_wo` = diferencia
/// en la primaria sin el token de más trades.
pub fn verdict(primary: &PopResult, primary_wo: Option<f64>, sec: [&PopResult; 2], low_power: bool) -> String {
    let pos = |d: Option<f64>| d.is_some_and(|d| d > EPS);
    let ok = primary.n1 >= MIN_GROUP
        && primary.n0 >= MIN_GROUP
        && primary.diff.is_some_and(|d| d >= MIN_DIFF - EPS)
        && primary.fisher_p < MAX_FISHER_P
        && pos(primary_wo)
        && sec.iter().all(|s| pos(s.diff));
    if ok {
        return "CONFIRMADA EN RÉPLICA".into();
    }
    let mut v = "NO CONFIRMADA EN RÉPLICA".to_string();
    if primary.diff.is_some_and(|d| d < -EPS) {
        v += " (dirección opuesta)";
    }
    if low_power {
        v += " — baja potencia";
    }
    v
}

/// Filtros "evitar" (CLAUDE.md 10): A = cualquiera de H5/H6'/H7'/H8, B = ≥ 2
/// de las cuatro, C = H5 o H7'. Un n/e cuenta como sin señal.
pub fn filter(t: &Tok, f: char) -> bool {
    let core = [1, 2, 3, 4].map(|i| t.sigs[i] == Some(true));
    match f {
        'A' => core.iter().any(|&x| x),
        'B' => core.iter().filter(|&&x| x).count() >= 2,
        'C' => core[0] || core[2],
        _ => unreachable!(),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FilterMetrics {
    pub evaluable: usize,
    pub avoided: usize,
    pub kept: usize,
    pub precision: f64,
    pub coverage: f64,
    pub sacrifice: f64,
}

/// Precisión (malos entre evitados), cobertura (malos evitados) y sacrificio
/// (buenos evitados) sobre los tokens con `bad` evaluable.
pub fn filter_metrics(pop: &[&Tok], f: char, bad: &dyn Fn(&Tok) -> Option<bool>) -> FilterMetrics {
    let ev: Vec<(&Tok, bool)> = pop.iter().filter_map(|t| bad(t).map(|b| (*t, b))).collect();
    let avoid: Vec<bool> = ev.iter().filter(|(t, _)| filter(t, f)).map(|x| x.1).collect();
    let malos = ev.iter().filter(|x| x.1).count() as u64;
    let buenos = ev.len() as u64 - malos;
    let am = avoid.iter().filter(|&&b| b).count() as u64;
    FilterMetrics {
        evaluable: ev.len(),
        avoided: avoid.len(),
        kept: ev.len() - avoid.len(),
        precision: rate(am, avoid.len() as u64),
        coverage: rate(am, malos),
        sacrifice: rate(avoid.len() as u64 - am, buenos),
    }
}

/// Mediana del retorno a +30 min (versión `v`) de los no evitados sin mayhem.
pub fn rest_return(rs: &[&Tok], f: char, v: usize) -> Option<f64> {
    let mut x: Vec<f64> = rs.iter().filter(|t| !t.mayhem && !filter(t, f)).filter_map(|t| t.ret30[v]).collect();
    median(&mut x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn randrange_coincide_con_python() {
        // python3: r = random.Random(20260930); [r.randrange(210) for _ in range(8)]
        let mut r = PyRandom::new(SEED);
        let v: Vec<usize> = (0..8).map(|_| r.randrange(210)).collect();
        assert_eq!(v, [118, 103, 161, 186, 56, 172, 78, 163]);
        // r = random.Random(20260930); [r.randrange(1) ×2], [r.randrange(2**32-5) ×3], [r.randrange(7) ×5]
        let mut r = PyRandom::new(SEED);
        assert_eq!([r.randrange(1), r.randrange(1)], [0, 0]);
        let big: Vec<usize> = (0..3).map(|_| r.randrange((1usize << 32) - 5)).collect();
        assert_eq!(big, [2708134837, 3134153672, 954878811]);
        let s: Vec<usize> = (0..5).map(|_| r.randrange(7)).collect();
        assert_eq!(s, [5, 2, 5, 4, 5]);
    }

    fn tok(creator: &str, sig: bool, pd: bool) -> Tok {
        Tok {
            mint: format!("{creator}{sig}{pd}"),
            creator: creator.into(),
            slot: 0,
            mayhem: false,
            window_trades: 0,
            sigs: [Some(sig); 5],
            pump_dump: pd,
            surv: None,
            ret30: [None, None],
        }
    }

    #[test]
    fn veredicto_exige_todas_las_condiciones() {
        let mk = |a, b, c, d| {
            let mut v = Vec::new();
            for (n, s, p) in [(a, true, true), (b, true, false), (c, false, true), (d, false, false)] {
                for i in 0..n {
                    v.push(tok(&format!("c{s}{p}{i}"), s, p));
                }
            }
            v
        };
        let toks = mk(8, 2, 2, 8); // 80 % vs 20 %, Fisher p ≈ 0.023
        let pop: Vec<&Tok> = toks.iter().collect();
        let r = pop_result(&pop, 1, pump_dump, false);
        assert!((r.diff.unwrap() - 0.6).abs() < 1e-12);
        assert!(r.fisher_p > 0.01);
        assert!(verdict(&r, Some(0.5), [&r, &r], false).starts_with("NO CONFIRMADA"));
        let toks = mk(30, 5, 5, 30);
        let pop: Vec<&Tok> = toks.iter().collect();
        let r = pop_result(&pop, 1, pump_dump, false);
        assert_eq!(verdict(&r, Some(0.5), [&r, &r], false), "CONFIRMADA EN RÉPLICA");
        assert!(verdict(&r, Some(0.0), [&r, &r], false).starts_with("NO CONFIRMADA"));
        let opp = pop_result(&pop.iter().map(|t| *t).filter(|_| true).collect::<Vec<_>>(), 1, |t| Some(!t.pump_dump), false);
        assert_eq!(verdict(&opp, Some(-0.5), [&r, &r], true), "NO CONFIRMADA EN RÉPLICA (dirección opuesta) — baja potencia");
        assert!(verdict(&r, Some(0.5), [&r, &opp], false).starts_with("NO CONFIRMADA"));
    }

    #[test]
    fn bootstrap_por_cluster_remuestrea_creators_enteros() {
        // Un creator con dos tokens y otro con uno: cada réplica tiene 2, 3 o 4 tokens.
        let toks = [tok("x", true, true), tok("x", false, false), tok("y", true, false)];
        let pop: Vec<&Tok> = toks.iter().collect();
        let (lo, hi, _) = ci(&pop, true, &|rs| Some(rs.len() as f64));
        assert!(lo >= 2.0 && hi <= 4.0);
        let (lo, hi, drop) = ci(&pop, false, &|rs| Some(rs.len() as f64));
        assert_eq!((lo, hi, drop), (3.0, 3.0, 0));
    }

    #[test]
    fn filtros_y_metricas() {
        let mut t = tok("a", false, true);
        t.sigs = [Some(true), Some(true), None, Some(false), Some(false)];
        assert!(filter(&t, 'A') && !filter(&t, 'B') && filter(&t, 'C'));
        t.sigs[2] = Some(true);
        assert!(filter(&t, 'B'));
        let u = tok("b", false, false);
        let pop = [&t, &u];
        let m = filter_metrics(&pop, 'A', &|t| Some(t.pump_dump));
        assert_eq!((m.evaluable, m.avoided, m.kept), (2, 1, 1));
        assert_eq!((m.precision, m.coverage, m.sacrifice), (1.0, 1.0, 0.0));
    }
}
