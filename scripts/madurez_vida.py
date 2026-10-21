#!/usr/bin/env python3
"""Descriptores de vida de los pools graduados de un día — POST HOC, SIN PRE-REGISTRO,
SOLO DESCRIPTIVO. Sirve para elegir el umbral del universo de tokens maduros. No calcula
caídas, subidas, drawdowns ni segundas vidas.

Modos (desde la raíz del repo):
    python3 -B scripts/madurez_vida.py --download [--day 2026-08-20]   # GeckoTerminal, tope 1 600 llamadas
    python3 -B scripts/madurez_vida.py --chain    [--day 2026-08-20]   # saldos actuales de los vaults (Alchemy)
    python3 -B scripts/madurez_vida.py --report   [--day 2026-08-20]   # solo lee madurez.db

--download: velas de 1 h en USD (currency=usd) de cada pool de chain_migrations del día,
  con before_timestamp = migración + 31 días (una sola llamada por pool: 744 h < 1000).
  Reanudable (gt_ohlcv_done con currency 'usd'). Las 40 de la fase 0b están en
  currency=token y no se reutilizan (el volumen no está en USD).
--chain: saldos de los vaults base y quote de cada pool (getMultipleAccounts,
  jsonParsed). Es la liquidez real de hoy, no la histórica: en PumpSwap el producto
  de reservas no se conserva (comprobado en 8 pools, 2026-10-21), así que la liquidez
  pasada no se puede reconstruir desde el precio.
Escribe solo en madurez.db y en rpc_usage de marxol.db.
"""
import argparse, datetime, json, sqlite3, statistics, sys, time
import madurez_fase0b as f0

ATA = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
TOKEN = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
TOKEN22 = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
USDC = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
H = 3600
AGES = [(24 * H, "24 h"), (72 * H, "72 h"), (7 * 24 * H, "7 d"), (14 * 24 * H, "14 d"), (30 * 24 * H, "30 d")]
VOL = [0, 1_000, 5_000, 25_000]   # volumen USD en las 24 h anteriores a la edad (0 = algún volumen)
LIQ = [1_000, 5_000, 25_000]      # liquidez USD actual en cadena (2 × lado quote)

SCHEMA = """
CREATE TABLE IF NOT EXISTS chain_pool_balances (pool TEXT PRIMARY KEY, fetched_at REAL, slot INTEGER,
    base_ui REAL, quote_ui REAL);  -- NULL = cuenta inexistente
CREATE TABLE IF NOT EXISTS quote_prices (mint TEXT PRIMARY KEY, fetched_at REAL, usd REAL);
"""


class Gecko(f0.Gecko):
    """Como en la fase 0b, con esperas más largas ante 429 (ejecución larga)."""

    def get(self, path):
        import urllib.error
        for wait in (60, 120, 240, 480):
            try:
                return super().get(path)
            except RuntimeError as e:
                if "HTTP 429" not in str(e):
                    raise
            except (urllib.error.URLError, OSError) as e:  # red caída (p. ej. DNS, 2026-10-21)
                print(f"  error de red: {e}; reintento en {wait} s", flush=True)
            time.sleep(wait)
        return super().get(path)


def migrations(db, day):
    return db.execute("SELECT pool, mint, quote_mint, timestamp FROM chain_migrations WHERE day = ? "
                      "ORDER BY slot, mint", (day,)).fetchall()


