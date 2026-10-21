#!/usr/bin/env python3
"""Señal de creador repetido: EXPLORACIÓN POST HOC, SIN PRE-REGISTRO, SIN VEREDICTO.
No cambia ninguna definición ni veredicto congelado. Los p-valores e intervalos
son descriptivos: V1, V2 y V3 ya se han visto, así que nada de aquí decide.
Sin red ni cuota. Lee marxol.db y llama a `marxol entry2 --check --json`
(código congelado) como cribado_posthoc.py.

Reproducir (desde la raíz del repo):
    cargo build --release && python3 -B scripts/creador_senal_explora.py > resultados/creador_senal_explora.txt
Aviso: `entry2` abre la base con Store::open (aplica el esquema); para no
tocar una copia de solo lectura, pasar --db con una copia.

Definiciones (fijadas en este script antes de calcular los retornos por grupo):
- Franjas: F1 = piloto + V1 (2026-09-28, 377 creaciones), F2 = V2 (400),
  F3 = V3 + margen (238). Tokens evaluados: los de V1 (337), V2 (400) y
  V3 (196, por timestamp); el piloto y el margen solo cuentan como historial.
- Tokens comparados: sin mayhem, con T_entry2, D2 definido (v3::d2, adenda 1)
  y retorno neto (a) a +30 min no n/e. Todos los tokens, no uno por creator:
  con un token por creator (el de menor slot) la señal S_t0 es 0 por construcción.
- Señales (identidad = CreateEvent.creator, salvo S_user):
  - S (principal): el creator tiene otra creación visible en la franja con
    slot < slot de T_entry2 del token.
  - S_t0: otra creación con slot < slot de creación del token (H11 en la franja).
  - S_user: como S, con el payer (`user`) como identidad.
  - S_mismo_simbolo / S_otro_simbolo: S partida según si alguna de esas
    creaciones previas tiene el mismo símbolo (fábrica de un mismo símbolo).
- Resultado: retorno neto (a) desde T_entry2 a +30 min (cribado_posthoc.ret_a,
  comprobado contra entry2 --json).
- Comparación dentro de los que pasan D2 y dentro de los descartados:
  con señal frente a sin señal. Diferencia de medianas (con − sin).
  Mann-Whitney unilateral "con > sin" (dirección de H11: el repetidor tiene
  menos pump-y-caída) con la aproximación de entry2::mann_whitney_greater, y
  también "con < sin" como contraste. Bootstrap 2 000 réplicas, semilla
  20260930, percentiles 2.5–97.5, por clúster de creator (los tokens con
  señal son por definición de creators con varios tokens) y, aparte, simple.
- Potencia: una celda con < 20 tokens en algún grupo se marca "baja potencia"
  (mismo mínimo que el criterio 1 de V3); con < 3 no se calcula.
"""
import argparse, collections, math, random
import cribado_posthoc as cp

V3_FROM, V3_TO = 1790845200, 1790845680
SEED, B, H = 20260930, 2000, 1800


def franja(t):
    if t["slot"] <= cp.PILOT_LAST:
        return "piloto"
    if t["slot"] <= cp.V1_LAST:
        return "V1"
    if cp.V2_FIRST <= t["slot"] <= cp.V2_LAST:
        return "V2"
    if V3_FROM <= t["t0"] < V3_TO:
        return "V3"
    return "margen V3"


BURST = {"piloto": 1, "V1": 1, "V2": 2, "V3": 3, "margen V3": 3}


def d2(h4, h6, h7):
    """v3::d2, con la adenda 1 de la validación 3."""
    k = h4 + h7
    if h6 is not None:
        return k + h6 >= 2
    return {2: True, 0: False}.get(k)


def erfc(x):
    """entry2::erfc (Numerical Recipes)."""
    z = abs(x)
    t = 1 / (1 + 0.5 * z)
    r = t * math.exp(-z * z - 1.26551223 + t * (1.00002368 + t * (0.37409196 + t * (0.09678418 + t * (-0.18628806
        + t * (0.27886807 + t * (-1.13520398 + t * (1.48851587 + t * (-0.82215223 + t * 0.17087277)))))))))
    return r if x >= 0 else 2 - r


