//! Banco de evaluación de reglas de veto y métricas (fase B, 2026-10-22).
//! Cálculo puro, sin RPC ni base: recibe los casos ya construidos y devuelve
//! un informe con lo mínimo para no engañarse con una regla:
//!
//! 1. el modelo trivial (predecir siempre la clase mayoritaria) junto a la regla;
//! 2. MCC, prevalencia, precisión y recall por clase, AUPRC con su línea base
//!    (la prevalencia) y n;
//! 3. un intervalo de confianza en cada porcentaje (Wilson) y bootstrap para
//!    MCC, AUPRC y la diferencia de medianas;
//! 4. fragilidad: el resultado sin los 1, 3 y 5 casos más extremos;
//! 5. censura: cuántos casos completan cada horizonte y por qué no los demás,
//!    y cuántos quedan fuera de la población y por qué;
//! 6. partición temporal por bloques fijados por quien llama; nunca aleatoria.
//!
//! No decide nada: los veredictos siguen en su módulo congelado (`v3`) o en
//! la adenda que los fije. La clase positiva y la dirección de la regla las
//! fija quien llama antes de mirar el resultado.

use crate::entry2::mann_whitney_greater;
use crate::replica::{bootstrap, median};
use std::fmt::{self, Write as _};

/// Casos que se quitan en la tabla de fragilidad.
pub const FRAGILIDAD_K: [usize; 3] = [1, 3, 5];
/// Horizontes de la tabla de censura (s): +5 min, +30 min, +1 h, +24 h.
pub const HORIZONTES_CENSURA: [(i64, &str); 4] = [(300, "+5 min"), (1800, "+30 min"), (3600, "+1 h"), (86_400, "+24 h")];

/// Un caso evaluable: un token con regla y resultado primario conocidos.
#[derive(Debug, Clone)]
pub struct Caso {
    pub id: String,
    /// Clave de clúster para el bootstrap (creator).
    pub cluster: String,
    /// Tiempo del caso para la partición temporal (timestamp de creación).
    pub t: i64,
    /// La regla marca el caso (veto).
    pub marca: bool,
    /// Puntuación para el AUPRC: mayor = más probable de la clase positiva.
    /// Para una regla binaria, 1.0 si marca y 0.0 si no.
    pub score: f64,
    /// Resultado primario (por ejemplo, el retorno neto (a) a +30 min).
    pub resultado: f64,
}

/// Remuestreo del bootstrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remuestreo {
    /// Casos uno a uno, en el orden de entrada (como la validación 3).
    Casos,
    /// Clústeres con todos sus casos (claves ordenadas).
    Cluster,
}

/// Lo que fija quien llama, antes de ver el resultado.
pub struct Spec<'a> {
    pub titulo: &'a str,
    /// Nombre de la clase positiva (la que la regla quiere vetar).
    pub positivo: &'a str,
    /// Un resultado es de la clase positiva.
    pub es_positivo: &'a dyn Fn(f64) -> bool,
    /// Unidad y escala del resultado al imprimirlo (por ejemplo, "pts" y 100).
    pub unidad: &'a str,
    pub escala: f64,
    pub remuestreo: Remuestreo,
    /// Bloques temporales [desde, hasta) con nombre, en orden.
    pub bloques: Vec<(String, i64, i64)>,
}

/// Cuántos casos completan un horizonte y por qué no los demás.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Censura {
    pub horizonte: String,
    pub completan: usize,
    /// (motivo, casos), en orden de aparición.
    pub motivos: Vec<(String, usize)>,
}

impl Censura {
    pub fn nuevo(horizonte: &str) -> Self {
        Censura { horizonte: horizonte.into(), ..Default::default() }
    }
    /// Anota un caso: `None` si completa el horizonte, o el motivo.
    pub fn anota(&mut self, motivo: Option<&str>) {
        match motivo {
            None => self.completan += 1,
            Some(m) => match self.motivos.iter_mut().find(|x| x.0 == m) {
                Some(x) => x.1 += 1,
                None => self.motivos.push((m.into(), 1)),
            },
        }
    }
    pub fn total(&self) -> usize {
        self.completan + self.motivos.iter().map(|x| x.1).sum::<usize>()
    }
}

/// Proporción con su IC de Wilson al 95 %.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prop {
    pub k: usize,
    pub n: usize,
}

impl Prop {
    pub fn valor(&self) -> Option<f64> {
        (self.n > 0).then(|| self.k as f64 / self.n as f64)
    }
    pub fn wilson(&self) -> Option<(f64, f64)> {
        wilson(self.k, self.n)
    }
}