def download(args):
    mdb = sqlite3.connect(args.madurez_db)
    gt = Gecko(mdb, args.max_gt, args.gt_interval)
    done = n500 = n404 = 0
    try:
        for pool, _, _, t_m in migrations(mdb, args.day):
            if mdb.execute("SELECT 1 FROM gt_ohlcv_done WHERE pool = ? AND timeframe = 'hour' AND currency = 'usd'",
                           (pool,)).fetchone():
                continue
            q = f"?aggregate=1&limit=1000&currency=usd&before_timestamp={t_m + 31 * 24 * H + H}"
            try:
                res = gt.get(f"/pools/{pool}/ohlcv/hour{q}")
            except RuntimeError as e:
                code = -1 if "HTTP 500" in str(e) else -2 if "HTTP 404" in str(e) else None
                if code is None:
                    raise
                n500 += code == -1
                n404 += code == -2
                mdb.execute("INSERT OR REPLACE INTO gt_ohlcv_done VALUES (?,?,?,?)", (pool, "hour", "usd", code))
                mdb.commit()
                continue
            candles = res["data"]["attributes"]["ohlcv_list"]
            mdb.executemany("INSERT OR IGNORE INTO gt_ohlcv VALUES (?,?,?,?,?,?,?,?,?)",
                            [(pool, "hour", "usd", *c) for c in candles])
            mdb.execute("INSERT OR REPLACE INTO gt_ohlcv_done VALUES (?,?,?,1)", (pool, "hour", "usd"))
            mdb.commit()
            done += 1
            if done % 50 == 0:
                print(f"  {done} pools nuevos, {gt.calls} llamadas", flush=True)
    finally:
        print(f"  pools nuevos {done}, HTTP 500 {n500}, 404 {n404}; llamadas a GeckoTerminal {gt.calls} (tope {gt.max_calls})")
        mdb.close()


def chain(args):
    mdb = sqlite3.connect(args.madurez_db)
    mdb.executescript(SCHEMA)
    f0.Alchemy.COST["getMultipleAccounts"] = 10  # supuesto: no figura en la tabla de CU revisada
    rpc = f0.Alchemy(args.max_cu)
    gt = Gecko(mdb, 2, args.gt_interval)
    try:
        migs = migrations(mdb, args.day)
        have = {r[0] for r in mdb.execute("SELECT pool FROM chain_pool_balances")}
        migs = [m for m in migs if m[0] not in have]
        for k in range(0, len(migs), 50):
            chunk = migs[k:k + 50]
            keys = []
            for pool, mint, quote, _ in chunk:
                qm, qprog = (USDC, TOKEN) if quote == USDC else (f0.WSOL, TOKEN)
                keys += [f0.pda([f0.b58decode(pool), f0.b58decode(TOKEN22), f0.b58decode(mint)], ATA),
                         f0.pda([f0.b58decode(pool), f0.b58decode(qprog), f0.b58decode(qm)], ATA)]
            res = rpc.call("getMultipleAccounts", [keys, {"encoding": "jsonParsed", "commitment": "confirmed"}])
            bal = lambda a: float(a["data"]["parsed"]["info"]["tokenAmount"]["uiAmountString"]) if a else None
            for i, (pool, *_rest) in enumerate(chunk):
                mdb.execute("INSERT OR REPLACE INTO chain_pool_balances VALUES (?,?,?,?,?)",
                            (pool, time.time(), res["context"]["slot"], bal(res["value"][2 * i]),
                             bal(res["value"][2 * i + 1])))
            mdb.commit()
        sol = gt.get(f"/tokens/{f0.WSOL}")["data"]["attributes"]["price_usd"]
        mdb.execute("INSERT OR REPLACE INTO quote_prices VALUES (?,?,?)", (f0.WSOL, time.time(), float(sol)))
        mdb.execute("INSERT OR REPLACE INTO quote_prices VALUES (?,?,?)", (USDC, time.time(), 1.0))
        mdb.commit()
    finally:
        rpc.record(args.marxol_db, "madurez-vida-chain")
        print(f"  Alchemy {rpc.calls} = {rpc.cu} CU (tope {rpc.max_cu}); GeckoTerminal {gt.calls}")
        mdb.close()


def pct(xs, q):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(q * len(xs)))] if xs else float("nan")