def mw_greater(x, y):
    """entry2::mann_whitney_greater: p de "x mayor que y"."""
    if not x or not y:
        return None
    n1, n2 = len(x), len(y)
    allv = sorted([(v, True) for v in x] + [(v, False) for v in y], key=lambda a: a[0])
    n, r1, ties, i = len(allv), 0.0, 0.0, 0
    while i < n:
        j = i
        while j + 1 < n and allv[j + 1][0] == allv[i][0]:
            j += 1
        rank, k = (i + j) / 2 + 1, j - i + 1
        ties += k ** 3 - k
        r1 += sum(1 for a in allv[i:j + 1] if a[1]) * rank
        i = j + 1
    u = r1 - n1 * (n1 + 1) / 2
    nn = n1 + n2
    var = n1 * n2 / 12 * ((nn + 1) - ties / (nn * (nn - 1)))
    if var <= 0:
        return 1.0
    return min(0.5 * erfc((u - n1 * n2 / 2 - 0.5) / math.sqrt(var) / math.sqrt(2)), 1.0)


def mdiff(pop, key):
    a = [t["r30"] for t in pop if t[key]]
    b = [t["r30"] for t in pop if not t[key]]
    if not a or not b:
        return None
    return cp.med(a) - cp.med(b)


def boot(pop, key, cluster):
    """replica::bootstrap / robustez_posthoc.resamples: misma semilla y orden."""
    rng = random.Random(SEED)
    vals, drop = [], 0
    if cluster:
        cl = collections.defaultdict(list)
        for t in pop:
            cl[t["creator"]].append(t)
        keys = sorted(cl)
    for _ in range(B):
        if cluster:
            rs = []
            for _ in keys:
                rs += cl[keys[rng.randrange(len(keys))]]
        else:
            rs = [pop[rng.randrange(len(pop))] for _ in pop]
        v = mdiff(rs, key)
        if v is None:
            drop += 1
        else:
            vals.append(v)
    vals.sort()
    n = len(vals)
    if n == 0:
        return None
    return vals[int(0.025 * n)], vals[min(int(0.975 * n), n - 1)], drop


def pts(x):
    return "n/a" if x is None else f"{100 * x:+.1f}"


