#!/usr/bin/env python3
"""Compras ajenas tempranas, segunda pasada — POST HOC, DESCRIPTIVO, SIN VEREDICTO.

Solo marxol.db (y marxol_fase3.db para R1 y la muestra de tx de creación), en solo
lectura y sin red. No abre marxol_vivo.db ni lee ningún retorno: D2 sale de
`entry2 --check --json` (código congelado, solo indicadores), al que se le pasa una
copia de marxol.db porque abre la base con Store::open. Nada de esto decide.

Población y definiciones: las de scripts/compras_ajenas_posthoc.py (538 tokens con
ventana completa, sin mayhem, un token por creator; compra ajena = compra de una
cuenta ≠ creator y ≠ pagador en slot > slot de creación). Fijadas antes de contar:
1. Δslot de la primera compra ajena en 5 min = slot − slot de creación (≥ 1).
   Tramos: 1, 2, 3–5, 6–25, 26–150, > 150; sin compra ajena en 5 min aparte.
2. Compras ajenas tempranas = las de 60 s (timestamp < t0 + 60). Cuota de las 10 y
   50 wallets con más compras (empate por dirección); wallets en ≥ 2 tokens.
3. Tokens sin compra ajena en 60 s frente a D2 (2 de 3 entre H4', H6', H7'; H6'
   n/e → v3::d2). Sin T_entry2 o D2 n/e = no evaluable, aparte.
4. Pagadores con R1 ≥ 50 previos (adenda 1) con tokens en la población: creators
   distintos entre todos sus tokens de marxol.db y en la muestra de 10 tx de
   creación de marxol_fase3.db ('operador único' = 1 creator; 'posible servicio' ≥ 2).
IC 95 % de Wilson en cada porcentaje.

Uso: python3 -B scripts/compras_ajenas2_posthoc.py --copia /ruta/copia.db
"""
import argparse
import collections
import json
import os
import subprocess
import sqlite3
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from compras_ajenas_posthoc import r1_map, share  # noqa: E402

TRAMOS = ["piloto", "validacion", "validacion2", "validacion3"]
FILL = 13.65  # H6', entry2::PARAMS.fill_threshold_pct
SLOT_BINS = [("1", 1, 1), ("2", 2, 2), ("3–5", 3, 5), ("6–25", 6, 25), ("26–150", 26, 150), ("> 150", 151, 10**9)]