impl fmt::Display for Prop {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match (self.valor(), self.wilson()) {
            (Some(p), Some((lo, hi))) => {
                write!(f, "{}/{} = {:.1} % [IC 95 % {:.1}–{:.1}]", self.k, self.n, 100.0 * p, 100.0 * lo, 100.0 * hi)
            }
            _ => write!(f, "{}/{} = n/d", self.k, self.n),
        }
    }
}

/// IC de Wilson al 95 % de k/n; `None` si n = 0.
pub fn wilson(k: usize, n: usize) -> Option<(f64, f64)> {
    if n == 0 {
        return None;
    }
    let z = 1.959_963_984_540_054_f64;
    let (nf, p) = (n as f64, k as f64 / n as f64);
    let d = 1.0 + z * z / nf;
    let c = (p + z * z / (2.0 * nf)) / d;
    let h = z * (p * (1.0 - p) / nf + z * z / (4.0 * nf * nf)).sqrt() / d;
    Some(((c - h).max(0.0), (c + h).min(1.0)))
}

/// Matriz de confusión (positivo = clase que la regla quiere vetar).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Confusion {
    pub vp: usize,
    pub fp: usize,
    pub fn_: usize,
    pub vn: usize,
}

impl Confusion {
    pub fn de(pred: impl Iterator<Item = (bool, bool)>) -> Self {
        let mut c = Confusion::default();
        for (marca, pos) in pred {
            match (marca, pos) {
                (true, true) => c.vp += 1,
                (true, false) => c.fp += 1,
                (false, true) => c.fn_ += 1,
                (false, false) => c.vn += 1,
            }
        }
        c
    }
    pub fn n(&self) -> usize {
        self.vp + self.fp + self.fn_ + self.vn
    }
    pub fn prevalencia(&self) -> Prop {
        Prop { k: self.vp + self.fn_, n: self.n() }
    }
    pub fn marcados(&self) -> Prop {
        Prop { k: self.vp + self.fp, n: self.n() }
    }
    pub fn exactitud(&self) -> Prop {
        Prop { k: self.vp + self.vn, n: self.n() }
    }
    pub fn precision_pos(&self) -> Prop {
        Prop { k: self.vp, n: self.vp + self.fp }
    }
    pub fn recall_pos(&self) -> Prop {
        Prop { k: self.vp, n: self.vp + self.fn_ }
    }
    pub fn precision_neg(&self) -> Prop {
        Prop { k: self.vn, n: self.vn + self.fn_ }
    }
    pub fn recall_neg(&self) -> Prop {
        Prop { k: self.vn, n: self.vn + self.fp }
    }
    /// Coeficiente de Matthews; `None` si alguna fila o columna está vacía
    /// (predicción o clase constante: indefinido).
    pub fn mcc(&self) -> Option<f64> {
        let (vp, fp, fn_, vn) = (self.vp as f64, self.fp as f64, self.fn_ as f64, self.vn as f64);
        let d = (vp + fp) * (vp + fn_) * (vn + fp) * (vn + fn_);
        (d > 0.0).then(|| (vp * vn - fp * fn_) / d.sqrt())
    }
}

/// Precisión media (AUPRC escalonado): suma de (ΔR · P) recorriendo los
/// umbrales de la puntuación de mayor a menor; los empates forman un único
/// umbral. `None` si no hay positivos.
pub fn auprc(casos: &[(f64, bool)]) -> Option<f64> {
    let pos = casos.iter().filter(|c| c.1).count();
    if pos == 0 {
        return None;
    }
    let mut v: Vec<&(f64, bool)> = casos.iter().collect();
    v.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (mut ap, mut vp, mut vistos, mut r_prev, mut i) = (0.0, 0, 0, 0.0, 0);
    while i < v.len() {
        let mut j = i;
        while j + 1 < v.len() && v[j + 1].0 == v[i].0 {
            j += 1;
        }
        vp += v[i..=j].iter().filter(|c| c.1).count();
        vistos += j - i + 1;
        let r = vp as f64 / pos as f64;
        ap += (r - r_prev) * vp as f64 / vistos as f64;
        r_prev = r;
        i = j + 1;
    }
    Some(ap)
}

/// Resultado de la regla sobre un conjunto de casos.
#[derive(Debug, Clone, PartialEq)]
pub struct Medida {
    pub conf: Confusion,
    pub mcc: Option<f64>,
    pub auprc: Option<f64>,
    pub mediana_sin: Option<f64>,
    pub mediana_con: Option<f64>,
    /// Mediana de los no marcados − mediana de los marcados.
    pub dif: Option<f64>,
    /// Mann-Whitney unilateral, no marcados > marcados.
    pub mw_p: Option<f64>,
}

