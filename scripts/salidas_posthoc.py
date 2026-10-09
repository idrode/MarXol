#!/usr/bin/env python3
"""Reglas de salida corta — POST HOC, EXPLORATORIO, SIN PRE-REGISTRO.
No cambia ninguna definición ni veredicto (V1, V2, V3, D2). Solo lee marxol.db
(read-only, sin red, sin cuota). Reutiliza la carga de `cribado_posthoc.py`.

Reproducir (desde la raíz del repo; tarda unos minutos por el bootstrap):
    cargo build --release && python3 -B scripts/salidas_posthoc.py > resultados/salidas_posthoc.txt

DEFINICIONES (fijadas antes de calcular)
- Población: en V1, V2 y V3 por separado, la regla de la validación 3 (la de
  etapa0_latencia.py): tokens con T_entry2, se quitan los mayhem y se toma un
  token por creator (menor slot, empate por mint). "Juntas" = la unión de las
  tres. Mayhem: la misma regla dentro de los mayhem, aparte y sin grupos D2.
- Grupos: todos; pasan D2 (D2 falso); descartados (D2 verdadero). D2 congelado;
  los D2 n/e (adenda 1) solo entran en "todos".
- Serie de precios: puntos de `price_points` (trades), ordenados por
  (timestamp, slot) como en la versión (a). Precio = virtual_quote / virtual_token.
- Comisión: f = fee_bps del TradeEvent de T_entry2 (entry2), en la entrada y en la
  salida. Neto = (P_salida / P_entrada)(1 − f)² − 1, como entry2::net_return.
  Bruto = P_salida / P_entrada − 1.
- Latencia L ∈ {0, 2, 5, 10} s. Instante de entrada te = T_entry2.ts + L.
  - L = 0: P_entrada = entry_price de entry2 (estado antes del trade de T_entry2).
    El trayecto son los puntos con (ts, slot) ≥ (ts, slot) de T_entry2: incluye el
    propio trade de T_entry2, igual que la versión (a).
  - L > 0: P_entrada = último punto con ts ≤ te (regla de la versión (a)). El
    trayecto son los puntos con ts > te.
- Horizontes fijos h ∈ {5, 30, 120} s: P_salida = último punto con ts ≤ te + h
  (versión (a)). n/e si la curva se completa en o antes de te + h, si te + h queda
  fuera del rango descargado o si no se conoce la comisión. Con L = 0 es
  exactamente entry2::return_at (se comprueba a +10/+30/+60 min).
- Reglas por camino: TP ∈ {10, 20, 30} %, SL ∈ {10, 20 %, sin stop}, tiempo
  T ∈ {30, 120, 300} s; 27 reglas.
  - Se recorre el trayecto con ts ≤ te + T. En el primer punto con
    P/P_entrada − 1 ≥ TP se sale por TP, y en el primero con ≤ −SL se sale por SL.
    El umbral se mide sobre el precio BRUTO. Se sale al precio de ese trade, no al
    umbral, y el resultado es NETO.
  - Si no se cruza ninguno, se sale por tiempo: último punto del trayecto con
    ts ≤ te + T, o P_entrada si no hay ninguno.
  - n/e con las mismas reglas que el horizonte fijo, con te + T como salida.
- "No salible": ningún trade entre la entrada y te + T, sin contar el propio
  trade de T_entry2 cuando L = 0. En una curva siempre se puede vender al precio
  de la curva, que sin trades es P_entrada, así que "con no salibles" les asigna
  el neto de solo pagar la comisión. "Sin" los excluye.
- Por combinación: n, tasa de acierto (neto > 0), media y mediana del neto e
  IC 95 % por bootstrap (2 000 réplicas, semilla 20260930, random.Random de
  Python sembrado de nuevo en cada combinación, remuestreo simple de tokens,
  percentiles 2.5–97.5) de la media y de la mediana.
"""
import argparse, json, random, statistics, subprocess
import cribado_posthoc as cp
import etapa0_latencia as e0

LATS = [0, 2, 5, 10]
HS = [5, 30, 120]
TPS = [0.10, 0.20, 0.30]
SLS = [0.10, 0.20, None]
TS = [30, 120, 300]
SEED, REPS = 20260930, 2000


