#!/usr/bin/env python3
"""Compras ajenas tempranas, tercera pasada — POST HOC, DESCRIPTIVO, SIN VEREDICTO.

Solo marxol.db y marxol_fase3.db, en solo lectura y sin red. No abre marxol_vivo.db.
Los retornos son los de V1–V3 en marxol.db, ya vistos: nada de esto decide, y lo
que se use luego va con adenda y se valida en el bloque B.

Población y definiciones: las de scripts/compras_ajenas_posthoc.py y
compras_ajenas2_posthoc.py (538 tokens; compra ajena = compra de una cuenta ≠ creator
y ≠ pagador en slot > slot de creación). Fijadas antes de contar:
1. Posición de la primera compra ajena de 60 s frente a T_entry2, en orden de
   ejecución (entry::execution_order, como scripts/auditoria_fase_a.py; T_entry2 es
   el trade número `trades_before`): antes / es el trade de T_entry2 / después.
   Sin T_entry2 aparte. "Después" incluye el propio trade de T_entry2 en la cifra
   pedida (no se conoce antes de T_entry2) y se da también por separado.
3. Wallets de 60 s en ≥ 2 tokens: días UTC distintos (piloto y V1 = 2026-09-28,
   V2 = 09-30, V3 = 10-01) frente a un solo día.
4. Tipos (V1–V3): sin compra ajena en 60 s / 1.ª compra ajena (de 60 s) a Δslot 1–2 /
   a Δslot > 2. Evaluables por D2 = con T_entry2 y D2 ≠ n/e: retorno (a) congelado a
   +30 min (entry2::return_at, recalculado aquí y comprobado contra `entry2 --json`
   de V1, V2 y V3). Mediana con IC 95 % por estadísticos de orden (aprox. normal a la binomial) y
   % ≤ −50 % con Wilson. No evaluables (sin T_entry2): el retorno (a) no existe; se da
   como sustituto, sin comparabilidad, el precio a t0 + 30 min frente al inicial
   (último punto con t ≤ t0 + 1 800 s, bruto, sin comisiones), con las mismas reglas
   de n/e (cobertura de precio y curva completada).

Uso: python3 -B scripts/compras_ajenas3_posthoc.py --copia /ruta/copia.db
"""
import argparse
import collections
import json
import math
import os
import sqlite3
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from auditoria_fase_a import execution_order  # noqa: E402
from compras_ajenas_posthoc import share, tramo  # noqa: E402
from compras_ajenas2_posthoc import FILL, TRAMOS, d2  # noqa: E402

H = 1800
DAY = {"piloto": "09-28", "V1": "09-28", "V2": "09-30", "V3": "10-01"}