pub fn medir(casos: &[&Caso], es_positivo: &dyn Fn(f64) -> bool) -> Medida {
    let conf = Confusion::de(casos.iter().map(|c| (c.marca, es_positivo(c.resultado))));
    let sin: Vec<f64> = casos.iter().filter(|c| !c.marca).map(|c| c.resultado).collect();
    let con: Vec<f64> = casos.iter().filter(|c| c.marca).map(|c| c.resultado).collect();
    let (ms, mc) = (median(&mut sin.clone()), median(&mut con.clone()));
    Medida {
        conf,
        mcc: conf.mcc(),
        auprc: auprc(&casos.iter().map(|c| (c.score, es_positivo(c.resultado))).collect::<Vec<_>>()),
        mediana_sin: ms,
        mediana_con: mc,
        dif: ms.zip(mc).map(|(a, b)| a - b),
        mw_p: mann_whitney_greater(&sin, &con).map(|m| m.1),
    }
}

/// Fragilidad: quitar casos y volver a medir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quitar {
    /// Los k casos con mayor |resultado| (ambos grupos juntos); empate por id.
    Extremos,
    /// Uno a uno, el caso cuya salida más reduce la diferencia de medianas
    /// (el que más sostiene el efecto); empate por id.
    AFavor,
}

/// Índices (en `casos`) de los `k` casos que se quitan.
pub fn a_quitar(casos: &[&Caso], k: usize, modo: Quitar, es_positivo: &dyn Fn(f64) -> bool) -> Vec<usize> {
    match modo {
        Quitar::Extremos => {
            let mut idx: Vec<usize> = (0..casos.len()).collect();
            idx.sort_by(|&a, &b| {
                casos[b].resultado.abs().total_cmp(&casos[a].resultado.abs()).then(casos[a].id.cmp(&casos[b].id))
            });
            idx.truncate(k);
            idx
        }
        Quitar::AFavor => {
            let mut fuera: Vec<usize> = Vec::new();
            for _ in 0..k.min(casos.len()) {
                let mejor = (0..casos.len())
                    .filter(|i| !fuera.contains(i))
                    .filter_map(|i| {
                        let resto: Vec<&Caso> = (0..casos.len())
                            .filter(|j| *j != i && !fuera.contains(j))
                            .map(|j| casos[j])
                            .collect();
                        medir(&resto, es_positivo).dif.map(|d| (d, i))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0).then(casos[a.1].id.cmp(&casos[b.1].id)));
                match mejor {
                    Some((_, i)) => fuera.push(i),
                    None => break,
                }
            }
            fuera
        }
    }
}

#[derive(Debug, Clone)]
pub struct Bloque {
    pub nombre: String,
    pub desde: i64,
    pub hasta: i64,
    pub medida: Medida,
}

#[derive(Debug, Clone)]
pub struct Informe {
    pub n: usize,
    pub medida: Medida,
    /// Clase mayoritaria (true = positiva) y su confusión.
    pub trivial_positiva: bool,
    pub trivial: Confusion,
    /// IC bootstrap (lo, hi, réplicas descartadas).
    pub ci_mcc: (f64, f64, usize),
    pub ci_auprc: (f64, f64, usize),
    pub ci_dif: (f64, f64, usize),
    /// (modo, k, ids quitados, medida).
    pub fragilidad: Vec<(Quitar, usize, Vec<String>, Medida)>,
    pub bloques: Vec<Bloque>,
    pub fuera_de_bloques: usize,
}

