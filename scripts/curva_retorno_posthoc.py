#!/usr/bin/env python3
"""Curva de retorno neto desde T_entry2 y máxima subida — EXPLORACIÓN POST HOC,
SIN VEREDICTO (CLAUDE.md sección 8). Solo lee marxol.db (sin red, sin cuota).
No propone horizontes ni umbrales: solo describe.

Reproducir (desde la raíz del repo):
    cargo build --release && python3 -B scripts/curva_retorno_posthoc.py

Reglas (las de la robustez post hoc, sección 10):
- Población: los 251 tokens de validación con T_entry2; todo lo de retorno
  solo sin mayhem.
- Retorno neto = el de la validación congelada (`entry2::returns`):
  (P_salida / P_entrada) · (1 − f)² − 1, f = comisión del trade T_entry2,
  P_entrada = precio justo antes de T_entry2, P_salida = último punto de la
  curva con timestamp ≤ t_T_entry2 + Δ (orden `timestamp, slot`, como en
  Rust); n/e si la curva se completa en o antes de la salida o si la salida
  queda fuera del rango descargado.
- Máxima subida: precio máximo de la curva en los trades del slot de
  T_entry2 o posteriores (T_entry2 es el primer trade de su slot en orden de
  ejecución, así que ninguno de ese slot es anterior) hasta t_T_entry2 + 60 min,
  frente a P_entrada. n/e si la curva se completa en ese intervalo.
"""
import argparse, sqlite3, math
from robustez_posthoc import load_tokens, med

HORIZONS = [5, 15, 30, 60, 120, 300, 600, 1800, 3600]
LABELS = ["+5 s", "+15 s", "+30 s", "+1 min", "+2 min", "+5 min", "+10 min", "+30 min", "+60 min"]
SIGNALS = ["H5", "H6'", "H7'", "H8"]


def pctl(v, p):
    """Percentil por rango más cercano (v ordenada)."""
    return v[min(len(v) - 1, max(0, math.ceil(p / 100 * len(v)) - 1))]