def d2(h4, h6, h7):
    k = h4 + h7
    if h6 is not None:
        return k + h6 >= 2
    return {2: True, 0: False}.get(k)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default="marxol.db")
    ap.add_argument("--fase3", default="marxol_fase3.db")
    ap.add_argument("--copia", required=True, help="copia de marxol.db para `entry2`")
    ap.add_argument("--marxol", default="target/release/marxol")
    args = ap.parse_args()
    db = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    f3 = sqlite3.connect(f"file:{args.fase3}?mode=ro", uri=True)

    ind, v3_check = {}, {}
    for t in TRAMOS:
        out = subprocess.run([args.marxol, "--db", args.copia, "--json", "entry2", "--check", "--tramo", t],
                             check=True, capture_output=True, text=True).stdout
        for r in json.loads(out):
            e = r["entry2"]
            ind[r["mint"]] = None if e is None else dict(
                h4=e["dev_sold"], h6=None if e["fill_pct"] is None else e["fill_pct"] >= FILL, h7=e["pairs"] >= 1)
            if "d2" in r:
                v3_check[r["mint"]] = (r["h4"], r["h6"], r["h7"], r["d2"])
    bad = sum((i := ind[m]) is None or (i["h4"], i["h6"], i["h7"], d2(i["h4"], i["h6"], i["h7"])) != v
              for m, v in v3_check.items())

    rows = db.execute("""SELECT c.mint, c.creator, c.user, c.slot, c.timestamp FROM creations c
                         JOIN trade_windows w ON w.mint = c.mint AND w.complete = 1 AND w.window_secs >= 300
                         WHERE COALESCE(c.is_mayhem_mode, 0) = 0 ORDER BY c.slot, c.mint""").fetchall()
    seen, pop = set(), []
    for r in rows:
        if r[1] not in seen:
            seen.add(r[1])
            pop.append(r)
    r1 = r1_map(f3)

    first_ds, b60 = {}, {}
    buys60 = collections.Counter()
    wallet_toks = collections.defaultdict(set)
    for mint, creator, user, cslot, t0 in pop:
        ids = {creator, user}
        tr = [(u, s, ts) for u, s, ts in db.execute(
            "SELECT user, slot, timestamp FROM early_trades WHERE mint = ? AND is_buy = 1 AND timestamp < ? + 300",
            (mint, t0)) if u not in ids and s > cslot]
        first_ds[mint] = min((s - cslot for _, s, _ in tr), default=None)
        early = [u for u, _, ts in tr if ts < t0 + 60]
        b60[mint] = bool(early)
        for u in early:
            buys60[u] += 1
            wallet_toks[u].add(mint)

    print("COMPRAS AJENAS TEMPRANAS, SEGUNDA PASADA — POST HOC, DESCRIPTIVO, SIN VEREDICTO")
    print("Solo marxol.db (+ marxol_fase3.db), sin red, sin retornos, sin marxol_vivo.db. IC 95 % de Wilson.")
    print(f"Población: {len(pop)} tokens (ventana completa, sin mayhem, un token por creator).")
    print(f"Control: H4'/H6'/H7'/D2 recalculados frente a `entry2 --check` de V3: {len(v3_check)} tokens, {bad} distintos.\n")

    # 1 ---------------------------------------------------------------------------
    n = len(pop)
    have = [d for d in first_ds.values() if d is not None]
    print("1. Δ SLOTS HASTA LA PRIMERA COMPRA AJENA (slot del trade − slot de creación; ventana de 5 min)")
    print(f"   con compra ajena en 5 min: {share(len(have), n)}")
    for name, lo, hi in SLOT_BINS:
        k = sum(lo <= d <= hi for d in have)
        print(f"   Δ = {name:7} {share(k, len(have)):30} (sobre todos: {share(k, n)})")
    k12 = sum(d <= 2 for d in have)
    print(f"   1–2 slots: {share(k12, len(have))} de los que la tienen; más tarde: {share(len(have) - k12, len(have))}")
    print(f"   sobre todos: 1–2 slots {share(k12, n)}; más tarde {share(len(have) - k12, n)}; ninguna {share(n - len(have), n)}")
    hs = sorted(have)
    if hs:
        pick = lambda p: hs[max(int(-(-p * len(hs) // 1)) - 1, 0)]
        print(f"   entre los que la tienen: mediana {pick(.5)}, p75 {pick(.75)}, p90 {pick(.9)}, máx {hs[-1]} slots\n")

    # 2 ---------------------------------------------------------------------------
    total = sum(buys60.values())
    ranked = sorted(buys60.items(), key=lambda kv: (-kv[1], kv[0]))
    nw = len(ranked)
    print("2. CONCENTRACIÓN DE COMPRADORES AJENOS EN LOS PRIMEROS 60 s")
    print(f"   compras ajenas (TradeEvent de compra): {total}; wallets distintas: {nw}; tokens con alguna: {sum(b60.values())}")
    for top in (10, 50):
        s = sum(c for _, c in ranked[:top])
        toks = set().union(*(wallet_toks[w] for w, _ in ranked[:top])) if ranked else set()
        print(f"   top {top:2} wallets: {share(s, total)} de las compras; aparecen en {share(len(toks), n)} de los tokens")
    multi = [w for w in wallet_toks if len(wallet_toks[w]) >= 2]
    print(f"   wallets en ≥ 2 tokens: {share(len(multi), nw)}; hacen {share(sum(buys60[w] for w in multi), total)} de las compras")
    print(f"   tokens con alguna de esas wallets: {share(len(set().union(*(wallet_toks[w] for w in multi))), n)}")
    dist = collections.Counter(min(len(v), 10) for v in wallet_toks.values())
    print("   wallets por nº de tokens: " + ", ".join(f"{'≥ 10' if k == 10 else k}: {dist[k]}" for k in sorted(dist)))
    print("   top 10 (compras / tokens): " + ", ".join(f"{w[:8]}… {c}/{len(wallet_toks[w])}" for w, c in ranked[:10]) + "\n")

    # 3 ---------------------------------------------------------------------------
    print("3. SOLAPE CON D2 (indicadores en T_entry2, sin retornos)")
    for lab, sel in (("SIN compra ajena en 60 s", False), ("CON compra ajena en 60 s", True)):
        ms = [m for m, *_ in pop if b60[m] == sel]
        ev = [m for m in ms if ind.get(m) is not None and d2(**ind[m]) is not None]
        no_e2 = sum(ind.get(m) is None for m in ms)
        print(f"   {lab}: n = {len(ms)}; sin T_entry2 {share(no_e2, len(ms))}; D2 n/e {len(ms) - no_e2 - len(ev)}")
        print(f"     evaluables n = {len(ev)}: D2 descarta {share(sum(d2(**ind[m]) for m in ev), len(ev))}")
        for h, name in (("h4", "H4'"), ("h7", "H7'")):
            print(f"       {name} {share(sum(ind[m][h] for m in ev), len(ev))}")
        h6 = [m for m in ev if ind[m]["h6"] is not None]
        print(f"       H6' {share(sum(ind[m]['h6'] for m in h6), len(h6))} (con H6' evaluable)")
        print(f"       alguno de H4'/H6'/H7' {share(sum(any(v for v in ind[m].values()) for m in ev), len(ev))}")
    print("   (sin T_entry2 = ningún trade llega a la condición de T_entry2 en 5 min: D2 no aplica)\n")

    # 4 ---------------------------------------------------------------------------
    print("4. PAGADORES CON R1 ≥ 50 PREVIOS EN LA POBLACIÓN: ¿MISMO CREATOR O VARIOS?")
    payers = {u for m, c, u, *_ in pop if r1.get(m) == "R1 sí (≥ 50)"}
    sample = collections.defaultdict(list)
    for p, c in f3.execute("SELECT payer, creator FROM ph_create_tx WHERE kind LIKE 'pagador%' AND creator IS NOT NULL"):
        sample[p].append(c)
    kinds = collections.Counter()
    tok_kind = collections.Counter()
    self_creator = 0
    for p in sorted(payers):
        cs = [c for (c,) in db.execute("SELECT creator FROM creations WHERE user = ?", (p,))]
        s = sample.get(p, [])
        k = ("sin muestra" if not s else "operador único" if len(set(s)) == 1 else "posible servicio")
        kinds[k] += 1
        tok_kind[k] += sum(1 for m, _, u, *_ in pop if u == p and r1.get(m) == "R1 sí (≥ 50)")
        self_creator += bool(s) and all(c == p for c in s)
        print(f"   {p[:12]}…  tokens en marxol.db {len(cs):3}, creators distintos {len(set(cs)):3}; "
              f"muestra de tx {len(s):2}: {len(set(s)):2} creators, {sum(c == p for c in s):2} = pagador → {k}")
    np_ = len(payers)
    print(f"   pagadores: {np_}")
    for k in ("operador único", "posible servicio", "sin muestra"):
        print(f"     {k}: {share(kinds[k], np_)}; tokens de la población con R1 ≥ 50: {share(tok_kind[k], sum(tok_kind.values()))}")
    print(f"     operador único con creator = pagador en toda la muestra: {self_creator}")
    print("   Con estos datos no se distingue un servicio de lanzamiento de un operador que rota creators;")
    print("   y un 'operador único' puede ser una muestra de 10 que no ve otros creators.")


if __name__ == "__main__":
    main()
