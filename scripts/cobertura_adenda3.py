#!/usr/bin/env python3
"""Cobertura de la recolección para la adenda 3 — OPERACIÓN, SIN RETORNOS.

Lee `marxol_vivo.db` en solo lectura y SOLO tablas de operación y de entrada
(`vivo_tokens`, `vivo_hourly`, `vivo_gaps`, `vivo_cycles`, `vivo_download_final`,
`creations`, `trade_windows`, `price_windows` [solo `complete`/`note`],
`early_trades` [solo `ix_name` y fecha]). No abre `vivo_results`, `price_points`
ni `vivo_outcomes`, y no calcula T_entry2, D2, R1 ni R2. Es una lectura permitida
por la regla 3 del pre-registro de la fase 5 (conteos por estado y por hora).

Definiciones (fijadas antes de contar):
- Día y franja: UTC del blockTime de la creación; franjas 00–06, 06–12, 12–18, 18–24.
- Retraso de captura = `vivo_tokens.captured_at` − `block_time` (cuándo lo listó `collect`).
- Incompleto = ventana temprana o de precio no completa, o marcado en
  `vivo_download_final`. Un token sin fila de ventana/precio aún dentro de plazo
  (creado hace < 48 h) es "pendiente" y no cuenta como incompleto.
- Protocolo 1 = capturado antes de 2026-10-26 18:40:53 UTC; protocolo 2 = después.
- Un token por creator: el de menor slot dentro del bloque, sin mayhem.

Uso: python3 -B scripts/cobertura_adenda3.py [--vivo marxol_vivo.db] [--ahora UNIX]
"""
import argparse
import collections
import datetime as dt
import math
import os
import sqlite3
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from compras_ajenas_posthoc import share  # noqa: E402

PROTO2 = 1793040053  # 2026-10-26 18:40:53 UTC
A_FROM, A_TO, B_TO = 1792713600, 1793318400, 1794528000
SOL = {"11111111111111111111111111111111", "So11111111111111111111111111111111111111112"}  # SOL nativo / wSOL
DELAY_BINS = [("< 1 h", 0, 3600), ("1–6 h", 3600, 6 * 3600), ("6–24 h", 6 * 3600, 86400), ("> 24 h", 86400, 10**12)]


def day(ts):
    return dt.datetime.fromtimestamp(ts, dt.timezone.utc).strftime("%m-%d")


def q(xs, p):
    xs = sorted(xs)
    return xs[max(math.ceil(p * len(xs)) - 1, 0)] if xs else None