def report(args):
    db = f0.ro(args.madurez_db)
    migs = migrations(db, args.day)
    L = []
    p = L.append
    p("Descriptores de vida de los graduados — POST HOC, SIN PRE-REGISTRO, SOLO DESCRIPTIVO")
    p(f"Generado con: python3 -B scripts/madurez_vida.py --report --day {args.day}  (solo lee madurez.db)")
    p("Sin caídas, subidas, drawdowns ni segundas vidas: solo actividad, volumen y liquidez, para elegir el umbral del universo.")
    p("")
    status = {pool: db.execute("SELECT complete FROM gt_ohlcv_done WHERE pool = ? AND timeframe = 'hour' AND currency = 'usd'",
                               (pool,)).fetchone() for pool, *_ in migs}
    ok = [m for m in migs if status[m[0]] and status[m[0]][0] == 1]
    p(f"== 1. Cobertura: {len(migs)} migraciones del {args.day}")
    p(f"  velas descargadas {len(ok)}; HTTP 500 {sum(1 for s in status.values() if s and s[0] == -1)}; "
      f"desconocidos por GeckoTerminal (404) {sum(1 for s in status.values() if s and s[0] == -2)}; "
      f"pendientes {sum(1 for s in status.values() if not s)}")
    calls = db.execute("SELECT count(*), sum(status = 200), sum(status = 429) FROM api_calls WHERE url LIKE '%currency=usd%'").fetchone()
    p(f"  llamadas OHLCV usd registradas en api_calls: {calls[0]} ({calls[1]} 200, {calls[2]} 429)")
    p("")
    rows = []
    for pool, mint, quote, t_m in ok:
        c = db.execute("SELECT ts, volume FROM gt_ohlcv WHERE pool = ? AND timeframe = 'hour' AND currency = 'usd' "
                       "AND ts >= ? ORDER BY ts", (pool, t_m - H)).fetchall()
        r = {"pool": pool, "quote": quote}
        for age, name in AGES:
            r["v" + name] = sum(v for ts, v in c if t_m + age - 24 * H <= ts < t_m + age)
            r["h" + name] = sum(1 for ts, v in c if ts < t_m + age and v > 0)
        r["vcum7 d"] = sum(v for ts, v in c if ts < t_m + 7 * 24 * H)
        rows.append(r)
    p("== 2. Volumen USD en las 24 h anteriores a cada edad (vela horaria con ts en [edad − 24 h, edad) desde la migración)")
    p("  edad    con volumen  >= 1k   >= 5k   >= 25k   | mediana   p75      p90      | horas con trades acumuladas: mediana p75 p90")
    for age, name in AGES:
        v = [r["v" + name] for r in rows]
        hh = [r["h" + name] for r in rows]
        cnt = [sum(x > 0 for x in v)] + [sum(x >= t for x in v) for t in VOL[1:]]
        p(f"  {name:<6} " + "  ".join(f"{n:>5} ({n / len(rows):>4.0%})" if i == 0 else f"{n:>5}" for i, n in enumerate(cnt)) +
          f"   | {statistics.median(v):>8,.0f} {pct(v, .75):>8,.0f} {pct(v, .9):>8,.0f} | {statistics.median(hh):>5.0f} {pct(hh, .75):>4.0f} {pct(hh, .9):>4.0f}")
    vc = [r["vcum7 d"] for r in rows]
    p(f"  volumen acumulado en los 7 primeros días: mediana {statistics.median(vc):,.0f} USD, p75 {pct(vc, .75):,.0f}, "
      f"p90 {pct(vc, .9):,.0f}, máx {max(vc):,.0f}")
    p("")
    bal = {r[0]: r[1:] for r in db.execute("SELECT pool, quote_ui, base_ui FROM chain_pool_balances")}
    prices = dict(db.execute("SELECT mint, usd FROM quote_prices").fetchall())
    p("== 3. Liquidez real en cadena HOY (no a 24 h / 72 h / 7 d; ver Límites)")
    if bal and prices:
        fetched = db.execute("SELECT min(fetched_at) FROM chain_pool_balances").fetchone()[0]
        p(f"  saldos de los vaults leídos el {datetime.datetime.fromtimestamp(fetched, datetime.timezone.utc):%Y-%m-%d %H:%M} UTC "
          f"(~{(fetched - migs[0][3]) / 86400:.0f} días tras la migración); SOL = {prices.get(f0.WSOL, float('nan')):.2f} USD (GeckoTerminal)")
        p("  liquidez = 2 × saldo del vault quote × precio del quote (no se usa el precio del base, que en pools muertos")
        p("  está obsoleto en GeckoTerminal)")
        for r in rows:
            q = bal.get(r["pool"], (None, None))[0]
            r["liq"] = None if q is None else 2 * q * prices[f0.WSOL if r["quote"] != USDC else USDC]
        liq = [r["liq"] for r in rows if r["liq"] is not None]
        p(f"  pools con saldo leído {len(liq)}/{len(rows)}; mediana {statistics.median(liq):,.0f} USD, p75 {pct(liq, .75):,.0f}, "
          f"p90 {pct(liq, .9):,.0f}, máx {max(liq):,.0f}")
        p("  " + "  ".join(f">= {t:>6,}: {sum(x >= t for x in liq)}" for t in LIQ))
        p("")
        p("== 4. Candidatos por umbral (volumen 24 h a la edad × liquidez actual en cadena)")
        p("  edad    volumen 24 h   | liq >= 1k  liq >= 5k  liq >= 25k  | sin filtro de liquidez")
        for age, name in AGES:
            for t in VOL:
                sel = [r for r in rows if (r["v" + name] > 0 if t == 0 else r["v" + name] >= t)]
                p(f"  {name:<6}  {'> 0' if t == 0 else f'>= {t:,}':>12}   | " +
                  "  ".join(f"{sum(1 for r in sel if r['liq'] is not None and r['liq'] >= l):>9}" for l in LIQ) +
                  f"  | {len(sel):>5}")
    else:
        p("  sin datos (falta --chain)")
    p("")
    p("== Límites")
    p("  - Un solo día de graduaciones (2026-08-20) y una fuente de velas (GeckoTerminal, sin verificar en cadena).")
    p("  - La liquidez a 24 h / 72 h / 7 d NO está: GeckoTerminal no da liquidez histórica y el modelo de producto")
    p("    constante desde el precio no vale (k cae al 2–93 % en 6 de 8 pools comprobados en cadena). Solo hay liquidez de hoy.")
    p("  - reserve_in_usd de GeckoTerminal no se usa: en pools muertos sobrestima mucho (p. ej. 1 858 $ frente a 86 $ reales).")
    p("  - Volumen por velas horarias alineadas a la hora: el borde de cada ventana de 24 h tiene hasta 1 h de imprecisión.")
    p("  - GeckoTerminal omite las horas sin trades; volumen 0 = ninguna vela en la ventana.")
    p("  - Los pools que GeckoTerminal no conoce o que fallan con 500 quedan fuera de las tablas (sección 1).")
    p("  - Post hoc, sin pre-registro: los umbrales son descriptivos y elegir uno mirando esta tabla no es validación.")
    print("\n".join(L))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--download", action="store_true")
    ap.add_argument("--chain", action="store_true")
    ap.add_argument("--report", action="store_true")
    ap.add_argument("--day", default="2026-08-20")
    ap.add_argument("--madurez-db", default="madurez.db")
    ap.add_argument("--marxol-db", default="marxol.db")
    ap.add_argument("--max-gt", type=int, default=1600)
    ap.add_argument("--gt-interval", type=float, default=6.5, help="segundos entre llamadas (10/min como máximo)")
    ap.add_argument("--max-cu", type=int, default=2000)
    args = ap.parse_args()
    if args.download:
        download(args)
    elif args.chain:
        chain(args)
    elif args.report:
        report(args)
    else:
        sys.exit("elige --download, --chain o --report")


if __name__ == "__main__":
    main()
