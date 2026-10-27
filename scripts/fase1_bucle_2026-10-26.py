#!/usr/bin/env python3
"""Fase 1 del informe 2026-10-26: medición del bucle de reintentos de collect.

Solo lectura: abre marxol_vivo.db con mode=ro y lee run/collect.log. No lee
retornos (ni vivo_results.result_json ni price_points ni vivo_outcomes).

a) Cobertura de ciclos por hora UTC y hora a la que se agotó la bolsa collect.
b) Tokens del bloque A por hora UTC de creación y % con ventana incompleta,
   por motivo (tope de 400 tx, max_pages, otros).
c) CU gastados en reintentos de ventanas sin progreso, total y por día.

Estimación de (c): CU de las etapas de ventana y precio de cada ciclo menos
el coste de las descargas que se completaron en ese ciclo (40 CU por página
de firmas y por tx). De lo que queda, se reparte entre primeros intentos y
reintentos según las líneas de "incompleta" del log de ese ciclo.
"""
import re
import sqlite3
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone

DB = "file:marxol_vivo.db?mode=ro"
LOG = "run/collect.log"
A_FROM, A_UNTIL = 1792713600, 1793318400
DAY_FROM = "2026-10-23"
CU = 40

db = sqlite3.connect(DB, uri=True)


def day(ts):
    return datetime.fromtimestamp(ts, timezone.utc).strftime("%Y-%m-%d")


def hm(ts):
    return datetime.fromtimestamp(ts, timezone.utc).strftime("%H:%M")


cycles = db.execute(
    "SELECT cycle_id, started_at, ended_at, units_poll + units_outcomes + units_create + units_windows + units_prices,"
    " units_windows, units_prices FROM vivo_cycles WHERE ended_at IS NOT NULL ORDER BY cycle_id"
).fetchall()
pool = dict(db.execute("SELECT day, units FROM cu_ledger_day WHERE pool = 'collect'").fetchall())

# ---------------------------------------------------------------- a)
print("a) Bolsa collect y horas con ciclos de listado (desde 2026-10-23)\n")
by_day = defaultdict(list)
for c in cycles:
    by_day[day(c[1])].append(c)
for d in sorted(by_day):
    if d < DAY_FROM:
        continue
    cs = by_day[d]
    hours = sorted({int(hm(c[1])[:2]) for c in cs})
    missing = [h for h in range(24) if h not in hours]
    # Agotada si el libro del día llegó a menos de MIN_CYCLE_UNITS (2 000) del tope:
    # a partir del último ciclo del día ya no se lista.
    exhausted = hm(cs[-1][2]) if pool.get(d, 0) >= 700_000 - 2_000 else None
    print(f"  {d}: {len(cs)} ciclos, bolsa {pool.get(d, 0)} CU, agotada a las "
          f"{exhausted or '— (no se agotó)'} UTC")
    print(f"    horas UTC sin ningún ciclo ({len(missing)}): ", end="")
    # compactar en rangos
    rng, start, prev = [], None, None
    for h in missing:
        if start is None:
            start = prev = h
        elif h == prev + 1:
            prev = h
        else:
            rng.append((start, prev))
            start = prev = h
    if start is not None:
        rng.append((start, prev))
    print(", ".join(f"{a:02d}–{b + 1:02d}" for a, b in rng) or "ninguna")

# ---------------------------------------------------------------- b)
print("\nb) Bloque A (creados hasta ahora): tokens por hora UTC de creación y ventana incompleta\n")
rows = db.execute(
    """SELECT t.hour_utc, t.mint, w.complete, w.note, p.complete, p.note,
              (SELECT attempts FROM download_attempts a WHERE a.purpose = 'window' AND a.mint = t.mint)
       FROM vivo_tokens t
       LEFT JOIN trade_windows w ON w.mint = t.mint
       LEFT JOIN price_windows p ON p.mint = t.mint
       WHERE t.block_time >= ? AND t.block_time < ? AND t.status = 'creado'""",
    (A_FROM, A_UNTIL),
).fetchall()


def motivo(complete, note):
    if complete == 1:
        return None
    if complete is None:
        return "pendiente"
    if note and "tope por token" in note:
        return "tope400"
    if note and note.startswith("max_pages"):
        return "max_pages"
    return "otros"


