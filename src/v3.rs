//! Validación 3 (CLAUDE.md sección 8, pre-registro congelado el 2026-10-01
//! 16:39:58 UTC): filtro de descarte D2 contra el retorno neto (a) a +30 min
//! desde T_entry2. Cálculo puro: clasificación, población, criterios y
//! veredicto. No toca RPC ni la base.

use std::collections::HashSet;

/// Criterio 1: mínimo de tokens en cada grupo (descartados y que pasan).
pub const MIN_GROUP: usize = 20;
/// Criterio 2: Mann-Whitney unilateral (pasan > descartados), p < esto.
pub const MAX_P: f64 = 0.01;
/// Criterio 3: diferencia de medianas (pasan − descartados) ≥ esto.
pub const MIN_DIFF: f64 = 0.10;
/// Tolerancia de coma flotante en las comparaciones de umbral (como en entry2).
const EPS: f64 = 1e-9;

pub const SEPARA: &str = "D2 SEPARA GRUPOS";
pub const NO_CONFIRMADO: &str = "NO CONFIRMADO";
pub const NO_CONCLUYENTE: &str = "NO CONCLUYENTE POR POTENCIA";

/// Adenda 3: intentos de descarga tras los que un token sin ventana o sin
/// precios es "sin datos".
pub const MAX_ATTEMPTS: i64 = 3;
/// Adenda 4: margen del listado por blockTime sobre el fin del tramo.
pub const UNTIL_MARGIN_SECS: i64 = 120;

/// Horizontes desde T_entry2; el resultado principal es +30 min.
pub const HORIZONS: [(i64, &str); 5] = [(5, "+5 s"), (30, "+30 s"), (300, "+5 min"), (1800, "+30 min"), (3600, "+60 min")];
pub const PRIMARY: usize = 3;

/// D2: descartado si cumple al menos 2 de H4', H6' y H7'. `None` = "no
/// evaluable" (adenda 1): H6' n/e y exactamente una de H4'/H7'.
pub fn d2(h4: bool, h6: Option<bool>, h7: bool) -> Option<bool> {
    let k = h4 as u8 + h7 as u8;
    match (h6, k) {
        (Some(b), _) => Some(k + b as u8 >= 2),
        (None, 2) => Some(true),
        (None, 0) => Some(false),
        (None, _) => None,
    }
}

/// Candidato a la población: un token con T_entry2.
pub struct Cand<'a> {
    pub mint: &'a str,
    pub creator: &'a str,
    pub slot: u64,
    pub mayhem: bool,
}

/// Un token por creator entre los candidatos con `mayhem == group` (la
/// población que decide es `group = false`: primero se excluyen los mayhem).
/// Menor slot; el orden de ejecución entre creaciones no se guarda, así que
/// el empate se resuelve por mint lexicográfico. Devuelve los índices en
/// orden (slot, mint) y el número de empates de slot resueltos por mint.
pub fn population(c: &[Cand], group: bool) -> (Vec<usize>, usize) {
    let mut idx: Vec<usize> = (0..c.len()).filter(|&i| c[i].mayhem == group).collect();
    idx.sort_by(|&a, &b| (c[a].slot, c[a].mint).cmp(&(c[b].slot, c[b].mint)));
    let mut first: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();
    let (mut out, mut ties) = (Vec::new(), 0);
    for i in idx {
        match first.get(c[i].creator) {
            None => {
                first.insert(c[i].creator, c[i].slot);
                out.push(i);
            }
            Some(&s) if s == c[i].slot => ties += 1,
            _ => {}
        }
    }
    (out, ties)
}

/// Estado de los datos descargados de un token (adenda 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Data {
    Complete,
    /// Incompleto tras `MAX_ATTEMPTS` intentos: n/e "sin datos".
    SinDatos,
    /// Incompleto con menos intentos: la descarga sigue pendiente.
    Pending,
}

pub fn data_status(complete: bool, attempts: i64) -> Data {
    if complete {
        Data::Complete
    } else if attempts >= MAX_ATTEMPTS {
        Data::SinDatos
    } else {
        Data::Pending
    }
}

/// Un token de la comparación (población que decide con retorno evaluable).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Obs {
    pub mint: String,
    pub slot: u64,
    pub discarded: bool,
    pub ret: f64,
    pub window_trades: usize,
}

/// Token de la población, antes de excluir nada de la comparación.
pub struct Item<'a> {
    pub mint: &'a str,
    pub slot: u64,
    pub window_trades: usize,
    pub d2: Option<bool>,
    /// Su creator queda fuera por un token sin ventana temprana que lo
    /// precede (adenda 7).
    pub creator_sin_datos: bool,
    /// Ventana de precio "sin datos" (adenda 3).
    pub sin_datos: bool,
    /// Retorno neto (a) a +30 min.
    pub ret: Option<f64>,
}