pub fn evaluar(casos: &[Caso], spec: &Spec) -> Informe {
    let todos: Vec<&Caso> = casos.iter().collect();
    let medida = medir(&todos, spec.es_positivo);
    let prev = medida.conf.prevalencia();
    let trivial_positiva = 2 * prev.k >= prev.n;
    let trivial = Confusion::de(casos.iter().map(|c| (trivial_positiva, (spec.es_positivo)(c.resultado))));
    fn key(c: &Caso) -> &str {
        c.cluster.as_str()
    }
    let k: &dyn Fn(&Caso) -> &str = &key;
    let cluster = (spec.remuestreo == Remuestreo::Cluster).then_some(k);
    let boot = |f: &dyn Fn(&Medida) -> Option<f64>| {
        bootstrap(casos, cluster, &|rs: &[&Caso]| f(&medir(rs, spec.es_positivo)))
    };
    let mut fragilidad = Vec::new();
    for modo in [Quitar::Extremos, Quitar::AFavor] {
        for k in FRAGILIDAD_K {
            let fuera = a_quitar(&todos, k, modo, spec.es_positivo);
            let resto: Vec<&Caso> = (0..todos.len()).filter(|i| !fuera.contains(i)).map(|i| todos[i]).collect();
            let ids = fuera.iter().map(|&i| todos[i].id.clone()).collect();
            fragilidad.push((modo, k, ids, medir(&resto, spec.es_positivo)));
        }
    }
    let bloques: Vec<Bloque> = spec
        .bloques
        .iter()
        .map(|(nombre, desde, hasta)| {
            let en: Vec<&Caso> = casos.iter().filter(|c| (*desde..*hasta).contains(&c.t)).collect();
            Bloque { nombre: nombre.clone(), desde: *desde, hasta: *hasta, medida: medir(&en, spec.es_positivo) }
        })
        .collect();
    let fuera_de_bloques =
        casos.iter().filter(|c| !spec.bloques.iter().any(|(_, d, h)| (*d..*h).contains(&c.t))).count();
    Informe {
        n: casos.len(),
        ci_mcc: boot(&|m| m.mcc),
        ci_auprc: boot(&|m| m.auprc),
        ci_dif: boot(&|m| m.dif),
        medida,
        trivial_positiva,
        trivial,
        fragilidad,
        bloques,
        fuera_de_bloques,
    }
}

fn num(x: Option<f64>, d: usize) -> String {
    x.map_or("n/d".into(), |v| format!("{v:.d$}"))
}

/// p con 6 decimales, o en notación científica por debajo de 1e-4.
pub fn fmt_p(p: Option<f64>) -> String {
    match p {
        Some(v) if v < 1e-4 => format!("{v:.1e}"),
        Some(v) => format!("{v:.6}"),
        None => "n/d".into(),
    }
}

fn ci(c: (f64, f64, usize), escala: f64, d: usize) -> String {
    if c.0.is_nan() {
        return format!("IC 95 % n/d (réplicas descartadas {})", c.2);
    }
    format!("IC 95 % [{:+.d$}, {:+.d$}] (réplicas descartadas {})", c.0 * escala, c.1 * escala, c.2)
}

