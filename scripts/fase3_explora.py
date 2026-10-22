#!/usr/bin/env python3
"""Fase 3, exploración POST HOC sobre marxol.db (solo lectura, sin red, SIN VEREDICTO).

Métrica 1: coste de lanzar e ingresos por comisión de creador.
Métrica 3: quién compra en el slot de creación (creador, otros en el mismo slot,
snipers en los 2 slots siguientes) y qué resultado tiene a los 5 min, sin
comisiones y con comisiones y salida realista por la curva.

Opcional: --fase3 marxol_fase3.db (resultado de `marxol payer-history`, solo
lectura) para medir el coste de lanzar en la muestra de tx de creación y para
agrupar por lanzamientos previos del pagador.

marxol_vivo.db queda reservada para validar (docs/preregistro_fase5_2026-10-22.md):
este script no la abre. Nada de esto decide; lo que se use como razón de veto
se fija en una adenda del pre-registro y se valida en el bloque B.

Uso: python3 -B scripts/fase3_explora.py [--db marxol.db] [--fase3 marxol_fase3.db]
"""
import argparse
import math
import os
import sqlite3
import statistics as stx

SOL = "11111111111111111111111111111111"
LAMPORTS = 1e9
SUPPLY = 1e15
DAYS = 7
# Coste de lanzar: n mínimo para citar "% que cubre el coste".
MIN_COST_N = 100


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
    return (f"n = {len(v)}: p10 {pct(v, .1):.4g}{unit}, mediana {stx.median(v):.4g}{unit}, "
            f"p75 {pct(v, .75):.4g}{unit}, p90 {pct(v, .9):.4g}{unit}, máx {max(v):.4g}{unit}")


def wilson(k, n, z=1.959964):
    if n == 0:
        return float("nan"), float("nan")
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return c - h, c + h


def share(k, n):
    """'k/n = x % [IC 95 % Wilson lo–hi]'."""
    if n == 0:
        return "0/0"
    lo, hi = wilson(k, n)
    return f"{k}/{n} = {100 * k / n:.1f} % [IC 95 % {100 * lo:.1f}–{100 * hi:.1f}]"


def payer_groups(f3):
    """mint → grupo por lanzamientos previos del pagador en 7 d (C', mínimo).
    Los truncados tienen ya ≥ 54 contados, así que entran en '≥ 50' con seguridad;
    un truncado con menos no se puede clasificar y queda fuera."""
    if f3 is None:
        return {}, {}
    rows = q(f3, f"""
        SELECT t.mint, t.payer,
               (SELECT COUNT(*) FROM ph_payer_sigs p JOIN ph_ma_sigs m ON m.signature = p.signature
                WHERE p.payer = t.payer AND m.failed = 0 AND m.signature != t.signature
                  AND m.block_time >= t.t0 - {DAYS * 86400} AND m.block_time < t.t0),
               (s.reached_end = 1 OR s.oldest_bt < t.t0 - {DAYS * 86400}),
               (SELECT MIN(block_time) FROM ph_ma_sigs) <= t.t0 - {DAYS * 86400}
        FROM ph_tokens t JOIN ph_payer_scan s ON s.payer = t.payer""")
    kind = payer_kind(f3)
    out = {}
    for mint, payer, n, complete, covered in rows:
        if not covered:
            continue
        if n >= 50:
            out[mint] = "≥ 50, " + kind.get(payer, "sin muestra de creators")
        elif not complete:
            continue
        elif n == 0:
            out[mint] = "0"
        elif n < 5:
            out[mint] = "1–4"
        else:
            out[mint] = "5–49"
    return out, kind