/// Exclusiones de la comparación, contadas aparte.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Excluded {
    /// Adenda 7: creators fuera por un token sin ventana temprana anterior.
    pub creator_sin_datos: usize,
    /// Adenda 3: sin precios tras 3 intentos.
    pub sin_datos: usize,
    /// Adenda 1: D2 no evaluable.
    pub no_evaluable: usize,
    /// Retorno (a) a +30 min n/e por las reglas congeladas.
    pub ret_ne: usize,
}

/// Tokens de la comparación y exclusiones (cada token se cuenta en la
/// primera que le aplica: creator sin datos, sin datos, no evaluable,
/// retorno n/e).
pub fn comparison(items: &[Item]) -> (Vec<Obs>, Excluded) {
    let (mut obs, mut ex) = (Vec::new(), Excluded::default());
    for it in items {
        if it.creator_sin_datos {
            ex.creator_sin_datos += 1;
        } else if it.sin_datos {
            ex.sin_datos += 1;
        } else if let Some(discarded) = it.d2 {
            match it.ret {
                Some(ret) => obs.push(Obs {
                    mint: it.mint.to_string(),
                    slot: it.slot,
                    discarded,
                    ret,
                    window_trades: it.window_trades,
                }),
                None => ex.ret_ne += 1,
            }
        } else {
            ex.no_evaluable += 1;
        }
    }
    (obs, ex)
}

/// Token de mayor peso (definición congelada): más trades en su ventana de
/// 5 min entre los de la comparación; empate por menor slot y después mint
/// lexicográfico (adenda 2).
pub fn heaviest(obs: &[Obs]) -> Option<usize> {
    (0..obs.len()).min_by(|&a, &b| {
        let (x, y) = (&obs[a], &obs[b]);
        y.window_trades.cmp(&x.window_trades).then(x.slot.cmp(&y.slot)).then(x.mint.cmp(&y.mint))
    })
}

/// Adenda 7: creators con un token "sin datos" de la ventana temprana
/// (T_entry2 desconocido) que precede, en (slot, mint), a su token elegido.
/// No se sabe qué token los representa: quedan fuera de la comparación y no
/// entra el siguiente. Devuelve los creators, sin repetir, en orden.
pub fn creators_sin_datos<'a>(chosen: &[&Cand<'a>], no_data: &[Cand]) -> Vec<&'a str> {
    let mut out: Vec<&str> = chosen
        .iter()
        .filter(|c| {
            no_data.iter().any(|n| !n.mayhem && n.creator == c.creator && (n.slot, n.mint) < (c.slot, c.mint))
        })
        .map(|c| c.creator)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    crate::replica::median(&mut v)
}

/// Diferencia de medianas (pasan − descartados); `None` si algún grupo está vacío.
pub fn median_diff(obs: &[&Obs]) -> Option<f64> {
    let pass = median(obs.iter().filter(|o| !o.discarded).map(|o| o.ret).collect())?;
    let disc = median(obs.iter().filter(|o| o.discarded).map(|o| o.ret).collect())?;
    Some(pass - disc)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Criteria {
    pub n_pass: usize,
    pub n_disc: usize,
    pub median_pass: Option<f64>,
    pub median_disc: Option<f64>,
    /// Mann-Whitney unilateral (pasan > descartados).
    pub mw_u: Option<f64>,
    pub mw_p: Option<f64>,
    pub diff: Option<f64>,
    pub heaviest: Option<String>,
    pub diff_without_heaviest: Option<f64>,
    /// IC bootstrap 2.5–97.5 de la diferencia de medianas (lo, hi, réplicas descartadas).
    pub ci: (f64, f64, usize),
}

/// Calcula los cinco criterios. `heaviest` ya resuelto (sin empate).
pub fn criteria(obs: &[Obs], heaviest: Option<usize>) -> Criteria {
    let all: Vec<&Obs> = obs.iter().collect();
    let pass: Vec<f64> = obs.iter().filter(|o| !o.discarded).map(|o| o.ret).collect();
    let disc: Vec<f64> = obs.iter().filter(|o| o.discarded).map(|o| o.ret).collect();
    let mw = crate::entry2::mann_whitney_greater(&pass, &disc);
    let wo: Vec<&Obs> = (0..obs.len()).filter(|&i| Some(i) != heaviest).map(|i| &obs[i]).collect();
    Criteria {
        n_pass: pass.len(),
        n_disc: disc.len(),
        median_pass: median(pass),
        median_disc: median(disc),
        mw_u: mw.map(|m| m.0),
        mw_p: mw.map(|m| m.1),
        diff: median_diff(&all),
        heaviest: heaviest.map(|i| obs[i].mint.clone()),
        diff_without_heaviest: median_diff(&wo),
        ci: crate::replica::bootstrap(obs, None, &|rs: &[&Obs]| median_diff(rs)),
    }
}

/// Veredicto y cumplimiento de cada criterio (1–5).
pub fn decide(c: &Criteria) -> (&'static str, [bool; 5]) {
    let ok = [
        c.n_pass >= MIN_GROUP && c.n_disc >= MIN_GROUP,
        c.mw_p.is_some_and(|p| p < MAX_P),
        c.diff.is_some_and(|d| d >= MIN_DIFF - EPS),
        c.diff_without_heaviest.is_some_and(|d| d > EPS),
        c.ci.0 > 0.0 || c.ci.1 < 0.0,
    ];
    let v = if !ok[0] {
        NO_CONCLUYENTE
    } else if ok.iter().all(|&x| x) {
        SEPARA
    } else {
        NO_CONFIRMADO
    };
    (v, ok)
}

/// Momento del primer precio ≥ 2× el inicial respecto a T_entry2, por slot
/// (T_entry2 es el primer trade de su slot en orden de ejecución).
pub fn first_2x_timing(first_2x_slot: Option<u64>, entry_slot: u64) -> &'static str {
    match first_2x_slot {
        None => "sin 2x",
        Some(s) if s < entry_slot => "antes",
        Some(s) if s == entry_slot => "en el slot de T_entry2",
        Some(_) => "después",
    }
}