def fmt(x):
    return "  n/a" if x is None or math.isnan(x) else f"{100 * x:+.1f}%"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--marxol", default="target/release/marxol")
    ap.add_argument("--db", default="marxol.db")
    args = ap.parse_args()
    toks, _ = load_tokens(args.marxol, args.db)
    db = sqlite3.connect(args.db)
    horizon = dict(db.execute("select mint, max(horizon_secs) from price_windows where complete = 1 group by mint"))

    rows, mismatch, ambiguous = [], 0, [0] * len(HORIZONS)
    for t in toks:
        if t["mayhem"]:
            continue
        e = t["e"]
        pts = db.execute(
            "select timestamp, slot, quote_reserves, token_reserves from price_points "
            "where mint = ? and kind = 'trade' order by timestamp, slot",
            (t["mint"],),
        ).fetchall()
        pts = [(ts, sl, q / v) for ts, sl, q, v in pts]
        covered = t["t0"] + horizon[t["mint"]] - 1
        f = e["fee_bps"] / 1e4
        ret = []
        for k, h in enumerate(HORIZONS):
            x = e["entry_ts"] + h
            if covered < x or (t["completed_at"] is not None and t["completed_at"] <= x):
                ret.append(None)
                continue
            before = [p for p in pts if p[0] <= x]
            last = before[-1]  # max_by_key de Rust devuelve el último de los empatados
            same = {p[2] for p in before if p[0] == last[0] and p[1] == last[1]}
            ambiguous[k] += len(same) > 1
            ret.append(last[2] / e["entry_price"] * (1 - f) ** 2 - 1)
        for k, h in ((6, 600), (7, 1800), (8, 3600)):
            j = [600, 1800, 3600].index(h)
            a, b = ret[k], t["ret"][j]
            if (a is None) != (b is None) or (a is not None and abs(a - b) > 1e-12):
                mismatch += 1
        end = e["entry_ts"] + 3600
        if t["completed_at"] is not None and t["completed_at"] <= end:
            mx = None
        else:
            post = [p for p in pts if p[1] >= e["entry_slot"] and p[0] <= end]
            top = max(p[2] for p in post)
            first_top = next(p for p in post if p[2] == top)
            mx = (top / e["entry_price"], first_top[0] - e["entry_ts"], (top / e["entry_price"]) * (1 - f) ** 2 - 1,
                  first_top[1] == e["entry_slot"])
        rows.append(dict(t=t, ret=ret, mx=mx))

    print("EXPLORACIÓN POST HOC, SIN VEREDICTO. Tokens sin mayhem:", len(rows))
    print("control: retornos +10/+30/+60 distintos de `marxol entry2 --json`:", mismatch)
    print("tokens cuyo punto de salida tiene varios precios en el mismo segundo y slot (orden interno no determinado):")
    print("  " + "  ".join(f"{l} {a}" for l, a in zip(LABELS, ambiguous)))

    def curve(label, sel):
        print(f"\n{label}")
        for k, l in enumerate(LABELS):
            v = [r["ret"][k] for r in sel if r["ret"][k] is not None]
            pos = sum(x > 0 for x in v)
            print(f"  {l:8} n={len(v):3}  mediana {fmt(med(v) if v else None)}  "
                  f"retorno > 0: {pos}/{len(v)} ({fmt(pos / len(v) if v else None)})")

    print("\n=== 1. RETORNO NETO MEDIANO Y FRACCIÓN > 0 SEGÚN EL HORIZONTE ===")
    curve("todos (sin mayhem)", rows)
    for s in SIGNALS:
        curve(f"{s} = señal", [r for r in rows if r["t"]["sig"][s] is True])
        curve(f"{s} = sin señal", [r for r in rows if r["t"]["sig"][s] is False])

    print("\n=== 2. MÁXIMA SUBIDA ALCANZABLE TRAS T_entry2 (hasta +60 min, sin mayhem) ===")
    ok = [r for r in rows if r["mx"] is not None]
    print(f"evaluables {len(ok)} de {len(rows)} (n/e: curva completada antes de +60 min: "
          f"{', '.join(r['t']['mint'][:8] + '…' for r in rows if r['mx'] is None) or '-'})")

    def dist(label, sel):
        if not sel:
            return
        m = sorted(r["mx"][0] for r in sel)
        net = sorted(r["mx"][2] for r in sel)
        tt = sorted(r["mx"][1] for r in sel)
        print(f"\n{label} (n={len(sel)})")
        print("  múltiplo bruto máx/entrada: " + "  ".join(f"p{p} {pctl(m, p):.3f}" for p in (10, 25, 50, 75, 90)) + f"  máx {m[-1]:.2f}")
        print("  retorno neto en el máximo: " + "  ".join(f"p{p} {fmt(pctl(net, p))}" for p in (10, 25, 50, 75, 90))
              + f"   > 0 en {sum(x > 0 for x in net)}/{len(net)}")
        print("  s desde T_entry2 hasta el máximo: " + "  ".join(f"p{p} {pctl(tt, p)}" for p in (10, 25, 50, 75, 90)))
        edges = [0, 5, 15, 30, 60, 120, 300, 600, 1800, 3600]
        at_entry = sum(r["mx"][3] for r in sel)
        cnt = []
        for i, hi in enumerate(edges):
            lo = edges[i - 1] if i else -1
            cnt.append(sum(lo < x <= hi for x in tt))
        names = ["0 s", "1–5 s", "6–15 s", "16–30 s", "31–60 s", "1–2 min", "2–5 min", "5–10 min", "10–30 min", "30–60 min"]
        print("  cuándo: " + "  ".join(f"{n} {c}" for n, c in zip(names, cnt)) + f"   (máximo en el propio slot de T_entry2: {at_entry})")

    dist("todos (sin mayhem)", ok)
    for s in SIGNALS:
        dist(f"{s} = señal", [r for r in ok if r["t"]["sig"][s] is True])
        dist(f"{s} = sin señal", [r for r in ok if r["t"]["sig"][s] is False])


if __name__ == "__main__":
    main()