def cell(label, pop, key):
    a = [t["r30"] for t in pop if t[key]]
    b = [t["r30"] for t in pop if not t[key]]
    head = f"  {label:<34} con {len(a):>3} (med {cp.fr(cp.med(a))}, cr. {len({t['creator'] for t in pop if t[key]})})" \
           f"  sin {len(b):>3} (med {cp.fr(cp.med(b))})"
    if min(len(a), len(b)) < 3:
        return head + "  -> < 3 en un grupo: no se calcula"
    pg, pl = mw_greater(a, b), mw_greater(b, a)
    bc, bs = boot(pop, key, True), boot(pop, key, False)
    ci = lambda c: "n/a" if c is None else f"[{pts(c[0])}, {pts(c[1])}] (desc. {c[2]})"
    low = "  BAJA POTENCIA (< 20)" if min(len(a), len(b)) < 20 else ""
    return (head + f"\n      dif. medianas {pts(cp.med(a) - cp.med(b))} pts; MW unilateral p(con > sin) = {pg:.4f}, "
            f"p(con < sin) = {pl:.4f}\n      IC 95 % clúster {ci(bc)}; simple {ci(bs)}{low}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--marxol", default="target/release/marxol")
    ap.add_argument("--db", default="marxol.db")
    args = ap.parse_args()
    toks, _ = cp.load(args.marxol, args.db)
    n1, pd_bad, ret_bad = cp.controls(args.marxol, args.db, toks)
    v3rows = {}
    for r in __import__("json").loads(__import__("subprocess").run(
            [args.marxol, "--db", args.db, "entry2", "--check", "--json", "--tramo", "validacion3"],
            check=True, capture_output=True, text=True).stdout):
        v3rows[r["mint"]] = r
        toks[r["mint"]]["e"] = r["entry2"]

    by = collections.defaultdict(list)
    for t in toks.values():
        t["fr"] = franja(t)
        by[(BURST[t["fr"]], t["creator"])].append(t)
    by_user = collections.defaultdict(list)
    for t in toks.values():
        by_user[(BURST[t["fr"]], t["user"])].append(t)
    sym = {m: s for m, s in cp.sqlite3.connect(f"file:{args.db}?mode=ro", uri=True).execute(
        "select mint, symbol from creations")}

    rows, d2_bad, same_slot = [], 0, 0
    for t in toks.values():
        e = t["e"]
        if t["fr"] not in ("V1", "V2", "V3") or e is None:
            continue
        fill = e["fill_pct"]
        h4, h6, h7 = bool(e["dev_sold"]), None if fill is None else fill >= 13.65, e["pairs"] >= 1
        t["d2"] = d2(h4, h6, h7)
        if t["fr"] == "V3" and t["d2"] != v3rows[t["mint"]]["d2"]:
            d2_bad += 1
        es = e["entry_slot"]
        others = [o for o in by[(BURST[t["fr"]], t["creator"])] if o["mint"] != t["mint"]]
        prev = [o for o in others if o["slot"] < es]
        same_slot += any(o["slot"] == t["slot"] for o in others)
        t["S"] = bool(prev)
        t["S_t0"] = any(o["slot"] < t["slot"] for o in others)
        t["S_user"] = any(o["mint"] != t["mint"] and o["slot"] < es for o in by_user[(BURST[t["fr"]], t["user"])])
        t["same_sym"] = any(sym.get(o["mint"]) == sym.get(t["mint"]) for o in prev)
        t["n_prev"] = len(prev)
        t["r30"] = cp.ret_a(t, H)
        rows.append(t)

    print("SEÑAL DE CREADOR REPETIDO — EXPLORACIÓN POST HOC, SIN PRE-REGISTRO, SIN VEREDICTO")
    print("V1, V2 y V3 ya se han visto: ningún p-valor ni intervalo de aquí confirma ni descarta nada.")
    print(f"controles: entry2 --json V1 = {n1} filas; p&c distinto: {pd_bad}; retorno +10/+30/+60 distinto: {ret_bad}; "
          f"D2 de V3 distinto de entry2 --tramo validacion3: {d2_bad}")
    pop3 = [t for t in rows if t["fr"] == "V3" and v3rows[t["mint"]]["in_population"] and t["r30"] is not None]
    pas = [t["r30"] for t in pop3 if t["d2"] is False]
    des = [t["r30"] for t in pop3 if t["d2"] is True]
    print(f"reproducción de V3 (población congelada): pasan {len(pas)}, descartados {len(des)}, "
          f"dif. medianas {pts(cp.med(pas) - cp.med(des))} pts (registrado en resultados/validacion3_d2.txt: comparación 46 / 28 / +19.0)")
    print(f"tokens con otra creación del mismo creator en el mismo slot de creación (orden no resuelto, cuentan "
          f"como sin S_t0): {same_slot}")

    print("\n" + "=" * 100)
    print("1. VIABILIDAD: cobertura de la señal (solo se ve la actividad del creator dentro de la franja)")
    print("=" * 100)
    for f in ("V1", "V2", "V3"):
        all_f = [t for t in toks.values() if t["fr"] == f]
        mh = [t for t in rows if t["fr"] == f and t["mayhem"]]
        ev = [t for t in rows if t["fr"] == f and not t["mayhem"]]
        cmp_ = [t for t in ev if t["d2"] is not None and t["r30"] is not None]
        rep = {c for (b, c), v in by.items() if b == BURST[f] and len(v) > 1}
        print(f"\n{f}: {len(all_f)} creaciones; {len({t['creator'] for t in all_f})} creators, {len(rep)} con > 1 "
              f"lanzamiento visible en la franja ({sum(t['creator'] in rep for t in all_f)} tokens)")
        print(f"  con T_entry2: {len(ev)} sin mayhem (+ {len(mh)} mayhem, aparte); comparables (D2 definido y "
              f"retorno +30 min): {len(cmp_)}")
        for k in ("S", "S_t0", "S_user"):
            ct = collections.Counter((t["d2"], t[k]) for t in cmp_)
            print(f"  {k:<7} pasan D2: con {ct[(False, True)]:>3} / sin {ct[(False, False)]:>3};  "
                  f"descartados: con {ct[(True, True)]:>3} / sin {ct[(True, False)]:>3}")
        s = [t for t in cmp_ if t["S"]]
        print(f"  S con mismo símbolo que un lanzamiento previo: {sum(t['same_sym'] for t in s)} de {len(s)}; "
              f"creators distintos con S: {len({t['creator'] for t in s})}; mayor creator con S: "
              f"{max(collections.Counter(t['creator'] for t in s).values(), default=0)} tokens")
        one = {}
        for t in sorted(ev, key=lambda t: (t["slot"], t["mint"])):
            one.setdefault(t["creator"], t)
        print(f"  con un token por creator (regla de V3): {len(one)} tokens, S_t0 = "
              f"{sum(t['S_t0'] for t in one.values())}, S = {sum(t['S'] for t in one.values())}")

    print("\n" + "=" * 100)
    print("2. RETORNO NETO (a) A +30 MIN DESDE T_entry2: con señal frente a sin señal (sin mayhem, todos los tokens)")
    print("=" * 100)
    comparable = [t for t in rows if not t["mayhem"] and t["d2"] is not None and t["r30"] is not None]
    for t in comparable:
        t["S_mismo_simbolo"] = t["S"] and t["same_sym"]
        t["S_otro_simbolo"] = t["S"] and not t["same_sym"]
    for f in ("V1", "V2", "V3", "juntas"):
        pf = [t for t in comparable if f == "juntas" or t["fr"] == f]
        print(f"\n--- {f} ({len(pf)} tokens; D2 pasan {sum(t['d2'] is False for t in pf)}, "
              f"descartados {sum(t['d2'] is True for t in pf)}) ---")
        for gname, g in (("pasan D2", [t for t in pf if t["d2"] is False]),
                         ("descartados por D2", [t for t in pf if t["d2"] is True])):
            print(f" {gname}:")
            for k in ("S", "S_t0", "S_user") + (("S_mismo_simbolo", "S_otro_simbolo") if f == "juntas" else ()):
                if k.startswith("S_") and k.endswith("simbolo"):
                    g2 = [t for t in g if not t["S"] or t[k]]
                    print(cell(f"{k} (vs sin S)", g2, k))
                else:
                    print(cell(k, g, k))

    print("\n" + "=" * 100)
    print("3. DESCRIPTIVO: ¿la señal se solapa con D2? (sin mayhem, comparables, juntas)")
    print("=" * 100)
    for k in ("S", "S_t0"):
        a = [t for t in comparable if t[k]]
        b = [t for t in comparable if not t[k]]
        print(f"  {k}: descartados por D2 con señal {sum(t['d2'] for t in a)}/{len(a)} "
              f"({cp.pc(sum(t['d2'] for t in a), len(a))}); sin señal {sum(t['d2'] for t in b)}/{len(b)} "
              f"({cp.pc(sum(t['d2'] for t in b), len(b))})")
        print(f"     retorno +30 med. con {cp.fr(cp.med([t['r30'] for t in a]))}, sin {cp.fr(cp.med([t['r30'] for t in b]))}; "
              f"fracción > 0: con {cp.pc(sum(t['r30'] > 0 for t in a), len(a))}, sin {cp.pc(sum(t['r30'] > 0 for t in b), len(b))}")
    dist = collections.Counter(min(t["n_prev"], 5) for t in comparable if t["S"])
    print("  lanzamientos previos visibles (con S): " + ", ".join(f"{'≥5' if k == 5 else k}: {v}" for k, v in sorted(dist.items())))
    mh = [t for t in rows if t["mayhem"] and t["r30"] is not None]
    print(f"  mayhem (aparte, sin contraste): {len(mh)} con retorno; S en {sum(t['S'] for t in mh)}; "
          f"med. con {cp.fr(cp.med([t['r30'] for t in mh if t['S']]))}, sin {cp.fr(cp.med([t['r30'] for t in mh if not t['S']]))}")


if __name__ == "__main__":
    main()