impl Informe {
    /// Texto del informe. `excluidos` = (motivo, casos) fuera de la
    /// población antes de evaluar; `censura` = una tabla por horizonte.
    pub fn texto(&self, spec: &Spec, excluidos: &[(String, usize)], censura: &[Censura]) -> String {
        let mut o = String::new();
        let esc = |x: Option<f64>| x.map_or("n/d".into(), |v| format!("{:+.1} {}", v * spec.escala, spec.unidad));
        let m = &self.medida;
        let c = &m.conf;
        let rem = match spec.remuestreo {
            Remuestreo::Casos => "por casos",
            Remuestreo::Cluster => "por clúster",
        };
        let _ = writeln!(o, "==== BANCO DE EVALUACIÓN: {} ====", spec.titulo);
        let _ = writeln!(o, "clase positiva (la que la regla quiere vetar): {}", spec.positivo);
        let _ = writeln!(o, "n = {}; prevalencia {}; la regla marca {}", self.n, c.prevalencia(), c.marcados());
        let _ = writeln!(o, "matriz: VP {} · FP {} · FN {} · VN {}", c.vp, c.fp, c.fn_, c.vn);

        let _ = writeln!(o, "\n-- Regla frente al modelo trivial --");
        let t = &self.trivial;
        let clase = if self.trivial_positiva { "positiva" } else { "negativa" };
        let _ = writeln!(o, "{:28} {:42} trivial (siempre clase {clase})", "", "regla");
        let fila = |o: &mut String, nombre: &str, a: String, b: String| {
            let _ = writeln!(o, "{nombre:28} {a:42} {b}");
        };
        fila(&mut o, "exactitud", c.exactitud().to_string(), t.exactitud().to_string());
        fila(&mut o, "precisión (positiva)", c.precision_pos().to_string(), t.precision_pos().to_string());
        fila(&mut o, "recall (positiva)", c.recall_pos().to_string(), t.recall_pos().to_string());
        fila(&mut o, "precisión (negativa)", c.precision_neg().to_string(), t.precision_neg().to_string());
        fila(&mut o, "recall (negativa)", c.recall_neg().to_string(), t.recall_neg().to_string());
        fila(
            &mut o,
            "MCC",
            format!("{} {}", num(m.mcc, 3), ci(self.ci_mcc, 1.0, 3)),
            format!("{} (constante: indefinido; equivale a 0)", num(t.mcc(), 3)),
        );
        fila(
            &mut o,
            "AUPRC (línea base = prev.)",
            format!("{} {}", num(m.auprc, 3), ci(self.ci_auprc, 1.0, 3)),
            num(c.prevalencia().valor(), 3),
        );
        let _ = writeln!(o, "bootstrap: {} réplicas, semilla {}, {rem}", crate::replica::REPLICATES, crate::replica::SEED);

        let _ = writeln!(o, "\n-- Resultado continuo por grupo --");
        let _ = writeln!(
            o,
            "mediana no marcados {} (n {}), marcados {} (n {}); diferencia {} {}; Mann-Whitney unilateral (no marcados > marcados) p = {}",
            esc(m.mediana_sin),
            c.vn + c.fn_,
            esc(m.mediana_con),
            c.vp + c.fp,
            esc(m.dif),
            ci(self.ci_dif, spec.escala, 1),
            fmt_p(m.mw_p)
        );

        let _ = writeln!(o, "\n-- Fragilidad --");
        for (modo, k, ids, f) in &self.fragilidad {
            let nombre = match modo {
                Quitar::Extremos => "mayor |resultado|",
                Quitar::AFavor => "los que más sostienen la diferencia",
            };
            let _ = writeln!(
                o,
                "sin {k} ({nombre}): n {} · marcados {}/{} · diferencia {} · p = {} · MCC {} · prevalencia {}",
                f.conf.n(),
                f.conf.vp + f.conf.fp,
                f.conf.n(),
                esc(f.dif),
                fmt_p(f.mw_p),
                num(f.mcc, 3),
                f.conf.prevalencia()
            );
            let _ = writeln!(o, "    quitados: {}", ids.join(", "));
        }

        let _ = writeln!(o, "\n-- Censura: horizontes de seguimiento --");
        for h in censura {
            let motivos: Vec<String> = h.motivos.iter().map(|(m, k)| format!("{m} {k}")).collect();
            let _ = writeln!(
                o,
                "{:8} completan {}{}",
                h.horizonte,
                Prop { k: h.completan, n: h.total() },
                if motivos.is_empty() { String::new() } else { format!("; no completan: {}", motivos.join(" · ")) }
            );
        }
        let _ = writeln!(o, "\n-- Excluidos de la población, y por qué --");
        for (motivo, k) in excluidos {
            let _ = writeln!(o, "{motivo}: {k}");
        }

        let _ = writeln!(o, "\n-- Partición temporal (bloques fijos, nunca aleatoria) --");
        for b in &self.bloques {
            let bm = &b.medida;
            let _ = writeln!(
                o,
                "{} [{}, {}): n {} · prevalencia {} · marcados {} · MCC {} · diferencia {} · p = {}",
                b.nombre,
                b.desde,
                b.hasta,
                bm.conf.n(),
                bm.conf.prevalencia(),
                bm.conf.marcados(),
                num(bm.mcc, 3),
                esc(bm.dif),
                fmt_p(bm.mw_p)
            );
        }
        if self.fuera_de_bloques > 0 {
            let _ = writeln!(o, "casos fuera de todo bloque: {}", self.fuera_de_bloques);
        }
        o
    }
}

/// erfc de Numerical Recipes (copia de `entry2::erfc`, privada en un
/// archivo congelado).
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

/// (U de `x`, n₁n₂/2, varianza con corrección de empates), como `entry2`.
fn u_parts(x: &[f64], y: &[f64]) -> Option<(f64, f64, f64)> {
    if x.is_empty() || y.is_empty() {
        return None;
    }
    let (n1, n2) = (x.len() as f64, y.len() as f64);
    let mut all: Vec<(f64, bool)> = x.iter().map(|&v| (v, true)).chain(y.iter().map(|&v| (v, false))).collect();
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut r1, mut ties, mut i) = (0.0, 0.0, 0);
    while i < all.len() {
        let mut j = i;
        while j + 1 < all.len() && all[j + 1].0 == all[i].0 {
            j += 1;
        }
        let t = (j - i + 1) as f64;
        ties += t * t * t - t;
        r1 += all[i..=j].iter().filter(|a| a.1).count() as f64 * ((i + j) as f64 / 2.0 + 1.0);
        i = j + 1;
    }
    let nn = n1 + n2;
    Some((r1 - n1 * (n1 + 1.0) / 2.0, n1 * n2 / 2.0, n1 * n2 / 12.0 * ((nn + 1.0) - ties / (nn * (nn - 1.0)))))
}