def payer_kind(f3):
    """Pagador con ≥ 50 → 'operador único' si todos los creators de su muestra de tx
    de creación son el mismo; 'posible servicio' si hay ≥ 2 distintos."""
    kinds = {}
    for payer, n, distinct, same in q(f3, """
            SELECT payer, COUNT(*), COUNT(DISTINCT creator), SUM(creator = payer)
            FROM ph_create_tx WHERE kind LIKE 'pagador%' AND creator IS NOT NULL GROUP BY payer"""):
        kinds[payer] = "operador único" if distinct == 1 else "posible servicio"
    return kinds


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default="marxol.db")
    ap.add_argument("--fase3", default=None)
    args = ap.parse_args()
    db = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    f3 = sqlite3.connect(f"file:{args.fase3}?mode=ro", uri=True) if args.fase3 and os.path.exists(args.fase3) else None
    print("FASE 3, EXPLORACIÓN POST HOC sobre marxol.db (piloto, V1, V2 y V3) — SIN VEREDICTO, EXPLORATORIO")
    print("Solo describe. Nada de esto decide; lo que se use como razón de veto se fija en una adenda")
    print("del pre-registro y se valida en el bloque B de marxol_vivo.db. Todo % lleva n e IC 95 % (Wilson).\n")

    # ---- 1a. Coste de lanzar --------------------------------------------------------
    costs = []
    if f3 is not None:
        for delta, buys, sells in q(f3, """SELECT payer_delta, buys, sells FROM ph_create_tx
                                           WHERE kind = 'coste' AND payer_delta IS NOT NULL AND decode_errors = 0"""):
            costs.append(delta - buys + sells)
    print("1a. COSTE DE LANZAR (muestra de tx de creación de marxol.db por hash de la firma, marxol_fase3.db)")
    print("    = lo que pierde el pagador en la tx − lo que pone en sus compras de la misma tx (sol + comisiones)")
    print("    + lo que recibe de sus ventas. Incluye rent de las cuentas, comisión, prioridad y propinas.")
    print(f"    {desc(costs, ' SOL', LAMPORTS)}")
    cost_ok = len(costs) >= MIN_COST_N
    launch_cost = stx.median(costs) if costs else float("nan")
    if not cost_ok:
        print(f"    n < {MIN_COST_N}: no se cita ningún '% que cubre el coste'.")
    print()

    # ---- 1b. Comisión de creador --------------------------------------------------
    groups, kinds = payer_groups(f3)
    toks = q(db, """SELECT c.mint, c.creator, c.timestamp FROM creations c
                    JOIN trade_windows w ON w.mint = c.mint AND w.complete = 1
                    WHERE COALESCE(c.quote_mint, ?) = ?""", SOL, SOL)
    fee5, fee65, no_bps = {}, {}, 0
    for mint, creator, t0 in toks:
        rows = q(db, "SELECT sol_amount, creator_fee_basis_points FROM early_trades WHERE mint = ? AND timestamp < ? + 300",
                 mint, t0)
        if any(b is None for _, b in rows):
            no_bps += 1
            continue
        fee5[mint] = sum(s * b / 1e4 for s, b in rows)
        last_bps = rows[-1][1] if rows else 0
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
            fee65[mint] = fee5[mint] + sum(abs(b - a) for a, b in zip(seq, seq[1:])) * last_bps / 1e4
    print("1b. INGRESO POR COMISIÓN DE CREADOR (tokens en SOL con ventana temprana completa)")
    print(f"    {len(toks)} tokens; {no_bps} sin creator_fee_basis_points en algún trade (filas antiguas) quedan fuera.")
    print("    5 min: exacto (sol_amount × creator_fee_bps de cada TradeEvent). 1 h: aproximado (5 min exactos")
    print("    + |Δ reservas| × bps del último trade de la ventana). Solo la curva: no incluye PumpSwap tras graduar.")
    sel = [("todos", lambda m: True)]
    if groups:
        # En 'posible servicio' la comisión va al creator, que a menudo no es el pagador: no se mezcla.
        sel.append(("todos salvo 'posible servicio' (con C')", lambda m: m in groups and groups[m] != "≥ 50, posible servicio"))
        print(f"    Grupos por lanzamientos previos del PAGADOR en {DAYS} d (C', mínimo: no ve creaciones del mismo")
        print("    creator pagadas por otra wallet ni operadores con varias wallets). Solo tokens de V1–V3 con C'.")
        for g in ["0", "1–4", "5–49", "≥ 50, operador único", "≥ 50, posible servicio", "≥ 50, sin muestra de creators"]:
            label = f"pagador con {g} previos" if not g.startswith("≥") else f"pagador con ≥ 50 previos ({g.split(', ', 1)[1]})"
            sel.append((label, lambda m, g=g: groups.get(m) == g))
    print("    La comisión de creador va al `creator` del token, que en 'posible servicio' a menudo no es el pagador.")
    for name, f in sel:
        a = [v for m, v in fee5.items() if f(m)]
        b = [v for m, v in fee65.items() if f(m)]
        if not a:
            continue
        print(f"    {name}:")
        print(f"      5 min  {desc(a, ' SOL', LAMPORTS)}")
        print(f"      1 h    {desc(b, ' SOL', LAMPORTS)}")
        if cost_ok and b and name.endswith("(posible servicio)"):
            print(f"      1 h > coste de lanzar mediano: {share(sum(v > launch_cost for v in b), len(b))} — NO COMPARABLE: la comisión")
            print("      la cobra el creator del token y el coste lo paga el pagador (otra wallet en parte de los casos)")
        elif cost_ok and b:
            print(f"      1 h > coste de lanzar mediano ({launch_cost / LAMPORTS:.4f} SOL): {share(sum(v > launch_cost for v in b), len(b))}")
    print()

    # ---- 3. Slot de creación -------------------------------------------------------
    print("3. QUIÉN COMPRA EN EL SLOT DE CREACIÓN (tokens con ventana completa)")
    print("    creador = creator o pagador del CreateEvent; 'otros, mismo slot' = otras wallets en el slot de creación")
    print("    (bundle o sniper muy rápido: sin las propinas a Jito no se distinguen); 'snipers' = slots +1 y +2.")
    print("    Cada wallet va a la categoría de su primera compra; se siguen todos sus trades de los 5 min.")
    print("    Resultados a 5 min, solo tokens en SOL, en tres versiones:")
    print("      bruto:      ventas − compras + lo que sigue teniendo × precio del último trade (sin comisiones)")
    print("      realizado:  ventas − compras dentro de los 5 min (no cuenta lo que sigue teniendo)")
    print("      realista:   con las comisiones de cada trade (fee + creator fee, bps del TradeEvent) y vendiendo lo que")
    print("                  sigue teniendo de una vez por la curva (producto constante sobre las reservas virtuales del")
    print("                  último trade, con su deslizamiento) pagando las comisiones del último trade. Para el creador,")
    print("                  la comisión de creador de sus propios trades cuenta como coste (vuelve a él: conservador) y")
    print("                  no se suma su ingreso por comisión de creador (1b).\n")
    allt = q(db, """SELECT c.mint, c.creator, c.user, c.slot, c.timestamp, COALESCE(c.quote_mint, ?) = ?
                    FROM creations c JOIN trade_windows w ON w.mint = c.mint AND w.complete = 1""", SOL, SOL)
    cats = ("creador", "otros, mismo slot", "snipers +1/+2")
    bought_share = {k: [] for k in cats}
    res = {(k, v): [] for k in cats for v in ("bruto", "realizado", "realista")}
    any_other = by_tok = 0
    no_fee_tokens = 0
    for mint, creator, user, cslot, t0, in_sol in allt:
        tr = q(db, """SELECT user, is_buy, quote_amount, token_amount, slot,
                             COALESCE(virtual_quote_reserves, virtual_sol_reserves), virtual_token_reserves,
                             fee_basis_points, creator_fee_basis_points
                      FROM early_trades WHERE mint = ? AND timestamp < ? + 300 ORDER BY slot, timestamp, ev_index""", mint, t0)
        if not tr:
            continue
        by_tok += 1
        last = tr[-1]
        vq, vt = last[5] or 0, last[6] or 0
        p_end = vq / vt if vq and vt else 0.0
        has_fees = all(r[7] is not None and r[8] is not None for r in tr)
        last_bps = (last[7] or 0) + (last[8] or 0)
        ids = {creator, user}

        def cat(u, s):
            if u in ids:
                return "creador" if s == cslot else None
            if s == cslot:
                return "otros, mismo slot"
            if cslot < s <= cslot + 2:
                return "snipers +1/+2"
            return None

        first = {}
        for r in tr:
            if r[1] and r[0] not in first:
                first[r[0]] = cat(r[0], r[4])
        bought = {k: 0 for k in cats}
        for r in tr:
            k = cat(r[0], r[4])
            if r[1] and k:
                bought[k] += r[3] or 0
        for k in cats:
            bought_share[k].append(100 * bought[k] / SUPPLY)
        any_other += bought["otros, mismo slot"] > 0
        if not in_sol:
            continue
        if not has_fees:
            no_fee_tokens += 1
        for k in cats:
            us = {u for u, kk in first.items() if kk == k}
            if not us:
                continue
            gross = net = held = 0
            for u, b, qa, ta, s, _, _, fb, cb in tr:
                if u not in us:
                    continue
                qa, ta = qa or 0, ta or 0
                bps = (fb or 0) + (cb or 0)
                if b:
                    gross -= qa
                    net -= qa * (1 + bps / 1e4)
                    held += ta
                else:
                    gross += qa
                    net += qa * (1 - bps / 1e4)
                    held -= ta
            res[(k, "bruto")].append(gross + held * p_end)
            res[(k, "realizado")].append(gross)
            if has_fees and vq and vt:
                h = max(held, 0)
                exit_q = (vq - vq * vt / (vt + h)) * (1 - last_bps / 1e4) if h else 0
                res[(k, "realista")].append(net + exit_q)
    print(f"    tokens: {by_tok}; con compras de otras wallets en el slot de creación: {share(any_other, by_tok)}")
    for k in cats:
        print(f"    {k:18} % del supply comprado:  {desc(bought_share[k], ' %')}")
    print(f"    (versión realista: {no_fee_tokens} tokens en SOL sin bps de comisión en algún trade quedan fuera)")
    for k in cats:
        for v in ("bruto", "realizado", "realista"):
            xs = res[(k, v)]
            if not xs:
                continue
            # Restos de redondeo de hasta 1 000 lamports cuentan como 0.
            pos = sum(x > 1000 for x in xs)
            print(f"    {k:18} {v:10} {desc(xs, ' SOL', LAMPORTS)}")
            print(f"    {'':18} {'':10} > 0: {share(pos, len(xs))}")


if __name__ == "__main__":
    main()
