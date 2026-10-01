#!/usr/bin/env python3
"""Cribado POST HOC, SIN PRE-REGISTRO, SOLO DESCRIPTIVO (CLAUDE.md sección 8).
No cambia ninguna definición ni veredicto. Sin contrastes ni p-valores nuevos.
Solo lee marxol.db (conexión read-only, sin red, sin cuota).

Reproducir (desde la raíz del repo):
    cargo build --release && python3 -B scripts/cribado_posthoc.py > resultados/cribado_posthoc.txt

Definiciones (las de CLAUDE.md, secciones 8, 10 y 11):
- Tramos por slot de creación: piloto <= 451 340 397 < V1 <= 451 342 089;
  V2 = [451 810 013, 451 811 800]. Ráfaga 1 = piloto + V1 (2026-09-28),
  ráfaga 2 = V2 (2026-09-30).
- T_entry2 e indicadores: `marxol entry2 --check --json --tramo ...` (código
  congelado). H4' = dev_sold; H6' = fill_pct >= 13.65 (n/e si fill_pct es
  null; en los filtros un n/e cuenta como sin señal); H7' = pairs >= 1.
- V1 = los 251 tokens de V1 con T_entry2. V2 = los 197 de la población
  primaria (un token por creator: menor slot, empate por mint).
- Pump-y-caída (1 h, la de h1b): pico >= 2x el precio inicial y caída > 30 %
  del pico al cierre en [t0, t0 + 3600). Se calcula aquí para todas las
  creaciones (hace falta en la parte 2) y se comprueba contra `entry2 --json`.
- Supervivencia (H10): >= 1 trade a más de 10 min de T_entry2; n/e si la curva
  se completa en o antes de T_entry2 + 10 min.
- Retorno neto (a): (P_salida / P_entrada) (1 - f)^2 - 1, P_salida = último
  punto con timestamp <= salida en orden (timestamp, slot); n/e si la curva se
  completa en o antes de la salida o si la salida queda fuera del rango
  descargado. Se comprueba contra `entry2 --json` a +10/+30/+60 min.
- "Bueno" = retorno neto (a) > 0 a +30 min.
- Mayhem siempre aparte (su precio cambia fuera de los trades).
"""
import argparse, collections, json, sqlite3, subprocess

PILOT_LAST, V1_LAST, V2_FIRST, V2_LAST = 451_340_397, 451_342_089, 451_810_013, 451_811_800
HORIZONS = [(5, "+5 s"), (30, "+30 s"), (300, "+5 min"), (1800, "+30 min"), (3600, "+60 min")]
SIGS = ["H4'", "H6'", "H7'"]


def tramo(slot):
    if slot <= PILOT_LAST:
        return "piloto"
    if slot <= V1_LAST:
        return "V1"
    if V2_FIRST <= slot <= V2_LAST:
        return "V2"
    return "otro"