/// Van Elteren unilateral (Wilcoxon estratificado, pesos 1/(N_s + 1)),
/// alternativa "x mayor que y" en cada estrato (x, y). Aproximación normal sin
/// corrección de continuidad. Los estratos con un grupo vacío no cuentan.
/// Devuelve (z, p).
pub fn van_elteren_greater(estratos: &[(Vec<f64>, Vec<f64>)]) -> Option<(f64, f64)> {
    let (mut num, mut var) = (0.0, 0.0);
    for (x, y) in estratos {
        if let Some((u, e, v)) = u_parts(x, y) {
            let w = 1.0 / ((x.len() + y.len()) as f64 + 1.0);
            num += w * (u - e);
            var += w * w * v;
        }
    }
    (var > 0.0).then(|| {
        let z = num / var.sqrt();
        (z, (0.5 * erfc(z / std::f64::consts::SQRT_2)).min(1.0))
    })
}

/// Media de las diferencias de medianas por estrato ponderada por n_s (solo
/// estratos con los dos grupos). `estrato[i]` es el estrato de `casos[i]`.
pub fn dif_ponderada(casos: &[&Caso], estrato: &[usize], n_estratos: usize, es_positivo: &dyn Fn(f64) -> bool) -> Option<f64> {
    let (mut num, mut den) = (0.0, 0.0);
    for s in 0..n_estratos {
        let en: Vec<&Caso> = (0..casos.len()).filter(|&i| estrato[i] == s).map(|i| casos[i]).collect();
        if let Some(d) = medir(&en, es_positivo).dif {
            num += en.len() as f64 * d;
            den += en.len() as f64;
        }
    }
    (den > 0.0).then(|| num / den)
}

#[derive(Debug, Clone)]
pub struct InformeEstratos {
    pub nombres: Vec<String>,
    pub medidas: Vec<Medida>,
    pub ci_estratos: Vec<(f64, f64, usize)>,
    /// (z, p) de van Elteren, no marcados > marcados.
    pub van_elteren: Option<(f64, f64)>,
    pub dif_ponderada: Option<f64>,
    pub ci_ponderada: (f64, f64, usize),
    /// Mínimo de casos por grupo para que un estrato pueda contradecir.
    pub min_contradice: usize,
    pub sobrevive: bool,
}

/// Análisis por estratos fijos. Bootstrap estratificado: en cada réplica se
/// remuestrean, dentro de cada estrato, los clústeres (claves ordenadas) con
/// todos sus casos; semilla `replica::SEED`, `replica::REPLICATES` réplicas.
/// "Sobrevive" si la diferencia ponderada es > 0, su IC excluye 0 por arriba
/// y ningún estrato con ≥ `min_contradice` casos por grupo tiene diferencia < 0.
pub fn estratificar(casos: &[Caso], estrato: &[usize], nombres: &[&str], spec: &Spec, min_contradice: usize) -> InformeEstratos {
    use std::collections::BTreeMap;
    let k = nombres.len();
    let todos: Vec<&Caso> = casos.iter().collect();
    let en = |s: usize| -> Vec<&Caso> { (0..casos.len()).filter(|&i| estrato[i] == s).map(|i| &casos[i]).collect() };
    let medidas: Vec<Medida> = (0..k).map(|s| medir(&en(s), spec.es_positivo)).collect();
    let ci_estratos = (0..k)
        .map(|s| {
            let sub: Vec<Caso> = en(s).into_iter().cloned().collect();
            fn key(c: &Caso) -> &str {
                c.cluster.as_str()
            }
            let kf: &dyn Fn(&Caso) -> &str = &key;
            bootstrap(&sub, Some(kf), &|rs: &[&Caso]| medir(rs, spec.es_positivo).dif)
        })
        .collect();
    let ve = van_elteren_greater(
        &(0..k)
            .map(|s| {
                let g = en(s);
                (
                    g.iter().filter(|c| !c.marca).map(|c| c.resultado).collect(),
                    g.iter().filter(|c| c.marca).map(|c| c.resultado).collect(),
                )
            })
            .collect::<Vec<_>>(),
    );
    let dp = dif_ponderada(&todos, estrato, k, spec.es_positivo);
    // Clústeres por estrato, claves ordenadas.
    let grupos: Vec<Vec<Vec<usize>>> = (0..k)
        .map(|s| {
            let mut m: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
            for i in (0..casos.len()).filter(|&i| estrato[i] == s) {
                m.entry(casos[i].cluster.as_str()).or_default().push(i);
            }
            m.into_values().collect()
        })
        .collect();
    let mut rng = crate::replica::PyRandom::new(crate::replica::SEED);
    let (mut vals, mut drop) = (Vec::new(), 0);
    for _ in 0..crate::replica::REPLICATES {
        let (mut rs, mut es) = (Vec::new(), Vec::new());
        for (s, g) in grupos.iter().enumerate() {
            for _ in 0..g.len() {
                for &i in &g[rng.randrange(g.len())] {
                    rs.push(&casos[i]);
                    es.push(s);
                }
            }
        }
        match dif_ponderada(&rs, &es, k, spec.es_positivo) {
            Some(v) => vals.push(v),
            None => drop += 1,
        }
    }
    vals.sort_by(f64::total_cmp);
    let ci_p = if vals.is_empty() {
        (f64::NAN, f64::NAN, drop)
    } else {
        let n = vals.len();
        (vals[(0.025 * n as f64) as usize], vals[((0.975 * n as f64) as usize).min(n - 1)], drop)
    };
    let contradice = medidas.iter().any(|m| {
        m.conf.vp + m.conf.fp >= min_contradice && m.conf.vn + m.conf.fn_ >= min_contradice && m.dif.is_some_and(|d| d < 0.0)
    });
    InformeEstratos {
        nombres: nombres.iter().map(|s| s.to_string()).collect(),
        medidas,
        ci_estratos,
        van_elteren: ve,
        dif_ponderada: dp,
        ci_ponderada: ci_p,
        min_contradice,
        sobrevive: dp.is_some_and(|d| d > 0.0) && ci_p.0 > 0.0 && !contradice,
    }
}