def load(marxol, db):
    toks, _ = cp.load(marxol, db)
    n1, pd_bad, ret_bad = cp.controls(marxol, db, toks)
    v3 = json.loads(subprocess.run([marxol, "--db", db, "entry2", "--check", "--json", "--tramo", "validacion3"],
                                   check=True, capture_output=True, text=True).stdout)
    for t in toks.values():
        if e0.V3_FIRST <= t["slot"] <= e0.V3_LAST:
            t["tramo"] = "V3"
    for r in v3:
        t = toks[r["mint"]]
        t["e"] = r["entry2"]
        fill = t["e"]["fill_pct"]
        t["sig"] = {"H4'": t["e"]["dev_sold"], "H6'": None if fill is None else fill >= 13.65, "H7'": t["e"]["pairs"] >= 1}
        t["ret"] = {h: cp.ret_a(t, h) for h in (600, 1800, 3600)}
        assert e0.d2(t["sig"]) == r["d2"]
    pops = {}
    for tr in ("V1", "V2", "V3"):
        withe = [t for t in toks.values() if t["tramo"] == tr and t["e"] is not None]
        for t in withe:
            t["d2"] = e0.d2(t["sig"])
        pops[tr] = (e0.one_per_creator([t for t in withe if not t["mayhem"]]),
                    e0.one_per_creator([t for t in withe if t["mayhem"]]))
    return toks, pops, (n1, pd_bad, ret_bad)


def entry(t, lat):
    """(te, P_entrada, trayecto) o None si n/e en la entrada."""
    e = t["e"]
    if e["fee_bps"] is None or t["horizon"] is None:
        return None
    te = e["entry_ts"] + lat
    covered = t["t0"] + t["horizon"] - 1
    if covered < te or (t["completed_at"] is not None and t["completed_at"] <= te):
        return None
    pts = t["pts"]
    if lat == 0:
        path = [p for p in pts if (p[0], p[1]) >= (e["entry_ts"], e["entry_slot"])]
        return te, e["entry_price"], path
    before = [p for p in pts if p[0] <= te]
    return te, before[-1][2], [p for p in pts if p[0] > te]


def evaluable(t, until):
    covered = t["t0"] + t["horizon"] - 1
    return not (covered < until or (t["completed_at"] is not None and t["completed_at"] <= until))


def net(t, pin, pout):
    f = t["e"]["fee_bps"] / 1e4
    return pout / pin * (1 - f) ** 2 - 1


def fixed(t, lat, h):
    en = entry(t, lat)
    if en is None:
        return None
    te, pin, _ = en
    if not evaluable(t, te + h):
        return None
    before = [p for p in t["pts"] if p[0] <= te + h]
    pout = before[-1][2]
    return pout / pin - 1, net(t, pin, pout)


def rule(t, lat, tp, sl, T):
    """(neto, motivo, no_salible) o None."""
    en = entry(t, lat)
    if en is None:
        return None
    te, pin, path = en
    lim = te + T
    if not evaluable(t, lim):
        return None
    inwin = [p for p in path if p[0] <= lim]
    if lat == 0:
        # El trade de T_entry2 es el primero de su slot en orden de ejecución, pero en (ts, slot) puede empatar
        # con otros del mismo slot: "salible" = hay algún punto del trayecto además de uno solo de ese slot.
        no_exit = len(inwin) <= 1
    else:
        no_exit = len(inwin) == 0
    for p in inwin:
        g = p[2] / pin - 1
        if g >= tp:
            return net(t, pin, p[2]), "TP", no_exit
        if sl is not None and g <= -sl:
            return net(t, pin, p[2]), "SL", no_exit
    pout = inwin[-1][2] if inwin else pin
    return net(t, pin, pout), "tiempo", no_exit


def boot(v, rng):
    n = len(v)
    means, meds = [], []
    for _ in range(REPS):
        s = rng.choices(v, k=n)
        means.append(sum(s) / n)
        meds.append(statistics.median(s))
    means.sort()
    meds.sort()
    lo, hi = int(0.025 * REPS), int(0.975 * REPS) - 1
    return (means[lo], means[hi]), (meds[lo], meds[hi])