def med(v):
    v = sorted(v)
    n = len(v)
    return None if n == 0 else (v[n // 2] if n % 2 else (v[n // 2 - 1] + v[n // 2]) / 2)


def pc(k, n):
    return "n/a" if n == 0 else f"{100 * k / n:.0f} %"


def fr(x):
    return "n/a" if x is None else f"{100 * x:+.1f} %"


def entry2_check(marxol, db, t):
    out = subprocess.run([marxol, "--db", db, "entry2", "--check", "--json", "--tramo", t],
                         check=True, capture_output=True, text=True).stdout
    return {r["mint"]: r["entry2"] for r in json.loads(out)}


def load(marxol, dbp):
    db = sqlite3.connect(f"file:{dbp}?mode=ro", uri=True)
    q = lambda s, *a: db.execute(s, a).fetchall()
    toks = {}
    for m, c, u, s, t0, mh, sig in q("select mint, creator, user, slot, timestamp, coalesce(is_mayhem_mode,0), signature from creations"):
        toks[m] = dict(mint=m, creator=c, user=u, slot=s, t0=t0, mayhem=bool(mh), sig_tx=sig, tramo=tramo(s), e=None)
    comp = dict(q("select mint, timestamp from completions"))
    horizon = dict(q("select mint, max(horizon_secs) from price_windows where complete = 1 group by mint"))
    sharing_same_tx = {m for (m,) in q("select cc.mint from creator_changes cc join creations c on c.mint = cc.mint "
                                         "and c.signature = cc.signature where cc.kind = 'migrate_to_sharing'")}
    changes = collections.Counter(k for (k,) in q("select kind from creator_changes"))
    pts = collections.defaultdict(list)
    init = {}
    for m, kind, qr, tr, ts, sl in q("select mint, kind, quote_reserves, token_reserves, timestamp, slot from price_points "
                                     "order by mint, timestamp, slot"):
        if kind == "create":
            init[m] = qr / tr
        else:
            pts[m].append((ts, sl, qr / tr))
    for m, t in toks.items():
        t["completed_at"] = comp.get(m)
        t["horizon"] = horizon.get(m)
        t["sharing_same_tx"] = m in sharing_same_tx
        t["pts"] = pts[m]
        # Pump-y-caída (h1b::compute, ventana de 1 h).
        p0 = init.get(m)
        if p0 is None or t["horizon"] is None or t["horizon"] < 3600:
            t["pd"] = None
        else:
            w = [p for p in t["pts"] if t["t0"] <= p[0] < t["t0"] + 3600]
            peak = max([p0] + [p[2] for p in w])
            close = w[-1][2] if w else p0
            t["pd"] = peak / p0 >= 2.0 and 1.0 - close / peak > 0.30
    for tr_name, tr_arg in (("V1", "validacion"), ("V2", "validacion2")):
        for m, e in entry2_check(marxol, dbp, tr_arg).items():
            toks[m]["e"] = e
    for t in toks.values():
        e = t["e"]
        if e is None:
            continue
        fill = e["fill_pct"]
        t["sig"] = {"H4'": e["dev_sold"], "H6'": None if fill is None else fill >= 13.65, "H7'": e["pairs"] >= 1}
        k = sum(bool(t["sig"][s]) for s in SIGS)
        t["D"], t["D2"] = k >= 1, k >= 2
        et = e["entry_ts"]
        t["surv"] = None if t["completed_at"] is not None and t["completed_at"] <= et + 600 else any(p[0] > et + 600 for p in t["pts"])
        t["ret"] = {h: ret_a(t, h) for h in [h for h, _ in HORIZONS] + [600]}
    return toks, changes


def ret_a(t, h):
    e = t["e"]
    if e["fee_bps"] is None or t["horizon"] is None:
        return None
    x = e["entry_ts"] + h
    if t["t0"] + t["horizon"] - 1 < x or (t["completed_at"] is not None and t["completed_at"] <= x):
        return None
    before = [p for p in t["pts"] if p[0] <= x]
    if not before:
        return None
    f = e["fee_bps"] / 1e4
    return before[-1][2] / e["entry_price"] * (1 - f) ** 2 - 1


def controls(marxol, dbp, toks):
    out = subprocess.run([marxol, "--db", dbp, "entry2", "--json"], check=True, capture_output=True, text=True).stdout
    rows = json.loads(out)
    pd_bad = sum(toks[r["mint"]]["pd"] != r["pump_dump"] for r in rows)
    ret_bad = 0
    for r in rows:
        for j, h in enumerate((600, 1800, 3600)):
            a, b = toks[r["mint"]]["ret"][h], r["returns"][j]
            ret_bad += (a is None) != (b is None) or (a is not None and abs(a - b) > 1e-12)
    return len(rows), pd_bad, ret_bad


def group_line(label, g):
    pd = [t["pd"] for t in g]
    sv = [t["surv"] for t in g if t["surv"] is not None]
    rets = []
    for h, hl in HORIZONS:
        v = [t["ret"][h] for t in g if t["ret"][h] is not None]
        rets.append(f"{hl} {fr(med(v))} (n={len(v)})")
    good = [t for t in g if t["ret"][1800] is not None]
    ng = sum(t["ret"][1800] > 0 for t in good)
    print(f"    {label:12} n={len(g):3}  p&c {sum(pd)}/{len(g)} ({pc(sum(pd), len(g))})  "
          f"superv {sum(sv)}/{len(sv)} ({pc(sum(sv), len(sv))})  buenos {ng}/{len(good)}")
    print(f"    {'':12} retorno neto mediano: " + "  ".join(rets))


def filter_block(pop, key, title):
    disc = [t for t in pop if t[key]]
    keep = [t for t in pop if not t[key]]
    n = len(pop)
    print(f"  [{title}] descartados {len(disc)}/{n} ({pc(len(disc), n)})  pasan {len(keep)}/{n} ({pc(len(keep), n)})")
    group_line("descartados", disc)
    group_line("pasan", keep)
    malos = sum(t["pd"] for t in pop)
    md = sum(t["pd"] for t in disc)
    good = [t for t in pop if t["ret"][1800] is not None and t["ret"][1800] > 0]
    gd = sum(t[key] for t in good)
    print(f"    como detector de p&c: precisión {md}/{len(disc)} ({pc(md, len(disc))})  cobertura {md}/{malos} ({pc(md, malos)})")
    print(f"    buenos (ret +30 > 0): {len(good)} en total; descartados {gd}, pasan {len(good) - gd}"
          + ("  — n muy pequeño: sin potencia para decir nada sobre la pérdida de buenos" if len(good) < 20 else ""))


def dist(counter):
    c = collections.Counter(min(v, 4) for v in counter.values())
    return "  ".join(f"{k if k < 4 else '4+'}: {c.get(k, 0)}" for k in (1, 2, 3, 4)) + f"   (creators {len(counter)}, tokens {sum(counter.values())})"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--marxol", default="target/release/marxol")
    ap.add_argument("--db", default="marxol.db")
    args = ap.parse_args()
    toks, changes = load(args.marxol, args.db)
    n1, pd_bad, ret_bad = controls(args.marxol, args.db, toks)

    v1 = sorted([t for t in toks.values() if t["tramo"] == "V1" and t["e"] is not None], key=lambda t: (t["slot"], t["mint"]))
    v2_all = sorted([t for t in toks.values() if t["tramo"] == "V2" and t["e"] is not None], key=lambda t: (t["slot"], t["mint"]))
    first = {}
    for t in v2_all:
        first.setdefault(t["creator"], t)
    v2 = list(first.values())

    print("CRIBADO — POST HOC, SIN PRE-REGISTRO, SOLO DESCRIPTIVO (sin contrastes ni p-valores)")
    print(f"controles: V1 con T_entry2 = {len(v1)} (entry2 --json: {n1}); p&c distinto de entry2 --json: {pd_bad}; "
          f"retorno +10/+30/+60 distinto: {ret_bad}; V2 primaria = {len(v2)} (V2 con T_entry2: {len(v2_all)})")
    for name, pop in (("V1", v1), ("V2 primaria", v2)):
        print(f"  {name}: p&c {sum(t['pd'] for t in pop)}/{len(pop)}; H6' n/e {sum(t['sig']['H6\'']is None for t in pop)}; "
              f"sin mayhem {sum(not t['mayhem'] for t in pop)}, mayhem {sum(t['mayhem'] for t in pop)}")

    print("\n" + "=" * 100)
    print("PARTE 1. FILTRO DE DESCARTE CON LAS SEÑALES CONFIRMADAS")
    print("D = H4' o H6' o H7'.  D2 = al menos 2 de las 3.  Las señales se eligieron mirando V1:")
    print("la cifra honesta es la de V2. Los retornos de los tokens mayhem no son fiables (precio fuera de los trades).")
    for name, pop in (("V2 primaria (cifra honesta)", v2), ("V1 (251; señales elegidas aquí, optimista)", v1)):
        for mh in (False, True):
            sub = [t for t in pop if t["mayhem"] == mh]
            print(f"\n## {name} — {'MAYHEM (aparte)' if mh else 'sin mayhem'}: n = {len(sub)}")
            if not sub:
                continue
            for key, title in (("D", "D: H4' o H6' o H7'"), ("D2", "D2: >= 2 de 3")):
                filter_block(sub, key, title)

    print("\n" + "=" * 100)
    print("PARTE 2. REPETICIÓN DE CREADORES (creator del CreateEvent, no el payer)")
    print(f"creaciones en marxol.db: {len(toks)} (piloto {sum(t['tramo'] == 'piloto' for t in toks.values())}, "
          f"V1 {sum(t['tramo'] == 'V1' for t in toks.values())}, V2 {sum(t['tramo'] == 'V2' for t in toks.values())})")
    print(f"creator != payer: {sum(t['creator'] != t['user'] for t in toks.values())}")
    print(f"eventos de cambio de creator: {dict(changes)} (SetCreatorEvent: {changes.get('set_creator', 0)})")
    for r in ("ráfaga 1", "ráfaga 2"):
        sel = [t for t in toks.values() if (t["tramo"] in ("piloto", "V1")) == (r == "ráfaga 1")]
        print(f"  {r}: creaciones con migración a sharing_config en la misma tx: {sum(t['sharing_same_tx'] for t in sel)} de {len(sel)}")
    print("  (el análisis usa el creator de la creación; si la tx ya migra a sharing_config, el creator del")
    print("   CreateEvent sigue siendo la wallet original, así que no afecta a la identidad usada aquí)")

    burst = lambda t: 1 if t["tramo"] in ("piloto", "V1") else 2
    by_b = {b: collections.Counter(t["creator"] for t in toks.values() if burst(t) == b) for b in (1, 2)}
    print("\n2.1 Tokens por creator (distribución 1 / 2 / 3 / 4+)")
    print(f"  ráfaga 1 (piloto + V1): {dist(by_b[1])}")
    print(f"  ráfaga 2 (V2):          {dist(by_b[2])}")
    both = set(by_b[1]) & set(by_b[2])
    print(f"  creators en las dos ráfagas: {len(both)}")
    if both:
        print(f"    tokens totales de esos creators: {dist({c: by_b[1][c] + by_b[2][c] for c in both})}")
        for c in sorted(both, key=lambda c: -(by_b[1][c] + by_b[2][c])):
            ts = [t for t in toks.values() if t["creator"] == c]
            print(f"    {c}: ráfaga 1 {by_b[1][c]}, ráfaga 2 {by_b[2][c]}, mayhem {sum(t['mayhem'] for t in ts)}/{len(ts)}, "
                  f"p&c ráfaga 1 {sum(t['pd'] for t in ts if burst(t) == 1)}, ráfaga 2 {sum(t['pd'] for t in ts if burst(t) == 2)}")

    v1c = collections.defaultdict(list)
    v2c = collections.defaultdict(list)
    for t in toks.values():
        if t["tramo"] == "V1":
            v1c[t["creator"]].append(t)
        elif t["tramo"] == "V2":
            v2c[t["creator"]].append(t)
    print("\n2.2 Creators con tokens en V1 y en V2: ¿el p&c de V1 predice el de V2?")
    print("  Creator 'malo' en un tramo = al menos un token con p&c. Todas las creaciones del tramo (no solo las de T_entry2).")
    for mh, lab in ((False, "solo tokens sin mayhem"), (True, "solo tokens mayhem (aparte)")):
        tab = collections.Counter()
        for c in set(v1c) & set(v2c):
            a = [t["pd"] for t in v1c[c] if t["mayhem"] == mh]
            b = [t["pd"] for t in v2c[c] if t["mayhem"] == mh]
            if a and b:
                tab[(any(a), any(b))] += 1
        n = sum(tab.values())
        print(f"  [{lab}] creators con tokens en ambos tramos: {n}")
        print(f"                     V2 p&c sí   V2 p&c no")
        print(f"    V1 p&c sí        {tab[(True, True)]:9}   {tab[(True, False)]:9}")
        print(f"    V1 p&c no        {tab[(False, True)]:9}   {tab[(False, False)]:9}")

    # 2.3: V1 (dos días antes) -> V2. El p&c de un token se conoce en t0 + 3600.
    def decision_ts(t):
        return t["e"]["entry_ts"] if t["e"] is not None else t["t0"]

    def marked_v1(t):
        return any(u["pd"] and u["t0"] + 3600 <= decision_ts(t) for u in v1c.get(t["creator"], []))

    def marked_v2_strict(t):
        return any(u["pd"] and u["slot"] < t["slot"] and u["t0"] + 3600 <= decision_ts(t) for u in v2c[t["creator"]])

    def marked_v2_leak(t):
        return any(u["pd"] and u["slot"] < t["slot"] for u in v2c[t["creator"]])

    def mark_block(pop, fn, label):
        for mh in (False, True):
            sub = [t for t in pop if t["mayhem"] == mh]
            if not sub:
                continue
            m = [t for t in sub if fn(t)]
            r = [t for t in sub if not fn(t)]
            def s(g):
                sv = [t["surv"] for t in g if t.get("surv") is not None]
                return f"n={len(g):3}  p&c {sum(t['pd'] for t in g)}/{len(g)} ({pc(sum(t['pd'] for t in g), len(g))})  superv {sum(sv)}/{len(sv)} ({pc(sum(sv), len(sv))})"
            print(f"  [{label} — {'MAYHEM' if mh else 'sin mayhem'}] marcados: {s(m)}")
            print(f"  {'':{len(label) + 17}} resto:     {s(r)}")

    v2_creations = [t for t in toks.values() if t["tramo"] == "V2"]
    print("\n2.3 Cribado retroactivo V1 -> V2 (creador marcado = tenía en V1 un token con p&c, ya conocido al decidir)")
    print(f"  sobre las 400 creaciones de V2: marcadas {sum(marked_v1(t) for t in v2_creations)} "
          f"(sin mayhem {sum(marked_v1(t) for t in v2_creations if not t['mayhem'])}, mayhem {sum(marked_v1(t) for t in v2_creations if t['mayhem'])})")
    mark_block(v2, marked_v1, "V2 primaria")
    mark_block(v2_all, marked_v1, "V2 con T_entry2 (283)")

    print("\n2.4 Dentro de V2, orden por slot")
    print("  ESTRICTO: el p&c de un token anterior solo se conoce en su t0 + 1 h; la ráfaga dura 8 min.")
    print(f"  marcados (estricto) entre las 400 creaciones: {sum(marked_v2_strict(t) for t in v2_creations)}")
    print("  CON FUGA (usa el p&c de tokens anteriores antes de que se conozca; NO es un cribado realizable, solo referencia):")
    print(f"  marcados (con fuga) entre las 400 creaciones: {sum(marked_v2_leak(t) for t in v2_creations)}")
    mark_block(v2, marked_v2_leak, "V2 primaria, con fuga")
    mark_block(v2_all, marked_v2_leak, "V2 con T_entry2, con fuga")

    print("\n2.5 V2 primaria: D, creador marcado (V1) y su unión. Cobertura de p&c y buenos descartados")
    for mh in (False, True):
        sub = [t for t in v2 if t["mayhem"] == mh]
        malos = sum(t["pd"] for t in sub)
        good = [t for t in sub if t["ret"][1800] is not None and t["ret"][1800] > 0]
        print(f"  [{'MAYHEM (aparte)' if mh else 'sin mayhem'}] n = {len(sub)}, p&c = {malos}, buenos (ret +30 > 0) = {len(good)}")
        for lab, fn in (("D", lambda t: t["D"]), ("D2", lambda t: t["D2"]), ("creador marcado", marked_v1),
                        ("D o creador marcado", lambda t: t["D"] or marked_v1(t)),
                        ("D2 o creador marcado", lambda t: t["D2"] or marked_v1(t))):
            d = [t for t in sub if fn(t)]
            md = sum(t["pd"] for t in d)
            print(f"    {lab:22} descarta {len(d):3}/{len(sub)} ({pc(len(d), len(sub))})  p&c capturado {md}/{malos} ({pc(md, malos)})  "
                  f"precisión {pc(md, len(d))}  buenos descartados {sum(fn(t) for t in good)}/{len(good)}")

    print("\n" + "=" * 100)
    print("LÍMITES")
    print("- Todo es post hoc y sin pre-registro: no confirma ni descarta nada y no cambia ningún veredicto.")
    print("- Las señales D se eligieron mirando V1; las cifras de V1 son optimistas. La cifra honesta es V2.")
    print("- n pequeño: los tokens 'buenos' (ret +30 > 0) son unos pocos; no hay potencia para medir cuántos buenos")
    print("  pierde un filtro. Las tablas de creators repetidos en ambos tramos tienen muy pocos creators.")
    print("- Dos ráfagas cortas (8 min cada una, 2026-09-28 y 2026-09-30): nada de esto generaliza a otras horas o días.")
    print("- Dentro de una ráfaga de 8 min, el p&c de un token anterior no se conoce a tiempo (hace falta 1 h): el")
    print("  cribado por creator dentro de la ráfaga solo existe con fuga de información.")
    print("- Identidad = creator del CreateEvent. No hay SetCreatorEvent en la base, pero un creator puede cambiar después")
    print("  (SetCreatorEvent, migración a sharing_config) o usar wallets distintas: la repetición está infraestimada.")
    print("- Los retornos de los tokens mayhem no son fiables (sus reservas virtuales cambian fuera de los trades).")


if __name__ == "__main__":
    main()