impl InformeEstratos {
    pub fn texto(&self, spec: &Spec) -> String {
        let mut o = String::new();
        let esc = |x: Option<f64>| x.map_or("n/d".into(), |v| format!("{:+.1} {}", v * spec.escala, spec.unidad));
        let _ = writeln!(o, "-- Por estratos fijos de actividad --");
        for ((nombre, m), c) in self.nombres.iter().zip(&self.medidas).zip(&self.ci_estratos) {
            let _ = writeln!(
                o,
                "{nombre:14} n {:3} · marcados {} · mediana no marcados {} · marcados {} · diferencia {} {} · p = {}",
                m.conf.n(),
                m.conf.marcados(),
                esc(m.mediana_sin),
                esc(m.mediana_con),
                esc(m.dif),
                ci(*c, spec.escala, 1),
                fmt_p(m.mw_p)
            );
        }
        let _ = writeln!(
            o,
            "van Elteren unilateral (no marcados > marcados): {}",
            self.van_elteren.map_or("n/d".into(), |(z, p)| format!("z = {z:.3}, p = {}", fmt_p(Some(p))))
        );
        let _ = writeln!(
            o,
            "diferencia ponderada por n_s: {} {} (bootstrap estratificado por clúster)",
            esc(self.dif_ponderada),
            ci(self.ci_ponderada, spec.escala, 1)
        );
        let _ = writeln!(
            o,
            "¿sobrevive al control? {} (dirección declarada, IC > 0 y ningún estrato con ≥ {} por grupo en contra)",
            if self.sobrevive { "SÍ" } else { "NO" },
            self.min_contradice
        );
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caso(id: &str, marca: bool, r: f64) -> Caso {
        Caso { id: id.into(), cluster: id.into(), t: 0, marca, score: if marca { 1.0 } else { 0.0 }, resultado: r }
    }

    #[test]
    fn wilson_conocido() {
        // 10/20 con Wilson al 95 %: [0.2993, 0.7007].
        let (lo, hi) = wilson(10, 20).unwrap();
        assert!((lo - 0.299_298).abs() < 1e-5 && (hi - 0.700_702).abs() < 1e-5);
        assert_eq!(wilson(0, 0), None);
        assert_eq!(wilson(0, 5).unwrap().0, 0.0);
    }

    #[test]
    fn mcc_y_trivial() {
        let c = Confusion { vp: 6, fp: 2, fn_: 3, vn: 9 };
        let esperado = (6.0 * 9.0 - 2.0 * 3.0) / (8.0f64 * 9.0 * 11.0 * 12.0).sqrt();
        assert!((c.mcc().unwrap() - esperado).abs() < 1e-12);
        // Predicción constante: indefinido.
        assert_eq!(Confusion { vp: 5, fp: 7, fn_: 0, vn: 0 }.mcc(), None);
    }

    #[test]
    fn auprc_binaria_y_empates() {
        // Regla binaria: AP = recall · precisión + (1 − recall) · prevalencia.
        let v = [(1.0, true), (1.0, false), (0.0, true), (0.0, false), (0.0, false)];
        let (r, p, prev) = (0.5, 0.5, 0.4);
        assert!((auprc(&v).unwrap() - (r * p + (1.0 - r) * prev)).abs() < 1e-12);
        // Puntuación constante: AP = prevalencia.
        let c = [(0.3, true), (0.3, false), (0.3, false), (0.3, false)];
        assert!((auprc(&c).unwrap() - 0.25).abs() < 1e-12);
        // Orden perfecto: 1.
        assert!((auprc(&[(0.9, true), (0.8, true), (0.1, false)]).unwrap() - 1.0).abs() < 1e-12);
        assert_eq!(auprc(&[(1.0, false)]), None);
    }

    #[test]
    fn fragilidad_quita_lo_esperado() {
        let cs = [caso("a", false, 0.5), caso("b", false, 0.1), caso("c", true, -0.9), caso("d", true, -0.1), caso("e", false, 0.0)];
        let refs: Vec<&Caso> = cs.iter().collect();
        let pos = |r: f64| r <= 0.0;
        assert_eq!(a_quitar(&refs, 2, Quitar::Extremos, &pos), vec![2, 0]);
        // Diferencia 0.1 − (−0.5) = 0.6. Sin "c" queda 0.1 − (−0.1) = 0.2 (sin "a", 0.55):
        // "c" es el que más la sostiene, y después "a".
        assert_eq!(a_quitar(&refs, 2, Quitar::AFavor, &pos), vec![2, 0]);
    }

    #[test]
    fn bloques_y_censura() {
        let mut cs = vec![caso("a", true, -0.5), caso("b", false, 0.2)];
        cs[1].t = 100;
        let pos = |r: f64| r <= 0.0;
        let spec = Spec {
            titulo: "t",
            positivo: "p",
            es_positivo: &pos,
            unidad: "pts",
            escala: 100.0,
            remuestreo: Remuestreo::Casos,
            bloques: vec![("B1".into(), 0, 50), ("B2".into(), 50, 200)],
        };
        let inf = evaluar(&cs, &spec);
        assert_eq!(inf.bloques[0].medida.conf.n(), 1);
        assert_eq!(inf.bloques[1].medida.conf.n(), 1);
        assert_eq!(inf.fuera_de_bloques, 0);
        assert!(inf.trivial_positiva);
        let mut c = Censura::nuevo("+1 h");
        c.anota(None);
        c.anota(Some("x"));
        c.anota(Some("x"));
        assert_eq!((c.completan, c.motivos.clone(), c.total()), (1, vec![("x".into(), 2)], 3));
    }

    #[test]
    fn van_elteren_un_estrato_es_mann_whitney_sin_continuidad() {
        let x = vec![3.0, 4.0, 5.0, 5.0, 9.0];
        let y = vec![1.0, 2.0, 5.0, 0.5];
        let (u, e, v) = u_parts(&x, &y).unwrap();
        let (u2, _) = crate::entry2::mann_whitney_greater(&x, &y).unwrap();
        assert_eq!(u, u2);
        let (z, p) = van_elteren_greater(&[(x, y)]).unwrap();
        assert!((z - (u - e) / v.sqrt()).abs() < 1e-12);
        assert!((p - 0.5 * erfc(z / std::f64::consts::SQRT_2)).abs() < 1e-12);
    }

    #[test]
    fn estratos_ponderan_y_detectan_contradiccion() {
        let mut cs = Vec::new();
        for i in 0..4 {
            cs.push(caso(&format!("a{i}"), false, 0.2 + i as f64 * 0.01));
            cs.push(caso(&format!("b{i}"), true, -0.2 - i as f64 * 0.01));
        }
        // Estrato 1 al revés.
        for i in 0..4 {
            cs.push(caso(&format!("c{i}"), false, -0.3));
            cs.push(caso(&format!("d{i}"), true, 0.3 + i as f64 * 0.01));
        }
        let est: Vec<usize> = (0..cs.len()).map(|i| usize::from(i >= 8)).collect();
        let pos = |r: f64| r <= 0.0;
        let spec = Spec {
            titulo: "t",
            positivo: "p",
            es_positivo: &pos,
            unidad: "pts",
            escala: 100.0,
            remuestreo: Remuestreo::Cluster,
            bloques: vec![],
        };
        let refs: Vec<&Caso> = cs.iter().collect();
        let d = dif_ponderada(&refs, &est, 2, &pos).unwrap();
        // (8 · 0.43 + 8 · (−0.615)) / 16.
        assert!((d - (0.43 - 0.615) / 2.0).abs() < 1e-9);
        let inf = estratificar(&cs, &est, &["e0", "e1"], &spec, 4);
        assert!(!inf.sobrevive);
    }
}
