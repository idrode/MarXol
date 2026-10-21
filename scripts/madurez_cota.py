#!/usr/bin/env python3
"""Cota superior de rentabilidad en el universo de madurez — POST HOC, SIN PRE-REGISTRO,
OPTIMISTA A PROPÓSITO. Sin red: solo lee madurez.db. Reutiliza la rejilla horaria, las
observaciones y los universos de madurez_explora.py.

    python3 -B scripts/madurez_cota.py [--day 2026-08-20] > resultados/madurez_cota.txt

Definiciones (fijadas antes de calcular, 2026-10-21):
- Entrada: cierre de la vela de observación (retraso 0) o de la vela siguiente (retraso 1 h).
- Reglas (lista cerrada): TP en {+50 %, +100 %}, stop en {−20 %, −40 %, ninguno},
  horizonte 7 d = 168 velas tras la entrada. Salida en el primer cierre horario con
  retorno bruto ≥ TP o ≤ stop, a ese cierre; si no cruza, al cierre de la vela 168.
- Comisión de ida y vuelta f ∈ {0.5 %, 2.5 %}: neto = (1 + bruto) · (1 − f) − 1.
  Sin deslizamiento ni impacto de precio.
- Acierto: neto > 0.
- Bootstrap: 2 000 réplicas, semilla 20260930, remuestreo de tokens con reemplazo
  (todas sus observaciones); IC95 de la media y la mediana del neto, percentiles 2.5–97.5.
"""
import argparse, random, statistics
import madurez_fase0b as f0
import madurez_explora as ex

SEED = 20260930
REPS = 2000
HORIZON = 168
DELAYS = [0, 1]
TPS = [0.50, 1.00]
SLS = [-0.20, -0.40, None]
FEES = [0.005, 0.025]


def trade(path, delay, tp, sl, fee):
    p0 = path[delay]
    exit_ = path[delay + HORIZON]
    for c in path[delay + 1:delay + HORIZON + 1]:
        r = c / p0 - 1
        if r >= tp or (sl is not None and r <= sl):
            exit_ = c
            break
    return (exit_ / p0) * (1 - fee) - 1


def boot(rows):
    by = {}
    for pool, r in rows:
        by.setdefault(pool, []).append(r)
    pools = sorted(by)
    rng = random.Random(SEED)
    means, meds = [], []
    for _ in range(REPS):
        s = [r for p in (rng.choice(pools) for _ in pools) for r in by[p]]
        means.append(statistics.fmean(s))
        meds.append(statistics.median(s))
    q = lambda xs: (sorted(xs)[int(.025 * (len(xs) - 1))], sorted(xs)[int(.975 * (len(xs) - 1))])
    return q(means), q(meds)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--day", default="2026-08-20")
    ap.add_argument("--madurez-db", default="madurez.db")
    args = ap.parse_args()
    obs, _, _ = ex.observations(f0.ro(args.madurez_db), args.day)
    L = []
    p = L.append
    p("Cota superior de rentabilidad, universo de madurez — POST HOC, SIN PRE-REGISTRO, OPTIMISTA A PROPÓSITO")
    p(f"Generado con: python3 -B scripts/madurez_cota.py --day {args.day}  (solo lee madurez.db, sin red)")
    p("")
    p("AVISO: sin liquidez histórica no se puede estimar el deslizamiento ni el impacto de precio. Cualquier")
    p("retorno positivo de este informe NO se puede dar por válido: es una cota superior, no una estimación.")
    p("")
    combos, best, meds_all, mean_pos = 0, None, [], []
    for thr in ex.UNIVERSES:
        univ = [x for x in obs if x["vol24"] >= thr]
        short = sum(1 for x in univ if len(x["path"]) < max(DELAYS) + HORIZON + 1)
        univ = [x for x in univ if len(x["path"]) >= max(DELAYS) + HORIZON + 1]
        p(f"== Universo volumen 24 h ≥ {thr:,} USD: {len(univ)} observaciones, {len({x['pool'] for x in univ})} tokens"
          f" (sin rejilla suficiente: {short})")
        p("  retraso  TP     stop   comisión |   n  acierto   media   IC95 media          mediana  IC95 mediana")
        for d in DELAYS:
            for tp in TPS:
                for sl in SLS:
                    for fee in FEES:
                        combos += 1
                        rows = [(x["pool"], trade(x["path"], d, tp, sl, fee)) for x in univ]
                        r = [v for _, v in rows]
                        (m0, m1), (d0, d1) = boot(rows)
                        med = statistics.median(r)
                        meds_all.append(med)
                        if m0 > 0:
                            mean_pos.append(f"≥ {thr:,}, retraso {d} h, TP +{tp:.0%}, stop {'ninguno' if sl is None else f'{sl:.0%}'}, comisión {fee:.1%} [{m0:+.1%}, {m1:+.1%}]")
                        if best is None or d0 > best[0]:
                            best = (d0, med, thr, d, tp, sl, fee)
                        p(f"  {d} h      +{tp:.0%}  {('ninguno' if sl is None else f'{sl:.0%}'):>7}  {fee:>6.1%}  | "
                          f"{len(r):>3}  {sum(v > 0 for v in r) / len(r):>6.1%}  {statistics.fmean(r):>+7.1%}  "
                          f"[{m0:>+6.1%}, {m1:>+6.1%}]  {med:>+7.1%}  [{d0:>+6.1%}, {d1:>+6.1%}]")
        p("")
    p(f"== Combinaciones: {len(ex.UNIVERSES)} universos × {len(DELAYS)} retrasos × {len(TPS)} TP × {len(SLS)} stops × "
      f"{len(FEES)} comisiones = {combos} (los universos se solapan; las reglas, también)")
    p(f"  Mayor límite inferior del IC de la mediana: {best[0]:+.1%} (mediana {best[1]:+.1%}; universo ≥ {best[2]:,}, "
      f"retraso {best[3]} h, TP +{best[4]:.0%}, stop {'ninguno' if best[5] is None else f'{best[5]:.0%}'}, comisión {best[6]:.1%})")
    p(f"  Medianas netas: todas ≤ {max(meds_all):+.1%}; combinaciones con IC de la mediana por encima de 0: 0" if max(meds_all) <= 0
      else f"  Mediana neta máxima: {max(meds_all):+.1%}")
    p(f"  Combinaciones con IC95 de la MEDIA por encima de 0: {len(mean_pos)} de {combos}" + "".join("\n    " + m for m in mean_pos))
    p("")
    p("== Límites")
    p("  - Sin liquidez histórica: no hay deslizamiento ni impacto de precio. Es optimista a propósito y un resultado")
    p("    positivo no sería válido.")
    p("  - Salida a cierres horarios: los máximos y mínimos intrahora no se usan para el stop, así que el stop puede")
    p("    salir mucho peor que su nivel (el cierre puede estar muy por debajo), y el TP también al cierre.")
    p("  - Precios rellenados: en horas sin vela se entra o sale al último cierre, que puede no ser ejecutable.")
    p("  - Un solo día de graduaciones (2026-08-20), una sola fuente (GeckoTerminal, sin verificar en cadena).")
    p("  - Post hoc, sin pre-registro; universo tras el corte de las 72 h y elegido mirando madurez_vida.")
    p("  - Observaciones del mismo token correlacionadas y con operaciones solapadas (una por día, horizonte 7 d):")
    p("    no es una cartera ejecutable; el bootstrap por token solo corrige el IC.")
    p("  - Precio en USD: en pools con quote SOL incluye la variación del SOL.")
    print("\n".join(L))


if __name__ == "__main__":
    main()