def h(x):
    return "—" if x is None else f"{x / 3600:.1f} h"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--vivo", default="marxol_vivo.db")
    ap.add_argument("--ahora", type=int, default=int(time.time()))
    a = ap.parse_args()
    db = sqlite3.connect(f"file:{a.vivo}?mode=ro", uri=True)
    now = a.ahora

    toks = db.execute("""
        SELECT v.mint, v.block_time, v.captured_at, v.status, c.creator, c.slot,
               COALESCE(c.is_mayhem_mode, 0), COALESCE(c.is_cashback, 0), COALESCE(c.is_holder_reward, 0),
               c.creator_fee_bps, c.quote_mint, tw.complete, pw.complete,
               (SELECT group_concat(f.state || ':' || f.purpose || ':' || f.reason, ' | ')
                  FROM vivo_download_final f WHERE f.mint = v.mint)
        FROM vivo_tokens v LEFT JOIN creations c ON c.mint = v.mint
        LEFT JOIN trade_windows tw ON tw.mint = v.mint LEFT JOIN price_windows pw ON pw.mint = v.mint
        WHERE v.block_time IS NOT NULL ORDER BY c.slot, v.mint""").fetchall()
    T = []
    for (mint, bt, cap, st, cre, slot, may, cash, hold, fee, quote, twc, pwc, fin) in toks:
        done = twc == 1 and pwc == 1
        pend = not done and not fin and bt > now - 48 * 3600
        T.append(dict(mint=mint, bt=bt, delay=cap - bt, st=st, cre=cre, slot=slot, may=may, cash=cash,
                      hold=hold, fee=fee, quote=quote, done=done, pend=pend, fin=fin or "",
                      inc=st == "creado" and not done and not pend, proto=2 if cap >= PROTO2 else 1))

    print("COBERTURA DE LA RECOLECCIÓN (adenda 3) — OPERACIÓN, SIN RETORNOS")
    print(f"marxol_vivo.db en solo lectura; corte {dt.datetime.fromtimestamp(now, dt.timezone.utc):%Y-%m-%d %H:%M} UTC."
          " No se lee vivo_results, price_points ni vivo_outcomes. IC 95 % de Wilson.\n")

    # 1. horas listadas y huecos por día ----------------------------------------
    hours = collections.defaultdict(set)
    for (hr, listed) in db.execute("SELECT hour, listed FROM vivo_hourly"):
        hours[day(hr * 3600)].add(hr % 24)
    gaps = db.execute("SELECT after_slot, before_slot, detected_at, reason FROM vivo_gaps").fetchall()
    print("1. HORAS UTC LISTADAS POR DÍA (vivo_hourly) Y HUECOS (vivo_gaps)")
    for d in sorted(hours):
        miss = sorted(set(range(24)) - hours[d])
        print(f"   {d}: {len(hours[d]):2}/24" + (f"; faltan {miss}" if miss else ""))
    print(f"   filas en vivo_gaps: {len(gaps)}" + "".join(f"\n     {g}" for g in gaps) + "\n")

    # 2. tokens por día, filtros y modos de creación -------------------------------
    print("2. TOKENS POR DÍA UTC DE CREACIÓN: estado, mayhem, un token por creator y campos del CreateEvent")
    print("   día   | muestreados | creado | mayhem | 1/creator sin mayhem | cashback | holder_reward | creator_fee_bps > 0 | quote ≠ SOL (de ellos …pump)")
    by_day = collections.defaultdict(list)
    for t in T:
        by_day[day(t["bt"])].append(t)
    for d in sorted(by_day):
        ts = by_day[d]
        cr = [t for t in ts if t["st"] == "creado"]
        seen, one = set(), 0
        for t in cr:
            if not t["may"] and t["cre"] not in seen:
                seen.add(t["cre"])
                one += 1
        print(f"   {d} | {len(ts):11} | {len(cr):6} | {sum(t['may'] for t in cr):6} | {one:20} | {sum(t['cash'] for t in cr):8} |"
              f" {sum(t['hold'] for t in cr):13} | {sum((t['fee'] or 0) > 0 for t in cr):19} | {sum((t['quote'] or '1' * 32) not in SOL for t in cr):11} ({sum((t['quote'] or '').endswith('pump') for t in cr)})")
    ix = collections.Counter()
    for d, name, n in db.execute("""SELECT strftime('%m-%d', timestamp, 'unixepoch'), COALESCE(ix_name, '(nulo)'), COUNT(*)
                                    FROM early_trades GROUP BY 1, 2"""):
        ix[(d, name)] = n
    names = sorted({k[1] for k in ix})
    print("   TradeEvent.ix_name por día (early_trades): " + "; ".join(
        f"{d}: " + ", ".join(f"{nm} {ix[(d, nm)]}" for nm in names if ix[(d, nm)]) for d in sorted({k[0] for k in ix})))
    print()

    # 3. bloque A ----------------------------------------------------------------
    def bloque(lo, hi):
        cr = [t for t in T if t["st"] == "creado" and lo <= t["bt"] < hi]
        seen, one = set(), []
        for t in cr:
            if not t["may"] and t["cre"] not in seen:
                seen.add(t["cre"])
                one.append(t)
        return cr, one
    cr, one = bloque(A_FROM, A_TO)
    dias = (min(now, A_TO) - A_FROM) / 86400
    print("3. BLOQUE A HASTA EL CORTE")
    print(f"   días transcurridos {dias:.2f}; creado {len(cr)}; un token por creator y sin mayhem {len(one)}"
          f" ({len(one) / dias:.0f}/día → {7 * len(one) / dias:.0f} esperados en 7 días; B: {14 * len(one) / dias:.0f} en 14)")
    print(f"   proporción que queda tras un token por creator y sin mayhem: {share(len(one), len(cr))}\n")

    # 4. retraso de captura e incompletos ------------------------------------------
    cr_all = [t for t in T if t["st"] == "creado" and t["bt"] >= A_FROM]
    print("4. RETRASO DE CAPTURA (captured_at − blockTime), tokens creado desde el inicio de A")
    for lab, sel in (("todos", cr_all), ("protocolo 1", [t for t in cr_all if t["proto"] == 1]),
                     ("protocolo 2", [t for t in cr_all if t["proto"] == 2])):
        ds = [t["delay"] for t in sel]
        print(f"   {lab:11} n {len(ds):4}: mediana {h(q(ds, .5))}, p75 {h(q(ds, .75))}, p90 {h(q(ds, .9))}, máx {h(max(ds) if ds else None)}")
    print("   incompletos por tramo de retraso (sin los pendientes):")
    for lab, lo, hi in DELAY_BINS:
        sel = [t for t in cr_all if lo <= t["delay"] < hi and not t["pend"]]
        print(f"     {lab:7} {share(sum(t['inc'] for t in sel), len(sel))}")
    print("   por día: mediana / p90 del retraso")
    for d in sorted({day(t["bt"]) for t in cr_all}):
        ds = [t["delay"] for t in cr_all if day(t["bt"]) == d]
        print(f"     {d}: n {len(ds):3}, {h(q(ds, .5))} / {h(q(ds, .9))}")
    print()

    # 5. incompletos por franja y motivo -------------------------------------------
    print("5. INCOMPLETOS POR DÍA Y FRANJA DE 6 h (creado; sin pendientes) Y MOTIVOS")
    for d in sorted({day(t["bt"]) for t in cr_all}):
        cells = []
        for f in range(4):
            sel = [t for t in cr_all if day(t["bt"]) == d and (t["bt"] // 3600 % 24) // 6 == f and not t["pend"]]
            cells.append(f"{6 * f:02}–{6 * f + 6:02} {sum(t['inc'] for t in sel)}/{len(sel)}")
        print(f"   {d}: " + " · ".join(cells))
    mot = collections.Counter()
    for t in cr_all:
        if t["inc"]:
            parts = t["fin"].split(" | ") if t["fin"] else ["sin marca final"]
            for part in parts:
                st, pur, why = (part.split(":", 2) + ["", ""])[:3]
                cat = "tope de 400 tx" if "más que el tope" in why else "max_pages" if "max_pages" in why else why
                mot[f"{st} · {pur} · {cat}"] += 1
    for k, n in mot.most_common():
        print(f"   {n:4}  {k}")
    print(f"   tokens incompletos: {sum(t['inc'] for t in cr_all)} (un token puede tener ventana y precio marcados)")
    print(f"   pendientes (dentro de plazo): {sum(t['pend'] for t in cr_all)}")
    print(f"   sin_create desde A: {sum(t['st'] == 'sin_create' for t in T if t['bt'] >= A_FROM)}")


if __name__ == "__main__":
    main()
