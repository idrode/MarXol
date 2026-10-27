"""Tarea E (2026-10-23): cuánto de C1 explican ya H6' (llenado de D2) y el
progreso de la curva en T_entry2. Post hoc, exploratorio, sin red; solo
marxol.db vía `marxol banco-fase-c --volcado DIR` (DIR/tokens.tsv).
Uso: python3 -B scripts/c1_vs_h6_progreso.py DIR
"""
import math
import random
import sys

import potencia_adenda2 as p


def ranks(v):
    o = sorted(range(len(v)), key=lambda i: v[i])
    r = [0.0] * len(v)
    i = 0
    while i < len(o):
        j = i
        while j + 1 < len(o) and v[o[j + 1]] == v[o[i]]:
            j += 1
        for k in range(i, j + 1):
            r[o[k]] = (i + j) / 2 + 1
        i = j + 1
    return r


def pearson(x, y):
    mx, my = sum(x) / len(x), sum(y) / len(y)
    sxy = sum((a - mx) * (b - my) for a, b in zip(x, y))
    return sxy / math.sqrt(sum((a - mx) ** 2 for a in x) * sum((b - my) ** 2 for b in y))


def spearman(x, y):
    return pearson(ranks(x), ranks(y))


def ci_spearman(x, y, rng, reps=2000):
    idx = range(len(x))
    vals = sorted(spearman(*zip(*[(x[i], y[i]) for i in rng.choices(idx, k=len(x))])) for _ in range(reps))
    return vals[int(0.025 * reps)], vals[int(0.975 * reps)]


def main():
    hdr, *rows = [l.rstrip("\n").split("\t") for l in open(sys.argv[1] + "/tokens.tsv")]
    rows = [dict(zip(hdr, r)) for r in rows]
    ev = [r for r in rows if r["fill_pct"] != "" and r["h6"] != ""]
    print("C1 FRENTE A H6' (LLENADO DE D2) Y AL PROGRESO DE LA CURVA EN T_entry2 — POST HOC, SIN RED, marxol.db V1–V3")
    print(f"población: {len(rows)} tokens (la de banco-fase-c); con H6' evaluable: {len(ev)} (n/e si V0 ≠ 1.073e15)")
    rng = random.Random(20260930)
    pct = [float(r["creador_pct"]) for r in ev]
    fill = [float(r["fill_pct"]) for r in ev]
    prog = [float(r["progreso"]) for r in ev]
    print("\n-- Correlaciones de Spearman (IC bootstrap por token = por creator, 2000 réplicas, semilla 20260930) --")
    for a, xa, b, xb in [("tenencia del creador", pct, "llenado H6'", fill), ("tenencia del creador", pct, "progreso", prog), ("llenado H6'", fill, "progreso", prog)]:
        lo, hi = ci_spearman(xa, xb, rng)
        print(f"{a} vs {b}: rho = {spearman(xa, xb):+.3f} [{lo:+.3f}, {hi:+.3f}]")
    c1 = [r["c1"] == "1" for r in ev]
    h6 = [r["h6"] == "1" for r in ev]
    a = sum(x and y for x, y in zip(c1, h6)); b = sum(x and not y for x, y in zip(c1, h6))
    c = sum(not x and y for x, y in zip(c1, h6)); d = sum(not x and not y for x, y in zip(c1, h6))
    phi = (a * d - b * c) / math.sqrt((a + b) * (c + d) * (a + c) * (b + d))
    print("\n-- Solapamiento C1 × H6' --")
    print(f"           H6' sí  H6' no")
    print(f"C1 sí      {a:6}  {b:6}")
    print(f"C1 no      {c:6}  {d:6}")
    print(f"phi = {phi:+.3f}; C1 con H6' = {a}/{a + b} = {100 * a / (a + b):.1f} %; H6' con C1 = {a}/{a + c} = {100 * a / (a + c):.1f} %")
    print(f"C1 con D2 (descartado) = {sum(r['c1'] == '1' and r['d2'] == '1' for r in ev)}/{a + b}")
    print("tenencia mínima que fuerza llenado: 5 % del supply = 5e13 tokens = 6.30 % de 793.1e12 (las compras del creador ya llenan la curva)")
    print("\n-- Retorno (a) a +30 min: C1 dentro de H6' (descriptivo) --")
    for lab, g in [("H6' sí", True), ("H6' no", False)]:
        s = [float(r["ret30"]) for r in ev if (r["h6"] == "1") == g and r["c1"] == "0"]
        k = [float(r["ret30"]) for r in ev if (r["h6"] == "1") == g and r["c1"] == "1"]
        if s and k:
            print(f"{lab}: n sin C1 {len(s)}, con C1 {len(k)} · mediana sin {100 * p.median(s):+.1f} · con {100 * p.median(k):+.1f} · diferencia {100 * (p.median(s) - p.median(k)):+.1f} pts · MW unilateral p = {p.mw_greater(s, k):.3g}")
        else:
            print(f"{lab}: n sin C1 {len(s)}, con C1 {len(k)} · sin contraste")
    print("\n-- Varianza de rangos de la tenencia explicada (R² de rangos, regresión lineal sobre rangos) --")
    rp, rf, rr = ranks(pct), ranks(fill), ranks(prog)
    print(f"por el llenado: R² = {pearson(rp, rf) ** 2:.3f}; por el progreso: R² = {pearson(rp, rr) ** 2:.3f}")


if __name__ == "__main__":
    main()