/// (H4', H7') de un token.
pub type H4H7 = (bool, bool);

/// Cambios de H4' y H7' con identidades de creador limitadas a lo anterior
/// a T_entry2 (descriptivo; no entra en el veredicto): pares (congeladas,
/// limitadas).
pub fn identity_changes(pairs: &[(H4H7, H4H7)]) -> (usize, usize) {
    (pairs.iter().filter(|(a, b)| a.0 != b.0).count(), pairs.iter().filter(|(a, b)| a.1 != b.1).count())
}

/// Creators distintos de un conjunto de candidatos.
pub fn creators(c: &[Cand], idx: &[usize]) -> usize {
    idx.iter().map(|&i| c[i].creator).collect::<HashSet<_>>().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(disc: bool, ret: f64, trades: usize) -> Obs {
        Obs { mint: format!("{disc}{ret}{trades}"), slot: 0, discarded: disc, ret, window_trades: trades }
    }

    #[test]
    fn d2_cuenta_dos_de_tres_y_marca_los_casos_que_dependen_de_h6_ne() {
        assert_eq!(d2(true, Some(true), false), Some(true));
        assert_eq!(d2(true, Some(false), false), Some(false));
        assert_eq!(d2(false, Some(true), true), Some(true));
        assert_eq!(d2(false, Some(false), false), Some(false));
        assert_eq!(d2(true, None, true), Some(true));
        assert_eq!(d2(false, None, false), Some(false));
        assert_eq!(d2(true, None, false), None);
        assert_eq!(d2(false, None, true), None);
    }

    #[test]
    fn poblacion_excluye_mayhem_primero_y_toma_un_token_por_creator() {
        let c = [
            Cand { mint: "m1", creator: "A", slot: 10, mayhem: true }, // mayhem: fuera
            Cand { mint: "m2", creator: "A", slot: 12, mayhem: false }, // primer no-mayhem de A
            Cand { mint: "zz", creator: "B", slot: 11, mayhem: false },
            Cand { mint: "aa", creator: "B", slot: 11, mayhem: false }, // empate de slot: gana por mint
            Cand { mint: "m5", creator: "B", slot: 9, mayhem: true },
            Cand { mint: "m6", creator: "C", slot: 20, mayhem: false },
            Cand { mint: "m7", creator: "C", slot: 15, mayhem: false }, // menor slot de C
        ];
        let (p, ties) = population(&c, false);
        let mints: Vec<&str> = p.iter().map(|&i| c[i].mint).collect();
        assert_eq!(mints, ["aa", "m2", "m7"]);
        assert_eq!(ties, 1);
        let (m, _) = population(&c, true);
        assert_eq!(m.iter().map(|&i| c[i].mint).collect::<Vec<_>>(), ["m5", "m1"]);
        assert_eq!(creators(&c, &p), 3);
    }

    #[test]
    fn mayor_peso_y_desempate_por_slot_y_mint() {
        assert_eq!(heaviest(&[obs(true, 0.0, 5), obs(false, 0.0, 9), obs(false, 0.0, 3)]), Some(1));
        assert_eq!(heaviest(&[]), None);
        // Empate en trades: gana el menor slot.
        let mut a = obs(true, 0.0, 9);
        a.slot = 20;
        let mut b = obs(false, 0.0, 9);
        b.slot = 10;
        assert_eq!(heaviest(&[a.clone(), b.clone()]), Some(1));
        // Empate en trades y slot: gana el mint menor.
        b.slot = 20;
        a.mint = "zz".into();
        b.mint = "aa".into();
        assert_eq!(heaviest(&[a, b]), Some(1));
    }

    #[test]
    fn sin_datos_tras_tres_intentos() {
        assert_eq!(data_status(true, 0), Data::Complete);
        assert_eq!(data_status(false, 0), Data::Pending);
        assert_eq!(data_status(false, 2), Data::Pending);
        assert_eq!(data_status(false, 3), Data::SinDatos);
        assert_eq!(data_status(true, 7), Data::Complete);
    }

    #[test]
    fn comparacion_excluye_no_evaluables_sin_datos_y_ne_y_los_cuenta() {
        let it = |mint, d2, sin_datos, ret| Item { mint, slot: 1, window_trades: 1, d2, creator_sin_datos: false, sin_datos, ret };
        let items = [
            it("a", Some(true), false, Some(-0.3)),
            it("b", Some(false), false, Some(-0.05)),
            it("c", None, false, Some(0.1)),       // no evaluable (adenda 1)
            it("d", Some(true), true, None),       // sin datos (adenda 3)
            it("e", None, true, None),             // sin datos primero
            it("f", Some(false), false, None),     // retorno n/e
        ];
        let (o, ex) = comparison(&items);
        assert_eq!(o.iter().map(|x| x.mint.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(ex, Excluded { creator_sin_datos: 0, sin_datos: 2, no_evaluable: 1, ret_ne: 1 });
    }

    #[test]
    fn creator_fuera_si_su_primer_token_no_tiene_ventana_y_no_entra_el_siguiente() {
        // A: m1 (sin ventana temprana, 3 intentos) precede a m2, que sería el
        // elegido. A queda fuera; m2 no entra en la comparación.
        let chosen = [
            Cand { mint: "m2", creator: "A", slot: 12, mayhem: false },
            Cand { mint: "m9", creator: "B", slot: 30, mayhem: false },
        ];
        let nd = [Cand { mint: "m1", creator: "A", slot: 11, mayhem: false }];
        let fuera = creators_sin_datos(&chosen.iter().collect::<Vec<_>>(), &nd);
        assert_eq!(fuera, ["A"]);
        let items: Vec<Item> = chosen
            .iter()
            .map(|c| Item {
                mint: c.mint,
                slot: c.slot,
                window_trades: 1,
                d2: Some(false),
                creator_sin_datos: fuera.contains(&c.creator),
                sin_datos: false,
                ret: Some(-0.05),
            })
            .collect();
        let (o, ex) = comparison(&items);
        assert_eq!(o.iter().map(|x| x.mint.as_str()).collect::<Vec<_>>(), ["m9"]);
        assert_eq!(ex.creator_sin_datos, 1);
    }

    #[test]
    fn creator_sin_datos_solo_si_el_sin_ventana_precede_al_elegido() {
        let chosen = Cand { mint: "m2", creator: "A", slot: 12, mayhem: false };
        let other = Cand { mint: "m9", creator: "B", slot: 30, mayhem: false };
        let nd = [
            Cand { mint: "m1", creator: "A", slot: 11, mayhem: false }, // precede: conflicto
            Cand { mint: "m3", creator: "A", slot: 13, mayhem: false }, // posterior: no
            Cand { mint: "m4", creator: "C", slot: 1, mayhem: false },  // otro creator: no
            Cand { mint: "m0", creator: "B", slot: 2, mayhem: true },   // mayhem: no cuenta
        ];
        assert_eq!(creators_sin_datos(&[&chosen, &other], &nd), ["A"]);
    }

    #[test]
    fn bootstrap_determinista_con_la_semilla() {
        // Referencia (python3, mismo algoritmo que robustez_posthoc.ci):
        //   pop = [(True,-.30),(True,-.25),(True,-.10),(False,-.05),(False,.02),(False,-.06)]
        //   r = random.Random(20260930); 2000 réplicas de [pop[r.randrange(6)] for _ in pop];
        //   stat = mediana(pasan) − mediana(descartados), None si algún grupo vacío;
        //   ordenar; lo = v[int(.025 n)], hi = v[min(n−1, int(.975 n))].
        let o = [
            obs(true, -0.30, 1),
            obs(true, -0.25, 2),
            obs(true, -0.10, 3),
            obs(false, -0.05, 4),
            obs(false, 0.02, 5),
            obs(false, -0.06, 6),
        ];
        let a = criteria(&o, Some(5));
        let b = criteria(&o, Some(5));
        assert_eq!(a.ci.2, b.ci.2);
        assert_eq!((a.ci.0.to_bits(), a.ci.1.to_bits()), (b.ci.0.to_bits(), b.ci.1.to_bits()));
        assert_eq!(a.ci, REF_CI);
    }
    // Salida de python3 con el algoritmo del comentario: 0.04000000000000001 0.32 55.
    const REF_CI: (f64, f64, usize) = (0.04000000000000001, 0.32, 55);

    fn base() -> Criteria {
        Criteria {
            n_pass: 20,
            n_disc: 20,
            median_pass: Some(-0.05),
            median_disc: Some(-0.30),
            mw_u: Some(300.0),
            mw_p: Some(0.001),
            diff: Some(0.25),
            heaviest: None,
            diff_without_heaviest: Some(0.2),
            ci: (0.1, 0.4, 0),
        }
    }

    #[test]
    fn cada_rama_del_veredicto() {
        assert_eq!(decide(&base()), (SEPARA, [true; 5]));
        let v = |f: &dyn Fn(&mut Criteria)| {
            let mut c = base();
            f(&mut c);
            decide(&c)
        };
        // Potencia: criterio 1, aunque todo lo demás se cumpla.
        assert_eq!(v(&|c| c.n_pass = 19).0, NO_CONCLUYENTE);
        assert_eq!(v(&|c| c.n_disc = 19).0, NO_CONCLUYENTE);
        // No confirmado por cada criterio 2–5 por separado.
        assert_eq!(v(&|c| c.mw_p = Some(0.01)), (NO_CONFIRMADO, [true, false, true, true, true]));
        assert_eq!(v(&|c| c.diff = Some(0.0999)), (NO_CONFIRMADO, [true, true, false, true, true]));
        assert_eq!(v(&|c| c.diff = Some(0.1)).0, SEPARA);
        assert_eq!(v(&|c| c.diff_without_heaviest = Some(0.0)), (NO_CONFIRMADO, [true, true, true, false, true]));
        assert_eq!(v(&|c| c.ci = (-0.01, 0.3, 0)), (NO_CONFIRMADO, [true, true, true, true, false]));
        assert_eq!(v(&|c| c.ci = (0.0, 0.3, 0)).0, NO_CONFIRMADO, "el IC debe excluir 0 estrictamente");
    }

    #[test]
    fn criterios_de_extremo_a_extremo_con_datos_sinteticos() {
        // 25 que pasan con retorno alrededor de −5 % y 25 descartados alrededor de −30 %.
        let mut o: Vec<Obs> = (0..25).map(|i| obs(false, -0.05 + 0.001 * i as f64, i)).collect();
        o.extend((0..25).map(|i| obs(true, -0.30 + 0.001 * i as f64, 100 + i)));
        let h = heaviest(&o);
        assert_eq!(o[h.unwrap()].window_trades, 124);
        let c = criteria(&o, h);
        assert_eq!((c.n_pass, c.n_disc), (25, 25));
        assert!((c.diff.unwrap() - 0.25).abs() < 1e-12);
        assert!(c.mw_p.unwrap() < 1e-6);
        assert_eq!(decide(&c).0, SEPARA);
        // Mismos datos con 19 descartados: no concluyente por potencia.
        let c = criteria(&o[..44], heaviest(&o[..44]));
        assert_eq!(decide(&c).0, NO_CONCLUYENTE);
        // Sin separación: no confirmado.
        let mixed: Vec<Obs> = (0..50).map(|i| obs(i % 2 == 0, -0.1 + 0.001 * i as f64, i)).collect();
        let c = criteria(&mixed, heaviest(&mixed));
        assert_eq!(decide(&c).0, NO_CONFIRMADO);
    }

    #[test]
    fn momento_del_primer_2x() {
        assert_eq!(first_2x_timing(Some(5), 7), "antes");
        assert_eq!(first_2x_timing(Some(7), 7), "en el slot de T_entry2");
        assert_eq!(first_2x_timing(Some(9), 7), "después");
        assert_eq!(first_2x_timing(None, 7), "sin 2x");
        assert_eq!(identity_changes(&[((true, false), (true, true)), ((false, false), (false, false))]), (0, 1));
    }
}
