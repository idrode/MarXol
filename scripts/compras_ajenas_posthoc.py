#!/usr/bin/env python3
"""Compras de cuentas ajenas tras la creación — POST HOC, DESCRIPTIVO, SIN VEREDICTO.

Solo marxol.db (y marxol_fase3.db para R1), en solo lectura y sin red. No abre
marxol_vivo.db ni lee ningún retorno. Nada de esto decide: si algo se usa luego,
va con adenda del pre-registro y se valida en el bloque B.

Población: tokens con ventana temprana completa (300 s), sin mayhem, un token por
creator (menor slot, empate por mint), de todo marxol.db (piloto, V1, V2, V3, margen).

Definiciones (fijadas antes de mirar las cifras):
- Identidades del creador: `creator` y pagador (`user`) del CreateEvent.
- Compra ajena: TradeEvent con is_buy = 1 de una cuenta que no es ninguna de las dos,
  en un slot POSTERIOR al de creación ("tras el slot de creación").
- Tiempos medidos desde el timestamp de la creación (blockTime, resolución 1 s):
  60 s = timestamp < t0 + 60; 5 min = timestamp < t0 + 300 (la ventana guardada).
- Tiempo hasta la primera compra ajena: se censura en 300 s. Los cuantiles se toman
  sobre todos los tokens con los censurados como "> 300 s" (la censura es común, así
  que es el cuantil de Kaplan-Meier); se da también la mediana solo entre los que la tienen.
- Estratos:
  * compra del creador en el slot de creación, % del supply (10^15 unidades):
    0 / (0, 2] / (2, 5] / (5, 10] / > 10 %;
  * otras cuentas compran en el mismo slot de creación (sí/no);
  * R1 de la adenda 1 (previos del pagador en 7 d ≥ 5, marxol_fase3.db); truncado con
    < 5 o sin cubrir = n/e; piloto y margen no tienen C' = sin dato.
- IC 95 % de Wilson en cada porcentaje.

Uso: python3 -B scripts/compras_ajenas_posthoc.py [--db marxol.db] [--fase3 marxol_fase3.db]
"""
import argparse
import math
import os
import sqlite3

SUPPLY = 1e15
DAYS = 7
PILOT_LAST, V1_LAST, V2_FIRST, V2_LAST = 451_340_397, 451_342_089, 451_810_013, 451_811_800
V3_FROM, V3_TO = 1790845200, 1790845680
SIZE_BINS = [("0 %", lambda x: x == 0), ("(0, 2] %", lambda x: 0 < x <= 2), ("(2, 5] %", lambda x: 2 < x <= 5),
             ("(5, 10] %", lambda x: 5 < x <= 10), ("> 10 %", lambda x: x > 10)]


def tramo(slot, t0):
    if slot <= PILOT_LAST:
        return "piloto"
    if slot <= V1_LAST:
        return "V1"
    if V2_FIRST <= slot <= V2_LAST:
        return "V2"
    if V3_FROM <= t0 < V3_TO:
        return "V3"
    return "margen"


def wilson(k, n, z=1.959964):
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return c - h, c + h


def share(k, n):
    if n == 0:
        return "0/0"
    lo, hi = wilson(k, n)
    return f"{k}/{n} = {100 * k / n:.1f} % [{100 * max(lo, 0):.1f}–{100 * min(hi, 1):.1f}]"


def quant(xs, p):
    """Cuantil inferior (orden ⌈p·n⌉); None = censurado (> 300 s)."""
    xs = sorted(xs, key=lambda x: math.inf if x is None else x)
    return xs[max(math.ceil(p * len(xs)) - 1, 0)]


def fmt_t(x):
    return "> 300 s" if x is None else f"{x} s"