per_hour = defaultdict(Counter)
tot = Counter()
for h, mint, wc, wn, pc, pn, att in rows:
    mw = motivo(wc, wn)
    mp = motivo(pc, pn) if wc == 1 else None
    k = per_hour[h]
    k["n"] += 1
    tot["n"] += 1
    m = mw or (f"precio_{mp}" if mp else None)
    if m:
        k[m] += 1
        tot[m] += 1
cols = ["tope400", "max_pages", "otros", "pendiente", "precio_tope400", "precio_max_pages", "precio_otros", "precio_pendiente"]
cols = [c for c in cols if tot[c]]
print("  hora   n   " + "  ".join(f"{c:>15}" for c in cols) + "   % incompleta")
for h in range(24):
    k = per_hour.get(h, Counter())
    inc = sum(k[c] for c in cols)
    pct = f"{100 * inc / k['n']:.1f}" if k["n"] else "—"
    print(f"  {h:02d}  {k['n']:4d}   " + "  ".join(f"{k[c]:15d}" for c in cols) + f"   {pct:>6}")
inc = sum(tot[c] for c in cols)
print(f"  tot {tot['n']:4d}   " + "  ".join(f"{tot[c]:15d}" for c in cols) + f"   {100 * inc / tot['n']:.1f}")
nsc = db.execute(
    "SELECT status, COUNT(*) FROM vivo_tokens WHERE block_time >= ? AND block_time < ? AND status != 'creado' GROUP BY 1",
    (A_FROM, A_UNTIL),
).fetchall()
print(f"  además, no 'creado': {nsc}")
print("  (precio_*: ventana temprana completa y ventana de precio incompleta)")

# ---------------------------------------------------------------- c)
print("\nc) CU en reintentos de ventanas sin progreso\n")
# Coste de las descargas completadas en cada ciclo.
done_w = db.execute("SELECT fetched_at, sigs_in_window FROM trade_windows WHERE complete = 1").fetchall()
done_p = db.execute("SELECT fetched_at, sigs_in_window FROM price_windows WHERE complete = 1").fetchall()

# Líneas de "incompleta" del log por ciclo: (mint, tipo); primer intento = primera vez que aparece.
inc_re = re.compile(r"^(\w{32,44}): (ventana incompleta|ventana de precio incompleta|error listando firmas)")
cyc_re = re.compile(r"^ciclo (\d+):")
r429 = re.compile(r"^reintento por 429")
per_cycle_lines = defaultdict(lambda: {"first": 0, "retry": 0, "429": 0})
seen = set()
pending = {"first": 0, "retry": 0, "429": 0}
with open(LOG, encoding="utf-8", errors="replace") as f:
    for line in f:
        m = inc_re.match(line)
        if m:
            key = (m.group(1), "precio" in m.group(2))
            pending["retry" if key in seen else "first"] += 1
            seen.add(key)
            continue
        if r429.match(line):
            pending["429"] += 1
            continue
        m = cyc_re.match(line)
        if m:
            per_cycle_lines[int(m.group(1))] = pending
            pending = {"first": 0, "retry": 0, "429": 0}

waste_day = Counter()
units_day = Counter()
retry_lines_day = Counter()
for cid, s, e, u, uw, up in cycles:
    succ = sum(CU * n for t, n in done_w if s <= t <= e) + sum(CU * (1 + n) for t, n in done_p if s <= t <= e)
    rest = max(0, uw + up - succ)
    L = per_cycle_lines.get(cid, {"first": 0, "retry": 0})
    att = L["first"] + L["retry"]
    waste = rest * L["retry"] / att if att else 0
    d = day(s)
    waste_day[d] += waste
    units_day[d] += u
    retry_lines_day[d] += L["retry"]
print("  día          reintentos  CU reintentos  CU del día  % del día  % de la bolsa (700 000)")
tw = tu = 0
for d in sorted(units_day):
    w, u = waste_day[d], units_day[d]
    tw += w
    tu += u
    print(f"  {d}  {retry_lines_day[d]:10d}  {w:13.0f}  {u:10d}  {100 * w / u:8.1f}  {100 * w / 700_000:8.1f}")
print(f"  total       {sum(retry_lines_day.values()):10d}  {tw:13.0f}  {tu:10d}  {100 * tw / tu:8.1f}")
print(f"\n  reintentos 429 en el log: {sum(v['429'] for v in per_cycle_lines.values())} (cada uno cuenta 40 CU)")
print("  estimación: CU de ventana+precio del ciclo − coste de lo completado en él (40 CU por página y por tx),")
print("  repartido entre primeros intentos y reintentos según las líneas del log.")
