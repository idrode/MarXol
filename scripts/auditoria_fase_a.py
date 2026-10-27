#!/usr/bin/env python3
"""Auditoría (fase A, 2026-10-22): qué se puede medir de bundles y concentración
en T_entry2 con lo que ya guarda marxol.db. POST HOC, SIN VEREDICTO, sin red.

Solo cuenta tokens por condición: no cruza nada con retornos ni con pump-y-caída.
marxol_vivo.db no se abre (docs/preregistro_fase5_2026-10-22.md).

Reproducir (desde la raíz del repo; `entry2` abre la base con Store::open, así
que se le pasa una copia):
    cargo build --release
    cp marxol.db /ruta/copia.db
    python3 -B scripts/auditoria_fase_a.py --copia /ruta/copia.db > resultados/auditoria_fase_a.txt

Definiciones (fijadas antes de contar):
- Población: tokens de piloto, V1, V2 y V3 con T_entry2 (`entry2 --check --json`,
  código congelado), sin mayhem. Mayhem aparte.
- Antes de T_entry2: los `trades_before` primeros trades de la ventana en orden
  de ejecución (`entry::execution_order`, reimplementado aquí y comprobado
  contra entry2: slot y timestamp del trade T_entry2, H4', H7', H5 y H8).
- Identidades de creador en T_entry2: creator y user del CreateEvent, cambios
  de creator con slot < slot de T_entry2 y creator embebido en los trades
  anteriores a T_entry2. No usa nada posterior (la unión congelada de H4'–H8 sí
  usa el creator embebido de toda la ventana; aquí solo se usa como control).
- Tx con varias cuentas: firma con compras (TradeEvent is_buy) de ≥ 2 `user`
  distintos del mismo mint. En pump.fun el `user` de un buy firma la tx, así que
  son varias firmas en una misma tx.
- Tenencia en T_entry2: compras − ventas de token por `user` antes de T_entry2,
  en % del supply (10^15). No ve transferencias de token entre wallets.
"""
import argparse, collections, json, math, sqlite3, subprocess

SUPPLY = 1e15
TRAMOS = ["piloto", "validacion", "validacion2", "validacion3"]


def wilson(k, n, z=1.959964):
    if n == 0:
        return float("nan"), float("nan")
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return c - h, c + h


def share(k, n):
    if n == 0:
        return "0/0"
    lo, hi = wilson(k, n)
    return f"{k}/{n} = {100 * k / n:.1f} % [IC 95 % {100 * max(lo, 0):.1f}–{100 * min(hi, 1):.1f}]"