def r1_map(f3):
    """mint → 'R1 sí' / 'R1 no' / 'n/e' según la adenda 1 (mínimo de previos en 7 d)."""
    out = {}
    for mint, n, complete, covered in f3.execute(f"""
            SELECT t.mint,
                   (SELECT COUNT(*) FROM ph_payer_sigs p JOIN ph_ma_sigs m ON m.signature = p.signature
                    WHERE p.payer = t.payer AND m.failed = 0 AND m.signature != t.signature
                      AND m.block_time >= t.t0 - {DAYS * 86400} AND m.block_time < t.t0),
                   (s.reached_end = 1 OR s.oldest_bt < t.t0 - {DAYS * 86400}),
                   (SELECT MIN(block_time) FROM ph_ma_sigs) <= t.t0 - {DAYS * 86400}
            FROM ph_tokens t JOIN ph_payer_scan s ON s.payer = t.payer"""):
        if not covered:
            out[mint] = "n/e"
        elif n >= 5:
            out[mint] = "R1 sí (≥ 50)" if n >= 50 else "R1 sí (5–49)"
        elif not complete:
            out[mint] = "n/e"
        else:
            out[mint] = "R1 no"
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default="marxol.db")
    ap.add_argument("--fase3", default="marxol_fase3.db")
    args = ap.parse_args()
    db = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    f3 = sqlite3.connect(f"file:{args.fase3}?mode=ro", uri=True) if os.path.exists(args.fase3) else None
    r1 = r1_map(f3) if f3 else {}

    rows = db.execute("""SELECT c.mint, c.creator, c.user, c.slot, c.timestamp FROM creations c
                         JOIN trade_windows w ON w.mint = c.mint AND w.complete = 1 AND w.window_secs >= 300
                         WHERE COALESCE(c.is_mayhem_mode, 0) = 0 ORDER BY c.slot, c.mint""").fetchall()
    seen, pop = set(), []
    for r in rows:
        if r[1] not in seen:
            seen.add(r[1])
            pop.append(r)

    toks = []
    for mint, creator, user, cslot, t0 in pop:
        ids = {creator, user}
        tr = db.execute("""SELECT user, slot, timestamp, token_amount FROM early_trades
                           WHERE mint = ? AND is_buy = 1 AND timestamp < ? + 300""", (mint, t0)).fetchall()
        cre = sum(ta or 0 for u, s, _, ta in tr if u in ids and s == cslot)
        same = any(u not in ids and s == cslot for u, s, _, _ in tr)
        after = [(ts - t0, u) for u, s, ts, _ in tr if u not in ids and s > cslot]
        r = r1.get(mint)
        toks.append(dict(
            tramo=tramo(cslot, t0),
            b60=any(d < 60 for d, _ in after),
            b300=bool(after),
            u3=len({u for _, u in after}) >= 3,
            t1=min((d for d, _ in after), default=None),
            size=100 * cre / SUPPLY,
            same=same,
            r1=("sin dato" if r is None else r),
        ))

    print("COMPRAS DE CUENTAS AJENAS TRAS EL SLOT DE CREACIÓN — POST HOC, DESCRIPTIVO, SIN VEREDICTO")
    print("Solo marxol.db (+ marxol_fase3.db para R1), sin red, sin retornos, sin marxol_vivo.db.")
    print("Población: ventana temprana completa, sin mayhem, un token por creator (menor slot, empate por mint).")
    print("Compra ajena = is_buy de una cuenta ≠ creator y ≠ pagador, en slot > slot de creación; tiempo desde t0")
    print("(blockTime, 1 s). Porcentajes con IC 95 % de Wilson. 1.ª compra ajena censurada en 300 s.\n")

    def block(name, ts):
        n = len(ts)
        if n == 0:
            return
        t1 = [t["t1"] for t in ts]
        have = sorted(x for x in t1 if x is not None)
        med_have = (f"mediana {quant(have, .5)} s, p90 {quant(have, .9)} s (n = {len(have)})"
                    if have else "n/a")
        print(f"  {name} — n = {n}")
        print(f"    ≥ 1 compra ajena en 60 s    {share(sum(t['b60'] for t in ts), n)}")
        print(f"    ≥ 1 compra ajena en 5 min   {share(sum(t['b300'] for t in ts), n)}")
        print(f"    ≥ 3 cuentas ajenas en 5 min {share(sum(t['u3'] for t in ts), n)}")
        print(f"    1.ª compra ajena: mediana {fmt_t(quant(t1, .5))}, p90 {fmt_t(quant(t1, .9))} "
              f"(todos, censura > 300 s);"
              f" entre los que la tienen: {med_have}")

    block("TODOS", toks)
    print()
    print("POR TRAMO (contexto; el piloto es calibración)")
    for g in ("piloto", "V1", "V2", "V3", "margen"):
        block(g, [t for t in toks if t["tramo"] == g])
    print()
    print("POR COMPRA DEL CREADOR EN EL SLOT DE CREACIÓN (% del supply, creator + pagador)")
    for name, f in SIZE_BINS:
        block(name, [t for t in toks if f(t["size"])])
    print()
    print("POR OTRAS CUENTAS COMPRANDO EN EL MISMO SLOT DE CREACIÓN")
    block("sí", [t for t in toks if t["same"]])
    block("no", [t for t in toks if not t["same"]])
    print()
    print("POR R1 (adenda 1: previos del pagador en 7 d ≥ 5; mínimo; solo V1–V3 tienen C')")
    block("R1 sí (total)", [t for t in toks if t["r1"].startswith("R1 sí")])
    for g in ("R1 sí (5–49)", "R1 sí (≥ 50)", "R1 no", "n/e", "sin dato"):
        block(g, [t for t in toks if t["r1"] == g])


if __name__ == "__main__":
    main()