def stats(v):
    if not v:
        return f"n {0:3}"
    rng = random.Random(SEED)
    (ml, mh), (dl, dh) = boot(v, rng)
    n = len(v)
    hit = sum(x > 0 for x in v)
    p = lambda x: f"{100 * x:+6.1f}"
    return (f"n {n:3} acierto {100 * hit / n:5.1f} % media {p(sum(v) / n)} [{p(ml)},{p(mh)}] "
            f"mediana {p(statistics.median(v))} [{p(dl)},{p(dh)}]")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--marxol", default="target/release/marxol")
    ap.add_argument("--db", default="marxol.db")
    args = ap.parse_args()
    toks, pops, (n1, pd_bad, ret_bad) = load(args.marxol, args.db)

    tramos = [("V1", pops["V1"][0]), ("V2", pops["V2"][0]), ("V3", pops["V3"][0]),
              ("juntas", pops["V1"][0] + pops["V2"][0] + pops["V3"][0])]
    mayhem = [("V1", pops["V1"][1]), ("V2", pops["V2"][1]), ("V3", pops["V3"][1]),
              ("juntas", pops["V1"][1] + pops["V2"][1] + pops["V3"][1])]
    groups = [("todos", lambda t: True), ("pasan D2", lambda t: t["d2"] is False), ("descartados D2", lambda t: t["d2"] is True)]

    # Control: con L = 0, el horizonte fijo reproduce entry2::return_at (cp.ret_a).
    bad = tot = 0
    for _, pop in tramos[:3] + mayhem[:3]:
        for t in pop:
            for h in (600, 1800, 3600):
                f = fixed(t, 0, h)
                a = t["ret"][h]
                tot += 1
                bad += (f is None) != (a is None) or (f is not None and abs(f[1] - a) > 1e-12)

    print("REGLAS DE SALIDA CORTA — POST HOC, EXPLORATORIO, SIN PRE-REGISTRO (no confirma ni descarta nada)")
    print("Definiciones: cabecera de scripts/salidas_posthoc.py. Umbral TP/SL sobre precio bruto; resultado neto.")
    print("controles:")
    print(f"  V1 (cribado_posthoc): p&c distinto de entry2 --json {pd_bad}/{n1}; retornos distintos {ret_bad}")
    print(f"  horizonte fijo con L = 0 frente a entry2::return_at (+10/+30/+60 min): {bad} distintos de {tot}")
    for (tr, nm), (_, mh) in zip(tramos, mayhem):
        print(f"  {tr}: sin mayhem {len(nm)} (pasan {sum(t['d2'] is False for t in nm)}, descartados "
              f"{sum(t['d2'] is True for t in nm)}, D2 n/e {sum(t['d2'] is None for t in nm)}); mayhem {len(mh)}")

    n_fixed = len(tramos) * len(groups) * len(LATS) * len(HS) + len(mayhem) * len(LATS) * len(HS)
    n_rules = len(tramos) * len(groups) * len(LATS) * len(TPS) * len(SLS) * len(TS) + len(mayhem) * len(LATS) * len(TPS) * len(SLS) * len(TS)
    print(f"\nCOMBINACIONES PROBADAS: horizontes fijos {n_fixed}; reglas por camino {n_rules} (cada una con y sin no salibles: "
          f"{2 * n_rules} estimaciones). Total {n_fixed + n_rules} combinaciones, {n_fixed + 2 * n_rules} estimaciones con IC.")
    print("  Con tantas combinaciones, varias saldrán con IC lejos del resto solo por azar; ninguna se puede leer como hallazgo.")

    print("\n" + "=" * 110)
    print("1. BRUTO FRENTE A NETO EN HORIZONTES FIJOS (cuánto es comisión y cuánto movimiento de precio)")
    print("   Celda: estadísticos del neto (IC bootstrap); 'bruto' = media y mediana del retorno sin comisión;")
    print("   'comisión' = media de (bruto − neto).")
    for label, sets, gs in (("SIN MAYHEM", tramos, groups), ("MAYHEM (aparte; precio no fiable)", mayhem, groups[:1])):
        print(f"\n## {label}")
        for tr, pop in sets:
            for gname, gf in gs:
                sub = [t for t in pop if gf(t)]
                for lat in LATS:
                    for h in HS:
                        r = [x for x in (fixed(t, lat, h) for t in sub) if x is not None]
                        g = [x[0] for x in r]
                        nt = [x[1] for x in r]
                        if g:
                            br = f"bruto media {100 * sum(g) / len(g):+6.1f} mediana {100 * statistics.median(g):+6.1f}  " \
                                 f"comisión {100 * (sum(g) - sum(nt)) / len(g):5.2f} pts  "
                        else:
                            br = ""
                        print(f"  {tr:6} {gname:15} L+{lat:<2} h+{h:<3}s  {br}NETO {stats(nt)}  (n/e {len(sub) - len(r)})")

    print("\n" + "=" * 110)
    print("2. REGLAS DE SALIDA POR CAMINO (TODAS las combinaciones)")
    print("   Por regla: 'con' = todos los evaluables (los no salibles salen a P_entrada y pagan la comisión);")
    print("   'sin' = excluyendo los no salibles; salidas = TP/SL/tiempo en 'con'.")
    for label, sets, gs in (("SIN MAYHEM", tramos, groups), ("MAYHEM (aparte; precio no fiable)", mayhem, groups[:1])):
        print(f"\n## {label}")
        for tr, pop in sets:
            for gname, gf in gs:
                sub = [t for t in pop if gf(t)]
                print(f"\n  [{tr} · {gname} · {len(sub)} tokens]")
                for lat in LATS:
                    for T in TS:
                        for sl in SLS:
                            for tp in TPS:
                                r = [x for x in (rule(t, lat, tp, sl, T) for t in sub) if x is not None]
                                ne = len(sub) - len(r)
                                con = [x[0] for x in r]
                                sin = [x[0] for x in r if not x[2]]
                                nx = sum(x[2] for x in r)
                                why = {k: sum(x[1] == k for x in r) for k in ("TP", "SL", "tiempo")}
                                sls = "—" if sl is None else f"{100 * sl:.0f}"
                                print(f"    L+{lat:<2} T{T:<3} SL{sls:>3} TP{100 * tp:.0f}  con {stats(con)}")
                                print(f"    {'':22}sin {stats(sin)}  | no salibles {nx} ({cp.pc(nx, len(r))}), "
                                      f"n/e {ne}, salidas TP {why['TP']} SL {why['SL']} tiempo {why['tiempo']}")

    print("\n" + "=" * 110)
    print("LÍMITES")
    print(f"- Comparaciones múltiples: {n_fixed + n_rules} combinaciones ({n_fixed + 2 * n_rules} estimaciones) sobre los mismos")
    print("  tokens. Los IC son por combinación, sin corrección: elegir la mejor regla de esta tabla sobreestima su")
    print("  resultado. Cualquier regla que interese tiene que pre-registrarse y evaluarse en una franja nueva.")
    print("- Post hoc y exploratorio: V1, V2 y V3 ya se han mirado (V1 y V2 para elegir D2).")
    print("- Optimismo del precio de salida: se sale al precio del trade que cruza el umbral, como si la orden se")
    print("  ejecutara justo después de ese trade y antes que cualquier otro. En la práctica la salida llega con")
    print("  latencia, y el precio puede haber vuelto. Con L = 0, el propio trade de T_entry2 forma parte del trayecto")
    print("  y puede disparar un TP (es el máximo en el 47 % de los tokens, sección 8).")
    print("- Orden dentro de un segundo y slot: los puntos se ordenan por (timestamp, slot), como en la versión (a). Con")
    print("  varios trades en el mismo slot, cuál cruza primero el umbral no está determinado.")
    print("- Latencia real: L ∈ {0, 2, 5, 10} s es una latencia supuesta, aplicada en segundos enteros (resolución de")
    print("  blockTime). La Etapa 1 midió ~1.2 s de mediana solo para recibir el dato, sin decisión ni envío de orden.")
    print("  Tampoco se modela la latencia de la salida.")
    print("- Sin impacto de precio: posición marginal. Comprar y vender mueven la curva; con una posición real la")
    print("  entrada es más cara y la salida más barata, y el efecto es mayor en curvas con pocas reservas.")
    print("- Comisión: la del TradeEvent de T_entry2 (entry2), igual a la entrada y a la salida. Sin cashback, buyback")
    print("  ni holder rewards, y sin la comisión de prioridad o de red de la transacción.")
    print("- Solo tres franjas de 8 minutos (2026-09-28, 2026-09-30, 2026-10-01). No generaliza a otros momentos.")
    print("- Mayhem: su precio cambia fuera de los trades; sus retornos no son fiables.")


if __name__ == "__main__":
    main()