def pct(xs, p):
    xs = sorted(xs)
    k = (len(xs) - 1) * p
    f = int(k)
    c = min(f + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def desc(xs, unit=""):
    if not xs:
        return "n = 0"
    return (f"n = {len(xs)}: p10 {pct(xs, .1):.3g}{unit}, p25 {pct(xs, .25):.3g}{unit}, mediana {pct(xs, .5):.3g}{unit}, "
            f"p75 {pct(xs, .75):.3g}{unit}, p90 {pct(xs, .9):.3g}{unit}, máx {max(xs):.3g}{unit}")


def execution_order(v0, trades):
    """entry::execution_order."""
    def before(t):
        return t["v"] + t["tok"] if t["buy"] else t["v"] - t["tok"]
    used = [False] * len(trades)
    out, cur = [], v0
    while True:
        i = next((i for i in range(len(trades)) if not used[i] and before(trades[i]) == cur), None)
        if i is None:
            break
        used[i] = True
        cur = trades[i]["v"]
        out.append(trades[i])
    complete = len(out) == len(trades)
    out += [trades[i] for i in range(len(trades)) if not used[i]]
    return out, complete


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default="marxol.db")
    ap.add_argument("--copia", required=True, help="copia de marxol.db para `entry2`")
    ap.add_argument("--marxol", default="target/release/marxol")
    args = ap.parse_args()

    e2 = {}
    for t in TRAMOS:
        out = subprocess.run([args.marxol, "--db", args.copia, "--json", "entry2", "--check", "--tramo", t],
                             check=True, capture_output=True, text=True).stdout
        for r in json.loads(out):
            e2[r["mint"]] = r["entry2"]

    db = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    q = lambda s, *a: db.execute(s, a).fetchall()
    toks = {m: dict(creator=c, user=u, slot=s, t0=t0, mayhem=bool(mh), sig=sig)
            for m, c, u, s, t0, mh, sig in q("SELECT mint, creator, user, slot, timestamp, COALESCE(is_mayhem_mode, 0), signature FROM creations")}
    v0 = dict(q("SELECT mint, token_reserves FROM price_points WHERE kind = 'create'"))
    changes = collections.defaultdict(list)
    for m, new, old, sl in q("SELECT mint, new_creator, old_creator, slot FROM creator_changes"):
        changes[m].append((sl, new, old))
    trades = collections.defaultdict(list)
    for m, u, b, tok, sl, ts, v, cr, sig, ix in q(
            "SELECT e.mint, e.user, is_buy, COALESCE(token_amount, 0), e.slot, e.timestamp, COALESCE(virtual_token_reserves, 0), "
            "e.creator, e.signature, ev_index FROM early_trades e JOIN creations c ON c.mint = e.mint "
            "WHERE e.timestamp >= c.timestamp AND e.timestamp < c.timestamp + 300 ORDER BY e.mint, e.slot, e.timestamp, e.signature, ev_index"):
        trades[m].append(dict(user=u, buy=bool(b), tok=tok, slot=sl, ts=ts, v=v, creator=cr, sig=sig))
    all_creator = collections.defaultdict(set)
    for m, cr in q("SELECT DISTINCT mint, creator FROM early_trades WHERE creator IS NOT NULL"):
        all_creator[m].add(cr)

    print("Auditoría fase A — POST HOC, SIN VEREDICTO, sin red. Solo recuentos; ningún cruce con retornos.\n")
    print(f"tokens en entry2 (4 tramos): {len(e2)}; con T_entry2: {sum(e is not None for e in e2.values())}")

    rows, bad = [], collections.Counter()
    for m, e in e2.items():
        if e is None:
            continue
        t = toks[m]
        ordered, _ = execution_order(v0[m], trades[m])
        k = e["trades_before"]
        before, entry = ordered[:k], ordered[k]
        # Controles contra el código congelado (unión completa de identidades).
        frozen_ids = {t["creator"], t["user"]} | {x for c in changes[m] for x in c[1:] if x} | all_creator[m]
        sides = collections.defaultdict(lambda: [0, 0])
        for x in before:
            if x["user"] not in frozen_ids:
                sides[x["user"]][0 if x["buy"] else 1] += 1
        slot_buys = [x for x in before if x["buy"] and x["slot"] == t["slot"]]
        bad["entry"] += (entry["slot"], entry["ts"]) != (e["entry_slot"], e["entry_ts"])
        bad["H4'"] += any(not x["buy"] and x["user"] in frozen_ids for x in before) != e["dev_sold"]
        bad["H7'"] += sum(s == [1, 1] for s in sides.values()) != e["pairs"]
        bad["H5"] += len({x["user"] for x in slot_buys if x["user"] not in frozen_ids}) != e["same_slot_wallets"]
        bad["H8"] += abs(100 * sum(x["tok"] for x in slot_buys if x["user"] in frozen_ids) / SUPPLY - e["creator_slot_buy_pct"]) > 1e-9

        # Variables nuevas, solo con lo disponible en T_entry2.
        ids = {t["creator"], t["user"]} | {x for c in changes[m] if c[0] < e["entry_slot"] for x in c[1:] if x}
        ids |= {x["creator"] for x in before if x["creator"]}
        buyers_by_sig = collections.defaultdict(set)
        for x in before:
            if x["buy"]:
                buyers_by_sig[x["sig"]].add(x["user"])
        multi = {s: us for s, us in buyers_by_sig.items() if len(us) >= 2}
        hold = collections.Counter()
        for x in before:
            hold[x["user"]] += x["tok"] if x["buy"] else -x["tok"]
        others = sorted((h for u, h in hold.items() if u not in ids and h > 0), reverse=True)
        multi_users = set().union(*multi.values()) if multi else set()
        rows.append(dict(
            mint=m, mayhem=t["mayhem"], n_before=k,
            creator_slot_buy=any(x["buy"] and x["slot"] == t["slot"] and x["user"] in ids for x in before),
            creator_create_tx_buy=any(x["buy"] and x["sig"] == t["sig"] and x["user"] in ids for x in before),
            create_tx_other_buyer=any(x["buy"] and x["sig"] == t["sig"] and x["user"] not in ids for x in before),
            n_multi=len(multi),
            n_multi_wallets=len(multi_users),
            multi_with_other=any(us - ids for us in multi.values()),
            multi_max=max((len(us) for us in multi.values()), default=0),
            slot_other_buyers=len({x["user"] for x in before if x["buy"] and x["slot"] == t["slot"] and x["user"] not in ids}),
            creator_hold=100 * max(0, sum(h for u, h in hold.items() if u in ids)) / SUPPLY,
            top={n: 100 * sum(others[:n]) / SUPPLY for n in (1, 3, 5, 10)},
            multi_hold=100 * sum(max(0, hold[u]) for u in multi_users - ids) / SUPPLY,
            buyers_before=len({x["user"] for x in before if x["buy"]}),
            sigs_before=len({x["sig"] for x in before}),
            sigs_slot=len({x["sig"] for x in before if x["slot"] == t["slot"]}),
            sigs_window=len({x["sig"] for x in trades[m]}),
            h4=e["dev_sold"], h7=e["pairs"] >= 1,
            h6=None if e["fill_pct"] is None else e["fill_pct"] >= 13.65, fill=e["fill_pct"],
        ))
    print("controles contra entry2 --json (deben ser 0): " + ", ".join(f"{k} distinto {v}" for k, v in sorted(bad.items())))

    for label, pop in (("SIN MAYHEM", [r for r in rows if not r["mayhem"]]), ("MAYHEM (aparte)", [r for r in rows if r["mayhem"]])):
        n = len(pop)
        c = lambda f: share(sum(1 for r in pop if f(r)), n)
        print(f"\n=== {label}: {n} tokens con T_entry2 ===")
        print(f"trades antes de T_entry2: {desc([r['n_before'] for r in pop])}")
        print(f"  con 0 trades antes: {c(lambda r: r['n_before'] == 0)}; con ≤ 2: {c(lambda r: r['n_before'] <= 2)}")
        print(f"compradores distintos antes de T_entry2: {desc([r['buyers_before'] for r in pop])}")
        print("\n-- Creador en el slot de creación --")
        print(f"identidad de creador compra en el slot de creación: {c(lambda r: r['creator_slot_buy'])}")
        print(f"  ... en la propia tx de creación: {c(lambda r: r['creator_create_tx_buy'])}")
        print(f"otra cuenta compra en la propia tx de creación: {c(lambda r: r['create_tx_other_buyer'])}")
        print(f"cuentas ajenas que compran en el slot de creación (H5 con identidades en T_entry2): {desc([r['slot_other_buyers'] for r in pop])}")
        for k in (1, 2, 3, 5):
            print(f"  ≥ {k}: {c(lambda r, k=k: r['slot_other_buyers'] >= k)}")
        print("\n-- (a) Tx con compras de ≥ 2 cuentas, antes de T_entry2 --")
        print(f"≥ 1 tx así: {c(lambda r: r['n_multi'] >= 1)}")
        print(f"≥ 1 tx así con alguna cuenta ajena al creador: {c(lambda r: r['multi_with_other'])}")
        print(f"tx así por token: {desc([r['n_multi'] for r in pop])}")
        print(f"  entre los que tienen ≥ 1: {desc([r['n_multi'] for r in pop if r['n_multi']])}")
        print(f"cuentas por tx (máx. por token, entre los que tienen ≥ 1): {desc([r['multi_max'] for r in pop if r['n_multi']])}")
        print(f"cuentas distintas en esas tx (entre los que tienen ≥ 1): {desc([r['n_multi_wallets'] for r in pop if r['n_multi']])}")
        print("\n-- (b) Tenencia en T_entry2 (% del supply, compras − ventas) --")
        print(f"identidades de creador: {desc([r['creator_hold'] for r in pop], ' %')}")
        for th in (0.0, 1, 5, 10, 20):
            print(f"  > {th} %: {c(lambda r, th=th: r['creator_hold'] > th)}")
        for k in (1, 3, 5, 10):
            print(f"top {k} cuentas ajenas: {desc([r['top'][k] for r in pop], ' %')}")
            print("  " + "; ".join(f"> {th} %: {c(lambda r, th=th, k=k: r['top'][k] > th)}" for th in (5, 10, 20, 30)))
        print(f"cuentas ajenas de tx con ≥ 2 cuentas: {desc([r['multi_hold'] for r in pop if r['n_multi']], ' %')} (solo tokens con ≥ 1 tx así)")
        if not pop:
            continue
        print("\n-- Solapamiento con los indicadores de D2 (sin retornos) --")
        def d2(r):
            k = r["h4"] + r["h7"]
            return k + r["h6"] >= 2 if r["h6"] is not None else {2: True, 0: False}.get(k)
        for name, f in (("tx ≥ 2 cuentas", lambda r: r["n_multi"] >= 1), ("top 10 ajenas > 10 %", lambda r: r["top"][10] > 10),
                        ("creador > 5 %", lambda r: r["creator_hold"] > 5)):
            sub = [r for r in pop if f(r)]
            rest = [r for r in pop if not f(r)]
            for lab, g in (("con", sub), ("sin", rest)):
                m = len(g)
                cc = lambda h: share(sum(1 for r in g if h(r)), m)
                print(f"{name}, {lab}: H4' {cc(lambda r: r['h4'])}; H6' {cc(lambda r: r['h6'])}; H7' {cc(lambda r: r['h7'])}; D2 descarta {cc(lambda r: d2(r) is True)}")
        xs = [(r["fill"], r["top"][10]) for r in pop if r["fill"] is not None]
        print(f"Spearman(llenado, top 10 ajenas) = {spearman(*zip(*xs)):.3f} (n = {len(xs)})")
        xs = [(r["fill"], r["creator_hold"]) for r in pop if r["fill"] is not None]
        print(f"Spearman(llenado, tenencia del creador) = {spearman(*zip(*xs)):.3f} (n = {len(xs)})")
        print("\n-- Firmas (para estimar coste de lo no guardado) --")
        print(f"firmas con trades antes de T_entry2: {desc([r['sigs_before'] for r in pop])}; total {sum(r['sigs_before'] for r in pop)}")
        print(f"firmas con trades en el slot de creación: {desc([r['sigs_slot'] for r in pop])}; total {sum(r['sigs_slot'] for r in pop)}")
        print(f"firmas con trades en la ventana de 300 s: {desc([r['sigs_window'] for r in pop])}; total {sum(r['sigs_window'] for r in pop)}")


def ranks(v):
    o = sorted(range(len(v)), key=lambda i: v[i])
    r = [0.0] * len(v)
    i = 0
    while i < len(o):
        j = i
        while j + 1 < len(o) and v[o[j + 1]] == v[o[i]]:
            j += 1
        for k in range(i, j + 1):
            r[o[k]] = (i + j) / 2
        i = j + 1
    return r


def spearman(x, y):
    a, b = ranks(x), ranks(y)
    ma, mb = sum(a) / len(a), sum(b) / len(b)
    num = sum((p - ma) * (q - mb) for p, q in zip(a, b))
    return num / math.sqrt(sum((p - ma) ** 2 for p in a) * sum((q - mb) ** 2 for q in b))


if __name__ == "__main__":
    main()
