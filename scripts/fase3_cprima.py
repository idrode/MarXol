#!/usr/bin/env python3
"""Fase 3, opción C': informe POST HOC de lanzamientos previos del pagador (SIN VEREDICTO).

Lee marxol_fase3.db (resultado de `marxol payer-history`), marxol.db y el libro de
CU de marxol_vivo.db, todo en solo lectura. No lee ningún resultado de
marxol_vivo.db (solo cu_ledger_runs).

LÍMITE (va con cada cifra): lanzamientos previos = creaciones sin error, en los
7 días anteriores, firmadas (pagadas) por la misma wallet. Es un MÍNIMO: no ve las
creaciones con el mismo `creator` pagadas por otra wallet ni operadores con varias
wallets, y un pagador truncado por el tope de páginas puede tener más.

Uso: python3 -B scripts/fase3_cprima.py [--fase3 marxol_fase3.db] [--db marxol.db] [--vivo marxol_vivo.db]
"""
import argparse
import math
import sqlite3

DAYS = 7
TRAMOS = [("V1", 1790604220, 1790604673), ("V2", 1790730000, 1790730479), ("V3", 1790845200, 1790845680)]


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
    return f"{k}/{n} = {100 * k / n:.1f} % [IC 95 % {100 * lo:.1f}–{100 * hi:.1f}]"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fase3", default="marxol_fase3.db")
    ap.add_argument("--db", default="marxol.db")
    ap.add_argument("--vivo", default="marxol_vivo.db")
    a = ap.parse_args()
    f3 = sqlite3.connect(f"file:{a.fase3}?mode=ro", uri=True)
    f3.execute(f"ATTACH 'file:{a.db}?mode=ro' AS m")
    rows = f3.execute(f"""
        SELECT t.mint, t.payer, t.creator, t.t0,
               (SELECT COUNT(*) FROM ph_payer_sigs p JOIN ph_ma_sigs s ON s.signature = p.signature
                WHERE p.payer = t.payer AND s.failed = 0 AND s.signature != t.signature
                  AND s.block_time >= t.t0 - {DAYS * 86400} AND s.block_time < t.t0),
               COALESCE((SELECT reached_end = 1 OR oldest_bt < t.t0 - {DAYS * 86400} FROM ph_payer_scan x WHERE x.payer = t.payer), -1),
               (SELECT MIN(block_time) FROM ph_ma_sigs) <= t.t0 - {DAYS * 86400}
        FROM ph_tokens t""").fetchall()
    kinds = {}
    for payer, n, distinct, same in f3.execute("""
            SELECT payer, COUNT(*), COUNT(DISTINCT creator), SUM(creator = payer)
            FROM ph_create_tx WHERE kind LIKE 'pagador%' AND creator IS NOT NULL GROUP BY payer"""):
        kinds[payer] = ("operador único" if distinct == 1 else "posible servicio", n, distinct, same)

    def bucket(n, complete, covered):
        if complete == -1 or not covered:
            return None
        if n >= 50:
            return "≥ 50"
        if not complete:
            return None
        return "0" if n == 0 else "1–4" if n < 5 else "5–49"

    print("FASE 3, OPCIÓN C' — LANZAMIENTOS PREVIOS DEL PAGADOR EN 7 DÍAS — POST HOC, EXPLORATORIO, SIN VEREDICTO")
    print("Límite: es un MÍNIMO. No ve creaciones del mismo `creator` pagadas por otra wallet (17 % de las creaciones de")
    print("marxol.db tienen creator ≠ pagador) ni operadores con varias wallets; un pagador truncado puede tener más.")
    print("Todo % lleva n e IC 95 % (Wilson). Nada decide: una razón de veto necesita su adenda y el bloque B.\n")
    ma = f3.execute("SELECT COUNT(*), SUM(failed), datetime(MIN(block_time), 'unixepoch'), datetime(MAX(block_time), 'unixepoch') FROM ph_ma_sigs").fetchone()
    print(f"Listado de mint-authority: {ma[0]} firmas ({ma[1]} con error), de {ma[2]} a {ma[3]} UTC.")
    sc = f3.execute("SELECT COUNT(*), SUM(truncated), SUM(reached_end), SUM(pages), MAX(pages) FROM ph_payer_scan").fetchone()
    print(f"Pagadores recorridos: {sc[0]} ({sc[1]} truncados por el tope de 5 páginas, {sc[2]} hasta el principio de la wallet); "
          f"{sc[3]} páginas, máx. {sc[4]} por pagador.")
    pend = f3.execute("SELECT COUNT(DISTINCT payer) FROM ph_tokens WHERE payer NOT IN (SELECT payer FROM ph_payer_scan)").fetchone()[0]
    print(f"Pagadores sin recorrer (fallo persistente, pendientes): {pend}\n")

    names = ["0", "1–4", "5–49", "≥ 50"]
    print("POR TOKEN (lanzamientos previos del pagador, mínimo)")
    for tramo, lo, hi in TRAMOS + [("V1–V3", 0, 1 << 62)]:
        tk = [r for r in rows if lo <= r[3] < hi]
        b = [bucket(r[4], r[5], r[6]) for r in tk]
        ev = [x for x in b if x]
        if not tk:
            continue
        print(f"  {tramo}: {len(tk)} tokens, {len(ev)} clasificables ({len(tk) - len(ev)} truncados con < 50 o sin cubrir)")
        for nm in names:
            print(f"    {nm:5} {share(sum(x == nm for x in ev), len(ev))}")
        print(f"    ≥ 1   {share(sum(x != '0' for x in ev), len(ev))}")

    print("\nPOR PAGADOR (máximo de previos entre sus tokens)")
    per = {}
    for r in rows:
        bk = bucket(r[4], r[5], r[6])
        if bk is None:
            continue
        per[r[1]] = max(per.get(r[1], 0), r[4])
    n = len(per)
    for thr in (1, 5, 50):
        print(f"  ≥ {thr:<3} {share(sum(v >= thr for v in per.values()), n)}")

    print("\nPAGADORES CON ≥ 50 PREVIOS: ¿MISMO CREATOR O VARIOS? (muestra de tx de creación por hash de la firma)")
    print("  'operador único' = todos los creators de su muestra son el mismo; 'posible servicio' = ≥ 2 creators distintos")
    print("  (puede ser un servicio de lanzamiento o un operador que rota creators: con estos datos no se distingue).")
    heavy = sorted({p for p, v in per.items() if v >= 50})
    tok_by = {}
    for r in rows:
        tok_by.setdefault(r[1], []).append(r)
    counts = {}
    for p in heavy:
        kind, ns, distinct, same = kinds.get(p, ("sin muestra", 0, 0, 0))
        insample = {r[2] for r in tok_by[p]}
        counts.setdefault(kind, [0, 0])
        counts[kind][0] += 1
        counts[kind][1] += len(tok_by[p])
        print(f"  {p[:12]}…  previos ≥ {per[p]:4}, tokens en la muestra {len(tok_by[p]):3}, muestra de tx {ns:2}: "
              f"{distinct} creators distintos, {same} = pagador; creators en marxol.db: {len(insample)} → {kind}")
    for kind, (np_, nt) in sorted(counts.items()):
        print(f"  {kind}: {np_} pagadores, {nt} tokens de V1–V3")

    try:
        v = sqlite3.connect(f"file:{a.vivo}?mode=ro", uri=True)
        cu = v.execute("SELECT COUNT(*), SUM(requests), SUM(units) FROM cu_ledger_runs WHERE command = 'payer-history'").fetchone()
        print(f"\nCONSUMO (libro de CU, bolsa manual): {cu[0]} ejecuciones de payer-history, {cu[1]} peticiones, {cu[2]} CU")
    except sqlite3.Error:
        pass


if __name__ == "__main__":
    main()
