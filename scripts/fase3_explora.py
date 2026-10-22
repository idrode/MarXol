#!/usr/bin/env python3
"""Fase 3, exploración POST HOC sobre marxol.db (solo lectura, sin red, sin veredicto).

Métrica 1: ingresos por comisión de creador en las primeras horas y coste de lanzar.
Métrica 3: quién compra en el slot de creación (creador, otros en el mismo slot,
snipers en los 2 slots siguientes) y cuánto se lleva a los 5 min.

marxol_vivo.db queda reservada para validar (docs/preregistro_fase5_2026-10-22.md):
este script no la abre.

Uso: python3 -B scripts/fase3_explora.py [--db marxol.db] > resultados/fase3_explora.txt
"""
import argparse
import json
import os
import sqlite3
import statistics as stx

SOL = "11111111111111111111111111111111"
LAMPORTS = 1e9
SUPPLY = 1e15


def q(db, sql, *a):
    return db.execute(sql, a).fetchall()


def pct(xs, p):
    xs = sorted(xs)
    if not xs:
        return float("nan")
    k = (len(xs) - 1) * p
    f = int(k)
    c = min(f + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def desc(xs, unit="", scale=1.0):
    if not xs:
        return "n = 0"
    v = [0.0 if abs(x) < 1e-6 * scale else x / scale for x in xs]
    return (f"n = {len(v)}: mediana {stx.median(v):.4g}{unit}, p75 {pct(v, .75):.4g}{unit}, "
            f"p90 {pct(v, .9):.4g}{unit}, máx {max(v):.4g}{unit}")


def launch_costs(db, fixtures):
    """Coste de lanzar medido en las tx de creación de tests/fixtures/tx: lo que
    pierde el pagador menos lo que paga por la compra inicial (sol + comisiones)."""
    out = []
    for f in sorted(os.listdir(fixtures)):
        sig = f[:-5]
        c = q(db, "SELECT mint, user FROM creations WHERE signature = ?", sig)
        if not c:
            continue
        mint, user = c[0]
        tx = json.load(open(os.path.join(fixtures, f)))
        t = tx.get("transaction", tx)
        msg = t["transaction"]["message"] if "transaction" in t else t["message"]
        meta = t.get("meta") or tx.get("meta")
        keys = [k if isinstance(k, str) else k.get("pubkey") for k in msg["accountKeys"]]
        i = keys.index(user)
        spent = meta["preBalances"][i] - meta["postBalances"][i]
        buys = q(db, """SELECT sol_amount, COALESCE(fee_basis_points, 0), COALESCE(creator_fee_basis_points, 0)
                         FROM early_trades WHERE signature = ? AND is_buy = 1 AND user = ?""", sig, user)
        buy = sum(s * (1 + (fb + cb) / 1e4) for s, fb, cb in buys)
        out.append((sig, spent, buy, spent - buy, meta["fee"]))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default="marxol.db")
    args = ap.parse_args()
    db = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    print("FASE 3, EXPLORACIÓN POST HOC sobre marxol.db (piloto, V1, V2 y V3) — SIN VEREDICTO")
    print("Solo describe. Nada de esto decide; lo que se use como razón de veto se fija en una adenda")
    print("del pre-registro y se valida en el bloque B de marxol_vivo.db.\n")

    # ---- Métrica 1: coste de lanzar -------------------------------------------------
    fx = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "tests", "fixtures", "tx")
    costs = launch_costs(db, fx)
    print("1a. COSTE DE LANZAR (tx de creación de tests/fixtures/tx; pérdida del pagador − compra inicial con comisiones)")
    for sig, spent, buy, cost, fee in costs:
        print(f"    {sig[:12]}…  pagador −{spent / LAMPORTS:.6f} SOL, compra inicial {buy / LAMPORTS:.6f}, "
              f"coste de lanzar {cost / LAMPORTS:.6f} SOL (comisión de la tx {fee / LAMPORTS:.6f})")
    launch_cost = stx.median([c[3] for c in costs]) if costs else float("nan")
    print(f"    mediana: {launch_cost / LAMPORTS:.6f} SOL (n = {len(costs)}; incluye rent de las cuentas, comisión y prioridad)\n")

    # ---- Métrica 1: comisión de creador -------------------------------------------
    toks = q(db, """SELECT c.mint, c.creator, c.user, c.timestamp, COALESCE(c.is_mayhem_mode, 0)
                    FROM creations c JOIN trade_windows w ON w.mint = c.mint AND w.complete = 1
                    WHERE COALESCE(c.quote_mint, ?) = ?""", SOL, SOL)
    n_creator = {r[0]: r[1] for r in q(db, "SELECT creator, COUNT(*) FROM creations GROUP BY creator")}
    fee5 = {}
    fee65 = {}
    no_bps = 0
    for mint, creator, user, t0, mayhem in toks:
        rows = q(db, """SELECT sol_amount, creator_fee_basis_points FROM early_trades
                        WHERE mint = ? AND timestamp < ? + 300""", mint, t0)
        if any(b is None for _, b in rows):
            no_bps += 1
            continue
        fee5[mint] = sum(s * b / 1e4 for s, b in rows)
        bps = [b for _, b in rows]
        last_bps = bps[-1] if bps else 0
        pw = q(db, "SELECT MAX(horizon_secs) FROM price_windows WHERE mint = ? AND complete = 1", mint)[0][0]
        if pw and pw >= 3600:
            # Tras los 5 min solo hay reservas: |Δ reservas virtuales de quote| entre puntos consecutivos
            # ≈ importe de cada trade (aproximado: el orden dentro de un slot no está reconstruido).
            pts = q(db, """SELECT quote_reserves FROM price_points WHERE mint = ? AND kind = 'trade'
                           AND timestamp >= ? + 300 AND timestamp < ? + 3600 ORDER BY timestamp, slot, ev_index""",
                    mint, t0, t0)
            prev = q(db, """SELECT quote_reserves FROM price_points WHERE mint = ? AND timestamp < ? + 300
                            ORDER BY timestamp DESC, slot DESC, ev_index DESC LIMIT 1""", mint, t0)
            seq = [p[0] for p in prev] + [p[0] for p in pts]
            vol = sum(abs(b - a) for a, b in zip(seq, seq[1:]))
            fee65[mint] = fee5[mint] + vol * last_bps / 1e4
    groups = {"todos": lambda m, c: True,
              "creador con 1 token en la muestra": lambda m, c: n_creator[c] == 1,
              "creador con ≥ 2 tokens en la muestra": lambda m, c: n_creator[c] >= 2}
    cre = {m: c for m, c, *_ in toks}
    print("1b. INGRESO POR COMISIÓN DE CREADOR (tokens en SOL con ventana temprana completa)")
    print(f"    {len(toks)} tokens; {no_bps} sin creator_fee_basis_points en algún trade (filas antiguas) quedan fuera.")
    print("    Ventana de 5 min: exacta (sol_amount × creator_fee_bps de cada TradeEvent).")
    print("    Primera hora: aproximada (5 min exactos + |Δ reservas| × bps del último trade de la ventana).")
    print("    'Creador con ≥ 2 tokens' es solo dentro de la muestra (4 franjas de ~8 min): mínimo, no historial.")
    for g, f in groups.items():
        a = [v for m, v in fee5.items() if f(m, cre[m])]
        b = [v for m, v in fee65.items() if f(m, cre[m])]
        over = sum(v > launch_cost for v in b)
        print(f"    {g}:")
        print(f"      5 min  {desc(a, ' SOL', LAMPORTS)}")
        print(f"      1 h    {desc(b, ' SOL', LAMPORTS)}")
        if b:
            print(f"      supera el coste de lanzar ({launch_cost / LAMPORTS:.4f} SOL) en 1 h: {over}/{len(b)} = {100 * over / len(b):.1f} %")
    print()

    # ---- Métrica 3: slot de creación ----------------------------------------------
    print("3. QUIÉN COMPRA EN EL SLOT DE CREACIÓN (todos los tokens con ventana completa, cualquier quote)")
    print("    creador = creator o pagador del CreateEvent; 'otros, mismo slot' = otras wallets en el slot de creación")
    print("    (bundle o sniper muy rápido: sin las propinas a Jito no se distinguen); 'snipers' = slots +1 y +2.")
    print("    Resultado a 5 min marcado a mercado con el precio del último trade de la ventana, sin comisiones.\n")
    allt = q(db, """SELECT c.mint, c.creator, c.user, c.slot, c.signature, c.timestamp, COALESCE(c.quote_mint, ?) = ?
                    FROM creations c JOIN trade_windows w ON w.mint = c.mint AND w.complete = 1""", SOL, SOL)
    cats = ("creador", "otros, mismo slot", "snipers +1/+2")
    share = {k: [] for k in cats}
    pnl = {k: [] for k in cats}
    realized = {k: [] for k in cats}
    any_other = 0
    by_tok = 0
    for mint, creator, user, cslot, csig, t0, in_sol in allt:
        tr = q(db, """SELECT user, is_buy, quote_amount, token_amount, slot, virtual_quote_reserves, virtual_token_reserves
                      FROM early_trades WHERE mint = ? AND timestamp < ? + 300 ORDER BY slot, timestamp, ev_index""", mint, t0)
        if not tr:
            continue
        by_tok += 1
        last = tr[-1]
        p_end = (last[5] / last[6]) if last[5] and last[6] else 0.0
        ids = {creator, user}

        def cat(u, s):
            if u in ids:
                return "creador" if s == cslot else None
            if s == cslot:
                return "otros, mismo slot"
            if cslot < s <= cslot + 2:
                return "snipers +1/+2"
            return None
        # Wallets por categoría según su primera compra.
        first = {}
        for u, b, qa, ta, s, *_ in tr:
            if b and u not in first:
                first[u] = cat(u, s)
        bought = {k: 0 for k in cats}
        for u, b, qa, ta, s, *_ in tr:
            k = cat(u, s)
            if b and k:
                bought[k] += ta or 0
        for k in cats:
            share[k].append(100 * bought[k] / SUPPLY)
        if bought["otros, mismo slot"] > 0:
            any_other += 1
        flows = {}
        for u, b, qa, ta, s, *_ in tr:
            k = first.get(u)
            if not k:
                continue
            q_in, t_in = flows.get(u, (0, 0))
            flows[u] = (q_in + (-(qa or 0) if b else (qa or 0)), t_in + ((ta or 0) if b else -(ta or 0)))
        for k in cats:
            us = [u for u, kk in first.items() if kk == k]
            if us and in_sol:
                pnl[k].append(sum(flows[u][0] + flows[u][1] * p_end for u in us))
                realized[k].append(sum(flows[u][0] for u in us))
    print(f"    tokens: {by_tok}; con compras de otras wallets en el slot de creación: {any_other} = {100 * any_other / by_tok:.1f} %")
    for k in cats:
        print(f"    {k:18} % del supply comprado:  {desc(share[k], ' %')}")
    for k in cats:
        print(f"    {k:18} resultado a 5 min por token, solo tokens en SOL:  {desc(pnl[k], ' SOL', LAMPORTS)}")
        if pnl[k]:
            # Restos de 1 lamport por redondeo de reservas cuentan como 0.
            pos = sum(v > 1000 for v in pnl[k])
            print(f"    {'':18} con resultado > 1e-6 SOL: {pos}/{len(pnl[k])} = {100 * pos / len(pnl[k]):.1f} %")
            pr = sum(v > 1000 for v in realized[k])
            print(f"    {'':18} realizado (ventas − compras en 5 min):  {desc(realized[k], ' SOL', LAMPORTS)}; > 0 en {pr}/{len(realized[k])} = {100 * pr / len(realized[k]):.1f} %")
    print("    (marcado a mercado sobrevalora posiciones grandes: vender todo movería el precio; el realizado no cuenta lo que siguen teniendo)")


if __name__ == "__main__":
    main()