def median_ci(xs, z=1.959964):
    """Mediana e IC 95 % por estadísticos de orden (aprox. normal a la binomial)."""
    xs = sorted(xs)
    n = len(xs)
    if n == 0:
        return None
    med = xs[n // 2] if n % 2 else (xs[n // 2 - 1] + xs[n // 2]) / 2
    lo = max(int(math.floor(n / 2 - z * math.sqrt(n) / 2)), 0)
    hi = min(int(math.ceil(n / 2 + z * math.sqrt(n) / 2)), n - 1)
    return med, xs[lo], xs[hi]


def fmt_med(xs):
    r = median_ci(xs)
    if r is None:
        return "n = 0"
    m, lo, hi = r
    return f"n = {len(xs)}: mediana {100 * m:+.1f} % [IC 95 % {100 * lo:+.1f} – {100 * hi:+.1f}]"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default="marxol.db")
    ap.add_argument("--copia", required=True, help="copia de marxol.db para `entry2`")
    ap.add_argument("--marxol", default="target/release/marxol")
    args = ap.parse_args()
    db = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    q = lambda s, *a: db.execute(s, a).fetchall()

    e2 = {}
    for t in TRAMOS:
        out = subprocess.run([args.marxol, "--db", args.copia, "--json", "entry2", "--check", "--tramo", t],
                             check=True, capture_output=True, text=True).stdout
        e2.update({r["mint"]: r["entry2"] for r in json.loads(out)})
    out = subprocess.run([args.marxol, "--db", args.copia, "--json", "entry2", "--tramo", "validacion"],
                         check=True, capture_output=True, text=True).stdout
    ref_ret = {"V1": {r["mint"]: r["returns"][1] for r in json.loads(out)}}
    out = subprocess.run([args.marxol, "--db", args.copia, "--json", "entry2", "--tramo", "validacion2"],
                         check=True, capture_output=True, text=True).stdout
    ref_ret["V2"] = {r["mint"]: r["ret30"][0] for r in json.loads(out)["tokens"]}
    out = subprocess.run([args.marxol, "--db", args.copia, "--json", "entry2", "--tramo", "validacion3"],
                         check=True, capture_output=True, text=True).stdout
    v3 = json.loads(out)
    # En V3 los tokens de la población sin retorno (excluded.ret_ne) no están en obs: n/e.
    ref_ret["V3"] = {m: None for m in v3["population"]} | {r["mint"]: r["ret"] for r in v3["obs"]}

    rows = q("""SELECT c.mint, c.creator, c.user, c.slot, c.timestamp FROM creations c
                JOIN trade_windows w ON w.mint = c.mint AND w.complete = 1 AND w.window_secs >= 300
                WHERE COALESCE(c.is_mayhem_mode, 0) = 0 ORDER BY c.slot, c.mint""")
    seen, pop = set(), []
    for r in rows:
        if r[1] not in seen:
            seen.add(r[1])
            pop.append(r)

    def return_a(mint, e, horizon, completed):
        exit_t = e["entry_ts"] + H
        if e["fee_bps"] is None or horizon is None or q("SELECT timestamp FROM creations WHERE mint = ?", mint)[0][0] + horizon - 1 < exit_t:
            return None
        if completed is not None and completed <= exit_t:
            return None
        pts = q("SELECT timestamp, quote_reserves * 1.0 / token_reserves FROM price_points WHERE mint = ? AND kind = 'trade' "
                "AND timestamp <= ? ORDER BY timestamp, slot", mint, exit_t)
        if not pts:
            return None
        f = e["fee_bps"] / 1e4
        return pts[-1][1] / e["entry_price"] * (1 - f) * (1 - f) - 1, abs(pts[-1][1] / e["entry_price"] - 1) < 1e-12

    def price_ret_t0(mint, t0, horizon, completed):
        exit_t = t0 + H
        if horizon is None or t0 + horizon - 1 < exit_t or (completed is not None and completed <= exit_t):
            return None
        init = q("SELECT quote_reserves * 1.0 / token_reserves FROM price_points WHERE mint = ? AND kind = 'create'", mint)
        pts = q("SELECT timestamp, quote_reserves * 1.0 / token_reserves FROM price_points WHERE mint = ? AND kind = 'trade' "
                "AND timestamp <= ? ORDER BY timestamp, slot", mint, exit_t)
        if not init:
            return None
        return (pts[-1][1] if pts else init[0][0]) / init[0][0] - 1, not pts

    toks, wallet_toks = [], collections.defaultdict(set)
    ret_cmp, ret_bad = collections.Counter(), collections.Counter()
    for mint, creator, user, cslot, t0 in pop:
        ids = {creator, user}
        e = e2.get(mint)
        horizon = q("SELECT MAX(horizon_secs) FROM price_windows WHERE mint = ? AND complete = 1", mint)[0][0]
        comp = q("SELECT timestamp FROM completions WHERE mint = ?", mint)
        completed = comp[0][0] if comp else None
        tr = [dict(user=u, buy=bool(b), tok=tok, slot=s, ts=ts, v=v, sig=sig, ev=ev) for u, b, tok, s, ts, v, sig, ev in q(
            "SELECT user, is_buy, COALESCE(token_amount, 0), slot, timestamp, COALESCE(virtual_token_reserves, 0), signature, ev_index "
            "FROM early_trades WHERE mint = ? AND timestamp >= ? AND timestamp < ? + 300 ORDER BY slot, timestamp, signature, ev_index",
            mint, t0, t0)]
        foreign60 = [x for x in tr if x["buy"] and x["user"] not in ids and x["slot"] > cslot and x["ts"] < t0 + 60]
        for x in foreign60:
            wallet_toks[x["user"]].add(mint)
        t = dict(mint=mint, tramo=tramo(cslot, t0), b60=bool(foreign60), e=e, pos=None, order_ok=None)
        if foreign60:
            t["ds"] = min(x["slot"] for x in foreign60) - cslot
        if e is not None:
            v0 = q("SELECT token_reserves FROM price_points WHERE mint = ? AND kind = 'create'", mint)[0][0]
            ordered, ok = execution_order(v0, tr)
            k = e["trades_before"]
            assert (ordered[k]["slot"], ordered[k]["ts"]) == (e["entry_slot"], e["entry_ts"]), mint
            t["order_ok"] = ok
            if foreign60:
                keys = {(x["sig"], x["ev"]) for x in foreign60}
                i = next(i for i, x in enumerate(ordered) if (x["sig"], x["ev"]) in keys)
                t["pos"] = "antes" if i < k else "en T_entry2" if i == k else "después"
            t["d2"] = d2(e["dev_sold"], None if e["fill_pct"] is None else e["fill_pct"] >= FILL, e["pairs"] >= 1)
            t["ret"], t["flat"] = return_a(mint, e, horizon, completed) or (None, None)
            ref = ref_ret.get(t["tramo"], {})
            if mint in ref:
                ret_cmp[t["tramo"]] += 1
                a, b = t["ret"], ref[mint]
                ret_bad[t["tramo"]] += (a is None) != (b is None) or (a is not None and abs(a - b) > 1e-9)
        t["ret_t0"], t["no_pts"] = price_ret_t0(mint, t0, horizon, completed) or (None, None)
        toks.append(t)

    print("COMPRAS AJENAS TEMPRANAS, TERCERA PASADA — POST HOC, DESCRIPTIVO, SIN VEREDICTO")
    print("Solo marxol.db (+ marxol_fase3.db en las pasadas anteriores), sin red, sin marxol_vivo.db. IC 95 %.")
    print(f"Población: {len(toks)} tokens. Controles: trade T_entry2 reconstruido = entry2 en todos (assert);")
    print("retorno (a) +30 min recalculado frente a `entry2 --json` (V1: returns[1]; V2: ret30 (a); V3: obs.ret): "
          + "; ".join(f"{k} {ret_cmp[k]} tokens, {ret_bad[k]} distintos" for k in ("V1", "V2", "V3")) + "\n")

    # 1 ---------------------------------------------------------------------------
    print("1. PRIMERA COMPRA AJENA (60 s) FRENTE A T_entry2, EN ORDEN DE EJECUCIÓN")
    w = [t for t in toks if t["b60"]]
    we = [t for t in w if t["e"] is not None]
    print(f"   tokens con compra ajena en 60 s: {len(w)}; sin T_entry2: {share(len(w) - len(we), len(w))}")
    c = collections.Counter(t["pos"] for t in we)
    for k in ("antes", "en T_entry2", "después"):
        print(f"     {k:12} {share(c[k], len(we))}")
    print(f"     en T_entry2 o después (no se conoce antes de T_entry2): {share(c['en T_entry2'] + c['después'], len(we))}")
    print(f"     orden de ejecución incompleto en {sum(not t['order_ok'] for t in we)} de {len(we)} (posición aproximada)")
    nb = [t for t in toks if not t["b60"] and t["e"] is not None]
    print(f"   tokens SIN compra ajena en 60 s pero con T_entry2: {len(nb)} (su T_entry2 llega sin compra ajena previa)\n")

    # 3 ---------------------------------------------------------------------------
    print("3. PERSISTENCIA DE LAS WALLETS DE 60 s EN ≥ 2 TOKENS (días UTC: piloto+V1 09-28, V2 09-30, V3 10-01)")
    tram = {t["mint"]: t["tramo"] for t in toks}
    multi = {wl: ms for wl, ms in wallet_toks.items() if len(ms) >= 2}
    days = {wl: {DAY[tram[m]] for m in ms} for wl, ms in multi.items()}
    nd = collections.Counter(len(d) for d in days.values())
    print(f"   wallets en ≥ 2 tokens: {len(multi)}")
    print(f"     en ≥ 2 días distintos: {share(sum(len(d) >= 2 for d in days.values()), len(multi))}"
          f" (2 días {nd[2]}, 3 días {nd[3]})")
    print(f"     en un solo día:        {share(nd[1], len(multi))}")
    for dd in ("09-28", "09-30", "10-01"):
        only = sum(d == {dd} for d in days.values())
        print(f"       solo {dd}: {only}")
    pv = sum(len({tram[m] for m in ms} & {"piloto", "V1"}) == 2 and days[wl] == {"09-28"} for wl, ms in multi.items())
    print(f"     (de las de un solo día, {pv} están en piloto y V1, mismo día y tramos contiguos)")
    toks_multi_day = set().union(*(multi[wl] for wl in multi if len(days[wl]) >= 2))
    print(f"   tokens con alguna wallet de varios días: {share(len(toks_multi_day), len(toks))}\n")

    # 4 ---------------------------------------------------------------------------
    print("4. RETORNO POR TIPO DE PRIMERA COMPRA AJENA (V1–V3; piloto fuera)")
    print("   evaluables por D2: retorno (a) congelado a +30 min desde T_entry2 (neto de comisión ida y vuelta).")
    print("   no evaluables (sin T_entry2): sustituto, precio a t0 + 30 min / precio inicial − 1, bruto. NO comparable.")
    vt = [t for t in toks if t["tramo"] in ("V1", "V2", "V3")]
    kinds = [("sin compra ajena en 60 s", lambda t: not t["b60"]),
             ("1.ª compra ajena a 1–2 slots", lambda t: t["b60"] and t["ds"] <= 2),
             ("1.ª compra ajena a > 2 slots", lambda t: t["b60"] and t["ds"] > 2)]
    for name, f in kinds:
        g = [t for t in vt if f(t)]
        ev = [t for t in g if t["e"] is not None and t["d2"] is not None]
        ne = [t for t in g if t["e"] is None]
        print(f"   {name}: n = {len(g)}; evaluables {len(ev)}, sin T_entry2 {len(ne)}, D2 n/e {len(g) - len(ev) - len(ne)}")
        for lab, sub in (("evaluables", ev), ("  pasan D2", [t for t in ev if not t["d2"]]),
                         ("  descartados por D2", [t for t in ev if t["d2"]])):
            r = [t["ret"] for t in sub if t["ret"] is not None]
            print(f"     {lab:22} ret (a) +30 min {fmt_med(r)}; ≤ −50 %: {share(sum(x <= -0.5 for x in r), len(r))}"
                  f"; sin retorno (n/e) {len(sub) - len(r)}")
            print(f"     {'':22} precio de salida = precio de T_entry2 (sin movimiento): "
                  f"{share(sum(bool(t['flat']) for t in sub if t['ret'] is not None), len(r))}")
        r = [t["ret_t0"] for t in ne if t["ret_t0"] is not None]
        print(f"     {'sin T_entry2':22} sustituto desde t0  {fmt_med(r)}; ≤ −50 %: {share(sum(x <= -0.5 for x in r), len(r))}"
              f"; n/e {len(ne) - len(r)}")
        print(f"     {'':22} sin ningún trade hasta t0 + 30 min: {share(sum(bool(t['no_pts']) for t in ne if t['ret_t0'] is not None), len(r))}")


if __name__ == "__main__":
    main()
