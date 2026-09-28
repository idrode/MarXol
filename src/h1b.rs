//! Validación retrospectiva H1b (CLAUDE.md sección 8): graduación y "pump
//! sostenido" en la ventana N desde la creación, y el test exacto de Fisher
//! para H1 × H1b. Cálculo puro sobre precios ya almacenados; no toca RPC.

#[derive(Debug, Clone, serde::Serialize)]
pub struct Params {
    /// Ventana N desde `CreateEvent` [P].
    pub horizon_secs: i64,
    /// Caída máxima permitida desde el pico al cierre [P].
    pub max_drawdown: f64,
    /// El pico debe ser al menos este múltiplo del precio inicial [P].
    pub min_peak_multiple: f64,
}

pub const PARAMS_1H: Params = Params { horizon_secs: 3600, max_drawdown: 0.30, min_peak_multiple: 2.0 };

/// Precio en la bonding curve: reservas virtuales de quote / de token.
/// mcap = precio × supply, y la supply es fija, así que los ratios de mcap
/// son ratios de precio.
#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub timestamp: i64,
    pub quote_reserves: u64,
    pub token_reserves: u64,
}

impl Point {
    fn price(&self) -> f64 {
        self.quote_reserves as f64 / self.token_reserves as f64
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct H1bResult {
    pub graduated: bool,
    /// `None` si no es evaluable: la curva se completó dentro de la ventana
    /// (el precio sigue en PumpSwap, que no se descarga todavía).
    pub sustained: Option<bool>,
    pub peak_multiple: f64,
    /// Caída del cierre respecto al pico (0 = cierra en el pico).
    pub drawdown: f64,
    pub points: usize,
}

/// `initial` es el precio del `CreateEvent`; `points`, los trades de la
/// ventana en cualquier orden.
pub fn compute(
    t0: i64,
    initial: Point,
    points: &[Point],
    completed_at: Option<i64>,
    migrated_at: Option<i64>,
    p: &Params,
) -> H1bResult {
    let end = t0 + p.horizon_secs;
    let in_window = |t: i64| (t0..end).contains(&t);
    let mut pts: Vec<&Point> = points.iter().filter(|x| in_window(x.timestamp)).collect();
    pts.sort_by_key(|x| x.timestamp);
    let p0 = initial.price();
    let peak = pts.iter().map(|x| x.price()).fold(p0, f64::max);
    let close = pts.last().map_or(p0, |x| x.price());
    let peak_multiple = peak / p0;
    let drawdown = 1.0 - close / peak;
    let sustained = (!completed_at.is_some_and(in_window))
        .then(|| peak_multiple >= p.min_peak_multiple && drawdown <= p.max_drawdown);
    H1bResult {
        graduated: migrated_at.is_some_and(in_window),
        sustained,
        peak_multiple,
        drawdown,
        points: pts.len(),
    }
}

/// Tabla 2×2: `a` = H1 ∧ éxito, `b` = H1 ∧ ¬éxito, `c` = ¬H1 ∧ éxito, `d` = ¬H1 ∧ ¬éxito.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct Table {
    pub a: u64,
    pub b: u64,
    pub c: u64,
    pub d: u64,
}

fn ln_fact(n: u64) -> f64 {
    (2..=n).map(|k| (k as f64).ln()).sum()
}

impl Table {
    /// p-valor bilateral del test exacto de Fisher: suma de las
    /// probabilidades hipergeométricas de todas las tablas con los mismos
    /// márgenes que no son más probables que la observada.
    pub fn fisher_p(&self) -> f64 {
        let (r1, r2, c1) = (self.a + self.b, self.c + self.d, self.a + self.c);
        let n = r1 + r2;
        let ln_p = |a: u64| {
            let (b, c) = (r1 - a, c1 - a);
            let d = r2 - c;
            ln_fact(r1) + ln_fact(r2) + ln_fact(c1) + ln_fact(n - c1)
                - ln_fact(n)
                - ln_fact(a)
                - ln_fact(b)
                - ln_fact(c)
                - ln_fact(d)
        };
        let lo = c1.saturating_sub(r2);
        let hi = r1.min(c1);
        let obs = ln_p(self.a);
        (lo..=hi)
            .map(ln_p)
            .filter(|&l| l <= obs + 1e-7)
            .map(f64::exp)
            .sum::<f64>()
            .min(1.0)
    }

    /// Odds ratio con IC 95 % de Woolf; con algún cero se suma 0.5 a cada celda.
    pub fn odds_ratio(&self) -> (f64, f64, f64) {
        let z = [self.a, self.b, self.c, self.d].contains(&0);
        let k = if z { 0.5 } else { 0.0 };
        let [a, b, c, d] = [self.a, self.b, self.c, self.d].map(|x| x as f64 + k);
        let or = (a * d) / (b * c);
        let se = (1.0 / a + 1.0 / b + 1.0 / c + 1.0 / d).sqrt();
        (or, (or.ln() - 1.96 * se).exp(), (or.ln() + 1.96 * se).exp())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(timestamp: i64, q: u64) -> Point {
        Point { timestamp, quote_reserves: q, token_reserves: 1000 }
    }

    #[test]
    fn pump_que_aguanta_es_sostenido() {
        let r = compute(0, pt(0, 100), &[pt(10, 250), pt(50, 300), pt(3000, 240)], None, None, &PARAMS_1H);
        assert!((r.peak_multiple - 3.0).abs() < 1e-9);
        assert!((r.drawdown - 0.2).abs() < 1e-9);
        assert_eq!(r.sustained, Some(true));
    }

    #[test]
    fn pump_que_se_desploma_antes_del_cierre_no_es_sostenido() {
        let r = compute(0, pt(0, 100), &[pt(10, 300), pt(3500, 150)], None, None, &PARAMS_1H);
        assert_eq!(r.sustained, Some(false));
    }

    #[test]
    fn token_plano_no_es_sostenido() {
        let r = compute(0, pt(0, 100), &[pt(10, 110), pt(3000, 105)], None, None, &PARAMS_1H);
        assert_eq!(r.sustained, Some(false), "sin pico ≥ 2× no hay pump");
    }

    #[test]
    fn trades_fuera_de_la_ventana_no_cuentan() {
        let r = compute(0, pt(0, 100), &[pt(10, 250), pt(3600, 50)], None, None, &PARAMS_1H);
        assert_eq!(r.points, 1);
        assert_eq!(r.sustained, Some(true));
    }

    #[test]
    fn graduado_en_ventana_cuenta_y_sostenido_no_es_evaluable() {
        let r = compute(0, pt(0, 100), &[pt(10, 900)], Some(20), Some(40), &PARAMS_1H);
        assert!(r.graduated);
        assert_eq!(r.sustained, None);
        let tarde = compute(0, pt(0, 100), &[pt(10, 900)], Some(4000), Some(4100), &PARAMS_1H);
        assert!(!tarde.graduated);
    }

    #[test]
    fn fisher_coincide_con_valores_de_referencia() {
        // Tabla clásica de "lady tasting tea": p bilateral = 0.4857.
        let t = Table { a: 3, b: 1, c: 1, d: 3 };
        assert!((t.fisher_p() - 0.4857).abs() < 1e-3, "{}", t.fisher_p());
        // Separación perfecta 5/0 vs 0/5: p = 2/252 ≈ 0.00794.
        let t = Table { a: 5, b: 0, c: 0, d: 5 };
        assert!((t.fisher_p() - 0.00794).abs() < 1e-4, "{}", t.fisher_p());
    }
}
