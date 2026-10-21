#!/usr/bin/env python3
"""Fase 0b de tokens maduros: listar desde la cadena las migraciones a PumpSwap
de un día y descargar de GeckoTerminal una muestra de sus pools.

Modos (desde la raíz del repo):
    python3 -B scripts/madurez_fase0b.py --dry-run [--day 2026-08-20]   # sin red
    python3 -B scripts/madurez_fase0b.py --check                        # etapa A, tope 2 000 CU
    python3 -B scripts/madurez_fase0b.py --run [--day 2026-08-20]       # tope 300 000 CU y 150 llamadas GT

--check y --run usan la clave de Alchemy (entorno o .env, vía bench_stream.api_key;
nunca se imprime). Escriben solo en madurez.db y en rpc_usage de marxol.db.

Qué hace el --dry-run (todo local, bases abiertas en solo lectura):
1. Comprueba la derivación de PDAs con las dos ya verificadas (Global y
   mint-authority, CLAUDE.md secciones 4 y 11).
2. Valida el método con las migraciones de marxol.db: cada pool registrado
   tiene que ser el PDA de pump_amm con las seeds de `migrate_v2` del IDL,
   ["pool", 0u16 LE, pool_authority, base_mint, quote_mint], con
   pool_authority = PDA(["pool-authority", mint]) del programa pump y el quote
   SOL (Pubkey::default) sustituido por wSOL.
3. Estima el rango de slots del día por extrapolación lineal desde los
   (slot, timestamp) de las creaciones indexadas. Es solo una estimación: la
   ejecución real tiene que cerrar el día por blockTime, como index-tramo.
4. Imprime el plan: dirección a listar, topes (CU y llamadas a GeckoTerminal)
   y dónde escribiría (madurez.db).

Dirección a listar: `withdraw_authority`. Es la única cuenta de `migrate` y
`migrate_v2` que es fija para todo el protocolo, writable y exclusiva de esas
dos instrucciones dentro del IDL de pump (relación `global`, es decir,
Global.withdraw_authority). Las demás cuentas fijas (amm_global_config,
event authorities, programas) aparecen en todo el tráfico de PumpSwap o de
pump. Sin verificar todavía en cadena: que todas las tx de migración la
incluyen (las tx no se guardan en local) y qué fracción de su tráfico son
migraciones (también es la set_creator_authority; ver CLAUDE.md 5.2).

Etapa A (--check, 2026-10-20): las tx de migración conocidas incluyen la dirección
y el 97 % de su tráfico sin error son migraciones.
"""
import argparse, datetime, hashlib, sqlite3, sys

A = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
PUMP = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMP_AMM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
WSOL = "So11111111111111111111111111111111111111112"
DEFAULT = "11111111111111111111111111111111"
# Global.withdraw_authority según CLAUDE.md 5.2 (2026-09-28). Hay que releerla
# con `marxol global` antes de la ejecución real: puede cambiar (SetParamsEvent).
WITHDRAW_AUTHORITY = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg"

MAX_CU = 300_000
MAX_GT_CALLS = 150
CU_PER_CALL = 40  # getSignaturesForAddress y getTransaction en Alchemy
SIGS_PER_PAGE = 1000
GT_POOLS_PER_CALL = 30  # /networks/solana/pools/multi/{direcciones}: no verificado


def b58decode(s):
    n = 0
    for ch in s:
        n = n * 58 + A.index(ch)
    b = n.to_bytes((n.bit_length() + 7) // 8, "big")
    return b"\0" * (len(s) - len(s.lstrip("1"))) + b


def b58encode(b):
    n = int.from_bytes(b, "big")
    s = ""
    while n:
        n, r = divmod(n, 58)
        s = A[r] + s
    return "1" * (len(b) - len(b.lstrip(b"\0"))) + s


P = 2**255 - 19
D = -121665 * pow(121666, P - 2, P) % P


def on_curve(b):
    """True si los 32 bytes son un punto ed25519 válido (no puede ser PDA)."""
    y = int.from_bytes(b, "little") & ((1 << 255) - 1)
    if y >= P:
        return False
    x2 = (y * y - 1) * pow(D * y * y + 1, P - 2, P) % P
    return x2 == 0 or pow(x2, (P - 1) // 2, P) == 1


def pda(seeds, program):
    prog = b58decode(program)
    for bump in range(255, -1, -1):
        h = hashlib.sha256(b"".join(seeds) + bytes([bump]) + prog + b"ProgramDerivedAddress").digest()
        if not on_curve(h):
            return b58encode(h)
    raise ValueError("sin bump válido")


def pool_pda(mint, quote_mint):
    quote = WSOL if quote_mint == DEFAULT else quote_mint
    authority = pda([b"pool-authority", b58decode(mint)], PUMP)
    return pda([b"pool", b"\0\0", b58decode(authority), b58decode(mint), b58decode(quote)], PUMP_AMM)


class Alchemy:
    """JSON-RPC a Alchemy con tope de CU. La clave sale de bench_stream.api_key()
    (entorno o .env) y nunca se imprime: los errores se limpian con clean()."""

    COST = {"getTransaction": 40, "getSignaturesForAddress": 40, "getAccountInfo": 10}

    def __init__(self, max_cu):
        import bench_stream
        self.key, self.clean = bench_stream.api_key(), bench_stream.clean
        self.url = f"https://{bench_stream.HTTP_HOST}/v2/{self.key}"
        self.max_cu, self.calls, self.last = max_cu, {}, 0.0

    @property
    def cu(self):
        return sum(self.COST[m] * n for m, n in self.calls.items())

    def call(self, method, params):
        import json, time, urllib.error, urllib.request
        if self.cu + self.COST[method] > self.max_cu:
            raise RuntimeError(f"tope de {self.max_cu} CU alcanzado")
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
        for wait in (2, 4, 8, 16, 32, 64, None):
            time.sleep(max(0.0, self.last + 0.25 - time.time()))  # <= 4 req/s
            self.last = time.time()
            self.calls[method] = self.calls.get(method, 0) + 1  # cada intento cuenta
            try:
                req = urllib.request.Request(self.url, body, {"Content-Type": "application/json"})
                with urllib.request.urlopen(req, timeout=30) as r:
                    out = json.loads(r.read())
                if "error" in out:
                    raise RuntimeError(f"{method}: {out['error']}")
                return out["result"]
            except urllib.error.HTTPError as e:
                if e.code != 429 or wait is None:
                    raise RuntimeError(self.clean(f"{method}: HTTP {e.code}", self.key)) from None
                time.sleep(wait)
            except urllib.error.URLError as e:
                raise RuntimeError(self.clean(f"{method}: {e}", self.key)) from None

    def get_tx(self, sig):
        return self.call("getTransaction", [sig, {"encoding": "json", "maxSupportedTransactionVersion": 1,
                                                  "commitment": "confirmed"}])

    def record(self, db_path, command):
        """Una fila en rpc_usage de marxol.db (única escritura permitida en esa base)."""
        import time
        db = sqlite3.connect(db_path)
        with db:
            db.execute("INSERT INTO rpc_usage VALUES (?,?,?,?,?,?,?)",
                       (int(time.time()), command, "alchemy", self.calls.get("getTransaction", 0),
                        self.calls.get("getSignaturesForAddress", 0), self.cu, 0))
        db.close()


def tx_accounts(tx):
    msg = tx["transaction"]["message"]
    loaded = tx["meta"].get("loadedAddresses") or {}
    return msg["accountKeys"] + loaded.get("writable", []) + loaded.get("readonly", [])


def pump_ix_names(tx):
    """Nombres de las instrucciones (externas e internas) dirigidas al programa pump."""
    import json
    disc = {bytes(i["discriminator"]): i["name"] for i in json.load(open("idl/pump.json"))["instructions"]}
    keys = tx_accounts(tx)
    ixs = list(tx["transaction"]["message"]["instructions"])
    for inner in tx["meta"].get("innerInstructions") or []:
        ixs += inner["instructions"]
    names = []
    for ix in ixs:
        if keys[ix["programIdIndex"]] == PUMP:
            data = b58decode(ix["data"])
            names.append(disc.get(data[:8], "evento/otro" if data[:8] == bytes.fromhex("e445a52e51cb9a1d") else "?"))
    return names


def check(args):
    """Etapa A: comprobaciones en cadena con tope de CU."""
    import collections, json, os, random, subprocess, time
    rpc = Alchemy(args.max_cu)
    try:
        print("== A1. Global.withdraw_authority (marxol global)")
        env = dict(os.environ, MARXOL_RPC_URL=rpc.url)
        out = subprocess.run([args.marxol, "--db", os.devnull, "--json", "global"], env=env,
                             capture_output=True, text=True)
        rpc.calls["getAccountInfo"] = 1
        if out.returncode != 0:
            raise RuntimeError(rpc.clean(out.stderr.strip(), rpc.key))
        g = json.loads(out.stdout)
        fields = g.get("fields", g)
        wa = fields.get("withdraw_authority")
        sca = fields.get("set_creator_authority")
        print(f"  withdraw_authority = {wa}  ({'coincide' if wa == WITHDRAW_AUTHORITY else 'NO coincide'} con {WITHDRAW_AUTHORITY[:8]}…)")
        print(f"  set_creator_authority = {sca}")

        print("\n== A2. ¿Las tx de migración conocidas incluyen esa dirección?")
        db = ro(args.marxol_db)
        known = db.execute("SELECT mint, quote_mint, signature FROM migrations ORDER BY slot").fetchall()
        picks = [known[0], known[4], known[-1]]  # SOL antiguo, USDC, SOL reciente
        for mint, quote, sig in picks:
            tx = rpc.get_tx(sig)
            keys = tx_accounts(tx)
            signer = tx["transaction"]["message"]["accountKeys"][0]
            print(f"  {sig[:12]}… mint {mint[:8]}… quote {quote[:6]}…: withdraw_authority "
                  f"{'SÍ' if WITHDRAW_AUTHORITY in keys else 'NO'} está; ix pump {pump_ix_names(tx)}; "
                  f"fee payer {signer[:8]}…")

        print("\n== A3. Una página de getSignaturesForAddress")
        page = rpc.call("getSignaturesForAddress", [WITHDRAW_AUTHORITY, {"limit": 1000, "commitment": "confirmed"}])
        ok = [s for s in page if s["err"] is None]
        times = [s["blockTime"] for s in page if s.get("blockTime")]
        span = max(times) - min(times)
        per_day = len(page) * 86400 / span if span else float("inf")
        print(f"  {len(page)} firmas, {len(ok)} sin error, de {datetime.datetime.utcfromtimestamp(min(times)):%Y-%m-%d %H:%M} "
              f"a {datetime.datetime.utcfromtimestamp(max(times)):%Y-%m-%d %H:%M} UTC ({span / 3600:.2f} h)")
        known_sigs = {k[2] for k in known}
        print(f"  coincidencias con la tabla migrations: {sum(s['signature'] in known_sigs for s in page)} "
              "(se esperan 0: la tabla es de 2026-09-28 a 2026-10-01)")
        sample = random.Random(20260930).sample(ok, min(args.sample, len(ok)))
        kinds = collections.Counter()
        for s in sample:
            names = pump_ix_names(rpc.get_tx(s["signature"]))
            top = [n for n in names if n != "evento/otro"]
            kinds["migrate/migrate_v2" if {"migrate", "migrate_v2"} & set(top) else "+".join(sorted(set(top))) or "sin ix pump"] += 1
        mig = kinds["migrate/migrate_v2"]
        print(f"  muestra de {len(sample)} tx sin error (semilla 20260930): " +
              ", ".join(f"{k} {n}" for k, n in kinds.most_common()))
        frac_ok = len(ok) / len(page)
        frac_mig = mig / len(sample) if sample else 0
        days_back = (time.time() - datetime.datetime(2026, 8, 20, tzinfo=datetime.timezone.utc).timestamp()) / 86400
        pages = per_day * days_back / 1000
        print(f"  firmas/día ≈ {per_day:,.0f} (todas), migraciones/día ≈ {per_day * frac_ok * frac_mig:,.0f} "
              f"({frac_ok:.0%} sin error × {frac_mig:.0%} migraciones en la muestra)")
        print(f"  hasta el 2026-08-20 ({days_back:.1f} días): ≈ {pages:,.0f} páginas ≈ {pages * 40:,.0f} CU de listado")
        print(f"  + getTransaction de las migraciones del día ≈ {per_day * frac_ok * frac_mig * 40:,.0f} CU "
              "(si el ritmo de entonces fuera el de esta página)")
    finally:
        rpc.record(args.marxol_db, "madurez-fase0b-check")
        print(f"\n  consumo: {rpc.calls} = {rpc.cu} CU (tope {rpc.max_cu}); registrado en rpc_usage")


MIGRATION_EVENT = bytes([189, 233, 93, 185, 92, 148, 234, 148])
EVENT_CPI = bytes.fromhex("e445a52e51cb9a1d")


def decode_migrations(tx):
    """CompletePumpAmmMigrationEvent de una tx (inner instructions de emit_cpi al
    programa pump). Campos en el orden del IDL; sol_amount va en unidades del quote."""
    import struct
    keys = tx_accounts(tx)
    out = []
    for inner in tx["meta"].get("innerInstructions") or []:
        for ix in inner["instructions"]:
            if keys[ix["programIdIndex"]] != PUMP:
                continue
            d = b58decode(ix["data"])
            if d[:8] != EVENT_CPI or d[8:16] != MIGRATION_EVENT:
                continue
            b = d[16:]
            pk = lambda o: b58encode(b[o:o + 32])
            mint_amount, sol_amount, fee = struct.unpack_from("<QQQ", b, 64)
            (ts,) = struct.unpack_from("<q", b, 120)
            out.append(dict(user=pk(0), mint=pk(32), mint_amount=mint_amount, quote_amount=sol_amount,
                            pool_migration_fee=fee, bonding_curve=pk(88), timestamp=ts, pool=pk(128),
                            quote_mint=pk(160), extra_bytes=len(b) - 192))
    return out


SCHEMA = """
CREATE TABLE IF NOT EXISTS fase0b_runs (run_at INTEGER, day TEXT, start INTEGER, end INTEGER, pages INTEGER,
    sigs_day INTEGER, sigs_day_ok INTEGER, listing_complete INTEGER, cu INTEGER, gt_calls INTEGER);
CREATE TABLE IF NOT EXISTS fase0b_sigs (signature TEXT PRIMARY KEY, day TEXT, slot INTEGER, block_time INTEGER,
    err INTEGER, status TEXT);  -- status: NULL (sin pedir), migration, other
CREATE TABLE IF NOT EXISTS chain_migrations (signature TEXT, mint TEXT PRIMARY KEY, pool TEXT, quote_mint TEXT,
    mint_amount INTEGER, quote_amount INTEGER, pool_migration_fee INTEGER, bonding_curve TEXT, user TEXT,
    timestamp INTEGER, slot INTEGER, day TEXT, pool_pda_ok INTEGER);
CREATE TABLE IF NOT EXISTS fase0b_cursor (day TEXT PRIMARY KEY, before_sig TEXT, oldest_bt INTEGER, pages INTEGER,
    complete INTEGER);  -- reanudación del listado
CREATE TABLE IF NOT EXISTS fase0b_sample (day TEXT, seed INTEGER, rank INTEGER, pool TEXT, mint TEXT,
    PRIMARY KEY (day, seed, rank));
CREATE TABLE IF NOT EXISTS gt_pool_info (pool TEXT PRIMARY KEY, fetched_at REAL, json TEXT);
CREATE TABLE IF NOT EXISTS gt_ohlcv (pool TEXT, timeframe TEXT, currency TEXT, ts INTEGER, open REAL, high REAL,
    low REAL, close REAL, volume REAL, PRIMARY KEY (pool, timeframe, currency, ts));
CREATE TABLE IF NOT EXISTS gt_ohlcv_done (pool TEXT, timeframe TEXT, currency TEXT, complete INTEGER,
    PRIMARY KEY (pool, timeframe, currency));
"""


class Gecko:
    """GeckoTerminal público con tope de llamadas (cada intento cuenta) y registro en api_calls."""

    BASE = "https://api.geckoterminal.com/api/v2/networks/solana"

    def __init__(self, db, max_calls, interval):
        self.db, self.max_calls, self.interval, self.calls, self.last = db, max_calls, interval, 0, 0.0

    def get(self, path):
        import json, time, urllib.error, urllib.request
        for wait in (30, 60, None):
            if self.calls >= self.max_calls:
                raise RuntimeError(f"tope de {self.max_calls} llamadas a GeckoTerminal alcanzado")
            time.sleep(max(0.0, self.last + self.interval - time.time()))
            self.calls += 1
            self.last = time.time()
            url = self.BASE + path
            try:
                req = urllib.request.Request(url, headers={"Accept": "application/json;version=20230203"})
                with urllib.request.urlopen(req, timeout=30) as r:
                    status, body = r.status, r.read()
            except urllib.error.HTTPError as e:
                status, body = e.code, None
            with self.db:
                self.db.execute("INSERT INTO api_calls VALUES (?,?,?)", (time.time(), url, status))
            if status == 200:
                return json.loads(body)
            if status != 429 or wait is None:
                raise RuntimeError(f"GeckoTerminal HTTP {status}: {url}")
            time.sleep(wait)


def run(args):
    """Modo real: lista el día por blockTime, decodifica las migraciones y descarga la muestra de GeckoTerminal."""
    import random, time
    day = datetime.datetime.strptime(args.day, "%Y-%m-%d").replace(tzinfo=datetime.timezone.utc)
    start, end = int(day.timestamp()), int(day.timestamp()) + 86400
    mdb = sqlite3.connect(args.madurez_db)
    mdb.executescript(SCHEMA)
    rpc = Alchemy(args.max_cu)
    gt = Gecko(mdb, args.max_gt, args.gt_interval)
    pages = 0
    complete = False
    try:
        # 1. Comprobación del decodificador con las migraciones conocidas de marxol.db.
        print("== 1. Decodificador contra las migraciones conocidas (marxol.db)")
        known = ro(args.marxol_db).execute("SELECT mint, pool, quote_mint, mint_amount, quote_amount, "
                                           "pool_migration_fee, timestamp, signature FROM migrations").fetchall()
        bad = 0
        if mdb.execute("SELECT 1 FROM fase0b_cursor WHERE day = ?", (args.day,)).fetchone():
            args.validate = 0  # ya validado en una ejecución anterior (16/16)
        for mint, pool, quote, ma, qa, fee, ts, sig in known[:args.validate]:
            ev = decode_migrations(rpc.get_tx(sig))
            ok = len(ev) == 1 and (ev[0]["mint"], ev[0]["pool"], ev[0]["quote_mint"], ev[0]["mint_amount"],
                                   ev[0]["quote_amount"], ev[0]["pool_migration_fee"], ev[0]["timestamp"]) == \
                (mint, pool, quote, ma, qa, fee, ts)
            bad += not ok
            if not ok:
                print(f"  NO CUADRA {sig[:12]}…: {ev}")
        print(f"  {min(args.validate, len(known)) - bad}/{min(args.validate, len(known))} cuadran campo a campo")
        if bad:
            raise RuntimeError("el decodificador no reproduce las migraciones conocidas")

        # 2. Listado por blockTime: hacia atrás desde ahora hasta pasar el inicio del día.
        print(f"\n== 2. Listado de {WITHDRAW_AUTHORITY[:8]}… hasta blockTime < {start}")
        cur = mdb.execute("SELECT before_sig, pages, complete FROM fase0b_cursor WHERE day = ?", (args.day,)).fetchone()
        before, prev_pages, complete = (cur[0], cur[1], bool(cur[2])) if cur else (None, 0, False)
        if cur:
            print(f"  reanudando tras {prev_pages} páginas de ejecuciones anteriores")
        while not complete:
            opts = {"limit": 1000, "commitment": "confirmed"}
            if before:
                opts["before"] = before
            page = rpc.call("getSignaturesForAddress", [WITHDRAW_AUTHORITY, opts])
            pages += 1
            if not page:
                raise RuntimeError("la lista se acabó antes de llegar al día (historial incompleto)")
            for s in page:
                bt = s.get("blockTime")
                if bt is None:
                    raise RuntimeError(f"firma sin blockTime: {s['signature']}")
                if start <= bt < end:
                    mdb.execute("INSERT OR IGNORE INTO fase0b_sigs VALUES (?,?,?,?,?,NULL)",
                                (s["signature"], args.day, s["slot"], bt, int(s["err"] is not None)))
            mdb.commit()
            oldest = page[-1].get("blockTime")
            before = page[-1]["signature"]
            complete = oldest < start
            mdb.execute("INSERT OR REPLACE INTO fase0b_cursor VALUES (?,?,?,?,?)",
                        (args.day, before, oldest, prev_pages + pages, int(complete)))
            mdb.commit()
            if pages % 20 == 0:
                print(f"  {pages} páginas, en {datetime.datetime.fromtimestamp(oldest, datetime.timezone.utc):%Y-%m-%d %H:%M}")
        n_day, n_ok = mdb.execute("SELECT count(*), sum(err = 0) FROM fase0b_sigs WHERE day = ?", (args.day,)).fetchone()
        print(f"  {pages} páginas en esta ejecución ({prev_pages + pages} en total); firmas del día: {n_day} ({n_ok} sin error)")

        # 3. getTransaction de las firmas sin error del día.
        print("\n== 3. Migraciones del día")
        todo = mdb.execute("SELECT signature, slot FROM fase0b_sigs WHERE day = ? AND err = 0 AND status IS NULL "
                           "ORDER BY slot", (args.day,)).fetchall()
        for i, (sig, slot) in enumerate(todo):
            evs = decode_migrations(rpc.get_tx(sig))
            for ev in evs:
                mdb.execute("INSERT OR REPLACE INTO chain_migrations VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                            (sig, ev["mint"], ev["pool"], ev["quote_mint"], ev["mint_amount"], ev["quote_amount"],
                             ev["pool_migration_fee"], ev["bonding_curve"], ev["user"], ev["timestamp"], slot,
                             args.day, int(pool_pda(ev["mint"], ev["quote_mint"]) == ev["pool"])))
            mdb.execute("UPDATE fase0b_sigs SET status = ? WHERE signature = ?", ("migration" if evs else "other", sig))
            if i % 100 == 99:
                mdb.commit()
                print(f"  {i + 1}/{len(todo)} tx, {rpc.cu} CU")
            time.sleep(0.03)
        mdb.commit()

        # 4. Muestra de 40 pools y descarga de GeckoTerminal (datos crudos; las métricas van aparte).
        print("\n== 4. Muestra y GeckoTerminal")
        rows = mdb.execute("SELECT pool, mint FROM chain_migrations WHERE day = ? ORDER BY slot, mint", (args.day,)).fetchall()
        sample = random.Random(args.seed).sample(rows, min(args.n_sample, len(rows)))
        mdb.executemany("INSERT OR REPLACE INTO fase0b_sample VALUES (?,?,?,?,?)",
                        [(args.day, args.seed, r, p, m) for r, (p, m) in enumerate(sample)])
        mdb.commit()
        pools = [p for p, _ in sample]
        missing = [p for p in pools if not mdb.execute("SELECT 1 FROM gt_pool_info WHERE pool = ?", (p,)).fetchone()]
        import json
        for k in range(0, len(missing), args.gt_chunk):
            chunk = missing[k:k + args.gt_chunk]
            try:
                res = gt.get(f"/pools/multi/{','.join(chunk)}?include=base_token")
                got = {d["attributes"]["address"]: d for d in res.get("data", [])}
            except RuntimeError as e:  # p. ej. HTTP 500 con 30 direcciones (2026-10-21): pool a pool
                if "HTTP 500" not in str(e):
                    raise
                got = {}
                for one in chunk:
                    try:
                        got[one] = gt.get(f"/pools/{one}?include=base_token")["data"]
                    except RuntimeError as e1:
                        if "HTTP 404" not in str(e1):
                            raise
            for p in chunk:
                mdb.execute("INSERT OR REPLACE INTO gt_pool_info VALUES (?,?,?)",
                            (p, time.time(), json.dumps(got.get(p))))  # null = GeckoTerminal no lo conoce
            mdb.commit()
        for p in pools:
            if mdb.execute("SELECT complete FROM gt_ohlcv_done WHERE pool = ? AND timeframe = 'hour' AND currency = 'token'",
                           (p,)).fetchone():
                continue
            if mdb.execute("SELECT json FROM gt_pool_info WHERE pool = ?", (p,)).fetchone()[0] == "null":
                continue
            before_ts = None
            while True:
                q = "?aggregate=1&limit=1000&currency=token" + (f"&before_timestamp={before_ts}" if before_ts else "")
                try:
                    res = gt.get(f"/pools/{p}/ohlcv/hour{q}")
                except RuntimeError as e:
                    if "HTTP 500" not in str(e):
                        raise
                    time.sleep(60)
                    try:
                        res = gt.get(f"/pools/{p}/ohlcv/hour{q}")
                    except RuntimeError as e2:
                        if "HTTP 500" not in str(e2):
                            raise
                        print(f"  {p}: HTTP 500 dos veces, n/e")
                        mdb.execute("INSERT OR REPLACE INTO gt_ohlcv_done VALUES (?,?,?,-1)", (p, "hour", "token"))
                        mdb.commit()
                        break
                candles = res["data"]["attributes"]["ohlcv_list"]
                mdb.executemany("INSERT OR IGNORE INTO gt_ohlcv VALUES (?,?,?,?,?,?,?,?,?)",
                                [(p, "hour", "token", *c) for c in candles])
                if len(candles) < 1000 or min(c[0] for c in candles) < start:
                    mdb.execute("INSERT OR REPLACE INTO gt_ohlcv_done VALUES (?,?,?,1)", (p, "hour", "token"))
                    mdb.commit()
                    break
                before_ts = min(c[0] for c in candles)
                mdb.commit()
        print(f"  muestra {len(pools)} pools; llamadas a GeckoTerminal en esta ejecución: {gt.calls}")
    finally:
        mdb.execute("INSERT INTO fase0b_runs VALUES (?,?,?,?,?,?,?,?,?,?)",
                    (int(time.time()), args.day, start, end, pages,
                     *mdb.execute("SELECT count(*), coalesce(sum(err = 0), 0) FROM fase0b_sigs WHERE day = ?",
                                  (args.day,)).fetchone(), int(complete), rpc.cu, gt.calls))
        mdb.commit()
        mdb.close()
        rpc.record(args.marxol_db, "madurez-fase0b")
        print(f"\n  consumo Alchemy: {rpc.calls} = {rpc.cu} CU (tope {rpc.max_cu}); GeckoTerminal {gt.calls} "
              f"(tope {gt.max_calls}). Registrado en rpc_usage (marxol.db) y fase0b_runs (madurez.db)")


DEAD_DAYS = 7     # muerto: sin velas con volumen > 0 en los últimos 7 días antes de la descarga
DROP = 0.80       # segunda vida: cierre <= (1 − DROP) × máximo de cierre anterior…
REBOUND = 2.0     # … y un cierre posterior >= REBOUND × el mínimo de cierre tras la caída


def second_life(closes):
    """closes: [(ts, close)] ordenados. Devuelve (cayó ≥ DROP, ts de la segunda vida o None).
    Un punto "caído" es un cierre <= (1 − DROP) × máximo de los cierres hasta él (incluido).
    Segunda vida = algún cierre posterior >= REBOUND × el menor cierre caído anterior a él."""
    runmax, low_dropped, dropped = 0.0, None, False
    for ts, c in closes:
        if low_dropped is not None and c >= REBOUND * low_dropped:
            return True, ts
        runmax = max(runmax, c)
        if c <= (1 - DROP) * runmax:
            dropped = True
            low_dropped = c if low_dropped is None else min(low_dropped, c)
    return dropped, None


def report(args):
    """Métricas de la muestra (solo lectura de madurez.db) → texto para resultados/madurez_fase0b.txt."""
    import collections, json, statistics
    db = ro(args.madurez_db)
    day = args.day
    fmt = lambda t: datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%Y-%m-%d %H:%M")
    L = []
    p = L.append
    p("Fase 0b de tokens maduros — POST HOC, SIN PRE-REGISTRO, SOLO DESCRIPTIVO")
    p(f"Generado con: python3 -B scripts/madurez_fase0b.py --report --day {day}  (solo lee madurez.db)")
    p("")
    start = int(datetime.datetime.strptime(day, "%Y-%m-%d").replace(tzinfo=datetime.timezone.utc).timestamp())
    cur = db.execute("SELECT pages, complete, oldest_bt FROM fase0b_cursor WHERE day = ?", (day,)).fetchone()
    n_day, n_ok, n_mig_tx, n_other, n_pend = db.execute(
        "SELECT count(*), sum(err = 0), sum(status = 'migration'), sum(status = 'other'), "
        "sum(err = 0 AND status IS NULL) FROM fase0b_sigs WHERE day = ?", (day,)).fetchone()
    migs = db.execute("SELECT mint, pool, quote_mint, quote_amount, timestamp, slot, pool_pda_ok, signature "
                      "FROM chain_migrations WHERE day = ? ORDER BY slot, mint", (day,)).fetchall()
    runs = db.execute("SELECT run_at, pages, cu, gt_calls FROM fase0b_runs WHERE day = ? ORDER BY run_at", (day,)).fetchall()
    p(f"== 1. Listado del día {day} UTC (blockTime en [{start}, {start + 86400}))")
    p(f"  dirección listada: {WITHDRAW_AUTHORITY} (Global.withdraw_authority, releída 2026-10-20)")
    p(f"  listado {'COMPLETO' if cur and cur[1] else 'INCOMPLETO'}: {cur[0] if cur else 0} páginas de 1000 en total "
      f"(última página hasta {fmt(cur[2]) if cur else '-'} UTC)")
    p(f"  firmas del día: {n_day} ({n_ok} sin error); tx sin error con migración {n_mig_tx}, sin migración {n_other}, "
      f"sin pedir {n_pend}")
    p(f"  migraciones (CompletePumpAmmMigrationEvent): {len(migs)}; pool = PDA derivado en {sum(m[6] for m in migs)}/{len(migs)}")
    if migs:
        ts = [m[4] for m in migs]
        p(f"  primera {fmt(min(ts))}, última {fmt(max(ts))} UTC; fuera del día por timestamp del evento: "
          f"{sum(not start <= t < start + 86400 for t in ts)}")
        q = collections.Counter(m[2] for m in migs)
        p("  quotes: " + ", ".join(f"{k[:8]}… {v}" for k, v in q.most_common()))
        hours = collections.Counter((t - start) // 3600 for t in ts)
        p("  por hora UTC: " + " ".join(f"{h:02d}:{hours.get(h, 0)}" for h in range(24)))
    p("  validación con las 16 de marxol.db: no coinciden en fechas (2026-09-28 a 2026-10-01). El decodificador se")
    p("  comprobó re-decodificando sus 16 tx: 16/16 campo a campo (mint, pool, quote, importes, comisión, timestamp).")
    p("")
    p("== 2. Consumo")
    for r in runs:
        p(f"  ejecución {fmt(r[0])}: {r[1]} páginas, {r[2]} CU, {r[3]} llamadas a GeckoTerminal")
    total_cu = sum(r[2] for r in runs)
    u = ro(args.marxol_db).execute("SELECT sum(get_transaction), sum(get_signatures), sum(units) FROM rpc_usage "
                                   "WHERE command = 'madurez-fase0b'").fetchone()
    p(f"  total --run: {total_cu} CU (rpc_usage: {u[0]} getTransaction + {u[1]} getSignaturesForAddress = {u[2]} CU,")
    p("  contando cada reintento por 429); más 1 370 CU de la etapa A (--check)")
    sig_pages = cur[0] if cur else 0
    p(f"  listado: {sig_pages} páginas útiles, {u[1]} llamadas reales ({u[1] * 40} CU; incluye 60 páginas de una ejecución")
    p("  cortada por 429 que no guardaba cursor y los reintentos). La etapa A estimaba ~112 páginas: el tráfico de agosto y")
    p("  septiembre tiene muchas más tx fallidas que la página de octubre medida (99 % frente a 5 %).")
    p(f"  coste por día listado: getTransaction de las {n_ok} firmas sin error = {n_ok * 40} CU, más el listado, que depende de")
    p(f"  la distancia a hoy y del tráfico intermedio ({sig_pages} páginas para llegar a {day} desde 2026-10-20)")
    p("")
    sample = db.execute("SELECT rank, pool, mint FROM fase0b_sample WHERE day = ? AND seed = ? ORDER BY rank",
                        (day, args.seed)).fetchall()
    p(f"== 3. Muestra: {len(sample)} pools al azar (random.Random({args.seed}).sample sobre las migraciones ordenadas por (slot, mint))")
    p(f"  muerto = GeckoTerminal no conoce el pool o ninguna vela horaria con volumen > 0 en los {DEAD_DAYS} días")
    p("          anteriores a la descarga de su ficha")
    p(f"  segunda vida = con cierres horarios (currency=token), un cierre <= {1 - DROP:.0%} del máximo de cierre previo")
    p(f"          y después un cierre >= {REBOUND:g}× el menor de esos cierres caídos")
    rows = []
    for rank, pool, mint in sample:
        info = db.execute("SELECT fetched_at, json FROM gt_pool_info WHERE pool = ?", (pool,)).fetchone()
        done = db.execute("SELECT complete FROM gt_ohlcv_done WHERE pool = ? AND timeframe = 'hour' AND currency = 'token'",
                          (pool,)).fetchone()
        candles = db.execute("SELECT ts, close, volume FROM gt_ohlcv WHERE pool = ? AND timeframe = 'hour' AND "
                             "currency = 'token' ORDER BY ts", (pool,)).fetchall()
        known = info is not None and info[1] != "null"
        if info is None or (known and (not done or done[0] != 1)):
            rows.append((rank, pool, "n/e", None, None, None, len(candles), known))
            continue
        last_active = max((t for t, _, v in candles if v and v > 0), default=None)
        dead = (not known) or last_active is None or last_active < info[0] - DEAD_DAYS * 86400
        dropped, sl = second_life([(t, c) for t, c, _ in candles if c and c > 0])
        attrs = json.loads(info[1])["attributes"] if known else {}
        rows.append((rank, pool, "muerto" if dead else "vivo", dropped, sl, attrs.get("reserve_in_usd"), len(candles), known))
    ev = [r for r in rows if r[2] != "n/e"]
    dead = [r for r in ev if r[2] == "muerto"]
    dropped = [r for r in ev if r[3]]
    sl = [r for r in ev if r[4]]
    p(f"  evaluables {len(ev)}/{len(rows)}; GeckoTerminal no conoce {sum(not r[7] for r in ev)}")
    def wilson(k, n, z=1.96):
        c = (k + z * z / 2) / (n + z * z)
        h = z * (k * (n - k) / n + z * z / 4) ** 0.5 / (n + z * z)
        return f"IC95 Wilson {c - h:.0%}–{c + h:.0%}"
    p(f"  MUERTOS: {len(dead)}/{len(ev)} ({len(dead) / len(ev):.0%}, {wilson(len(dead), len(ev))})" if ev else "  MUERTOS: n/e")
    p(f"  con caída >= {DROP:.0%} desde el máximo de cierre: {len(dropped)}/{len(ev)}")
    if ev:
        p(f"  SEGUNDA VIDA: {len(sl)}/{len(ev)} de la muestra ({len(sl) / len(ev):.0%}, {wilson(len(sl), len(ev))}); "
          f"{len(sl)}/{len(dropped)} de los que caen")
        p("  Lectura: casi ningún pool cae >= 80 % en cierres horarios porque la mayoría deja de operar en pocas horas")
        p("  (mediana de velas por pool baja); muerto = abandono, no desplome. La tasa de segunda vida condicionada a la")
        p("  caída (n = 2) no es interpretable.")
        if sl:
            p(f"    de ellos muertos hoy: {sum(r[2] == 'muerto' for r in sl)}; horas hasta la segunda vida desde la "
              f"migración (mediana): {statistics.median((r[4] - migts) / 3600 for r in sl for migts in [next(m[4] for m in migs if m[1] == r[1])]):.0f}")
    p("")
    p("  rank pool                                          estado  cae80  2ª vida            reserve_usd  velas")
    for r in rows:
        p(f"  {r[0]:>4} {r[1]:<44} {r[2]:<7} {('sí' if r[3] else 'no') if r[3] is not None else '-':<5}  "
          f"{fmt(r[4]) if r[4] else '-':<17}  {r[5] or '-':>11}  {r[6]:>5}")
    p("")
    p("== Límites")
    p("  - Un solo día (2026-08-20) y 40 pools: describe, no estima con precisión (IC binomial amplio con n = 40).")
    p("  - Post hoc y sin pre-registro: las definiciones de muerto y segunda vida se fijaron el 2026-10-20, antes de calcular,")
    p("    pero sin pre-registro en CLAUDE.md. No valen como validación.")
    p("  - GeckoTerminal omite las horas sin trades; los cierres son de horas con actividad. Una sola vela con un trade")
    p("    puede bastar para marcar segunda vida (no hay mínimo de volumen ni de duración).")
    p("  - currency=token: precio en el quote del pool, no en USD; la segunda vida es relativa y no depende del precio de SOL.")
    p("  - Precio de GeckoTerminal, no reconstruido desde la cadena: no está verificado contra las reservas del pool.")
    p("  - El listado depende de que todas las migraciones toquen withdraw_authority: comprobado en 3 tx conocidas y en")
    p("    una muestra de 30 tx recientes (29 migraciones), no en todo el historial. Una migración que no la incluyera no se vería.")
    p("  - Muerto se mide a la fecha de descarga (2026-10-20/21), unos 61 días después de la migración.")
    print("\n".join(L))


def ro(path):
    c = sqlite3.connect(f"file:{path}?mode=ro&immutable=1", uri=True)
    c.execute("PRAGMA query_only = 1")
    return c


def validate(db):
    rows = db.execute("SELECT m.mint, m.pool, m.quote_mint, m.slot, m.signature, c.slot, c.signature "
                      "FROM migrations m LEFT JOIN completions c USING (mint) ORDER BY m.slot").fetchall()
    ok = same_tx = 0
    gaps = []
    for mint, pool, quote, slot, sig, cslot, csig in rows:
        got = pool_pda(mint, quote)
        ok += got == pool
        if got != pool:
            print(f"  NO CUADRA {mint}: registrado {pool}, derivado {got}")
        same_tx += csig == sig
        if cslot is not None:
            gaps.append(slot - cslot)
    print(f"  pools derivados con las seeds de migrate_v2: {ok}/{len(rows)} cuadran")
    print(f"  con CompleteEvent indexado: {len(gaps)}/{len(rows)}; misma tx que la migración: {same_tx}")
    if gaps:
        print(f"  slots entre CompleteEvent y migración: mín {min(gaps)}, máx {max(gaps)}, 0 en {gaps.count(0)}")
    quotes = {}
    for r in rows:
        quotes[r[2]] = quotes.get(r[2], 0) + 1
    print("  quotes:", ", ".join(f"{q[:8]}… {n}" for q, n in sorted(quotes.items(), key=lambda x: -x[1])))
    return ok == len(rows) and len(rows) > 0


def slot_estimate(db, start, end):
    (s0, t0), (s1, t1) = (db.execute(f"SELECT slot, timestamp FROM creations ORDER BY slot {o} LIMIT 1").fetchone()
                          for o in ("ASC", "DESC"))
    rate = (s1 - s0) / (t1 - t0)
    est = lambda t: round(s0 + (t - t0) * rate)
    return rate, (s0, t0), (s1, t1), est(start), est(end)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dry-run", action="store_true", help="obligatorio: no hay modo real todavía")
    ap.add_argument("--day", default="2026-08-20", help="día UTC (AAAA-MM-DD)")
    ap.add_argument("--marxol-db", default="marxol.db")
    ap.add_argument("--madurez-db", default="madurez.db")
    ap.add_argument("--check", action="store_true", help="etapa A: comprobaciones en cadena (usa red y CU)")
    ap.add_argument("--max-cu", type=int, default=None, help="2000 en --check, 300000 en --run")
    ap.add_argument("--sample", type=int, default=30, help="tx de la página que se decodifican en --check")
    ap.add_argument("--marxol", default="target/release/marxol")
    ap.add_argument("--run", action="store_true", help="modo real (usa red, CU y escribe en madurez.db)")
    ap.add_argument("--max-gt", type=int, default=MAX_GT_CALLS)
    ap.add_argument("--gt-interval", type=float, default=7.0, help="segundos entre llamadas a GeckoTerminal")
    ap.add_argument("--validate", type=int, default=16, help="migraciones conocidas que se re-decodifican")
    ap.add_argument("--gt-chunk", type=int, default=10, help="pools por llamada multi (máx. documentado 30)")
    ap.add_argument("--seed", type=int, default=20260930)
    ap.add_argument("--n-sample", type=int, default=40)
    ap.add_argument("--report", action="store_true", help="métricas de la muestra (solo lee madurez.db)")
    args = ap.parse_args()
    if args.report:
        return report(args)
    if args.check:
        args.max_cu = args.max_cu or 2000
        return check(args)
    if args.run:
        args.max_cu = args.max_cu or MAX_CU
        return run(args)
    if not args.dry_run:
        sys.exit("elige --dry-run, --check o --run")

    day = datetime.datetime.strptime(args.day, "%Y-%m-%d").replace(tzinfo=datetime.timezone.utc)
    start, end = int(day.timestamp()), int(day.timestamp()) + 86400

    print("== 1. Derivación de PDAs")
    g, ma = pda([b"global"], PUMP), pda([b"mint-authority"], PUMP)
    assert g.startswith("4wTV1Ymi") and ma.startswith("TSLvdd1p"), (g, ma)
    print(f"  Global {g} y mint-authority {ma}: cuadran con CLAUDE.md")

    print(f"\n== 2. Validación local con {args.marxol_db} (solo lectura)")
    db = ro(args.marxol_db)
    valid = validate(db)

    print(f"\n== 3. Rango de slots de {args.day} UTC (estimación, no definitiva)")
    rate, a0, a1, s_start, s_end = slot_estimate(db, start, end)
    print(f"  blockTime [{start}, {end})")
    print(f"  anclas locales: slot {a0[0]} @ {a0[1]} y slot {a1[0]} @ {a1[1]} → {rate:.3f} slots/s")
    print(f"  slots estimados [{s_start}, {s_end}) (~{s_end - s_start} slots). Extrapolación de "
          f"{(a0[1] - start) / 86400:.0f} días hacia atrás: el error puede ser de decenas de miles de slots.")
    print("  La ejecución real tiene que cerrar el día por blockTime de las firmas, no por estos slots.")

    print("\n== 4. Plan (no se ejecuta)")
    print(f"  dirección: {WITHDRAW_AUTHORITY} (Global.withdraw_authority; releerla con `marxol global`)")
    print("  a) getSignaturesForAddress paginando hacia atrás desde ahora hasta blockTime < "
          f"{start}; firmas con err descartadas sin pedir la tx.")
    print(f"     Coste: {CU_PER_CALL} CU por página de {SIGS_PER_PAGE}. Número de páginas DESCONOCIDO: depende del "
          "tráfico de la dirección desde el día pedido hasta hoy.")
    print("  b) getTransaction (versión 1) de las firmas con blockTime en el día → CompletePumpAmmMigrationEvent "
          f"(mint, pool, quote_mint, importes). {CU_PER_CALL} CU cada una.")
    print(f"  c) GeckoTerminal: estado actual de los pools, hasta {GT_POOLS_PER_CALL} por llamada (no verificado) "
          f"→ como mucho {MAX_GT_CALLS * GT_POOLS_PER_CALL} pools.")
    print(f"  topes: {MAX_CU:,} CU de Alchemy ({MAX_CU // CU_PER_CALL:,} llamadas) y {MAX_GT_CALLS} llamadas a "
          "GeckoTerminal. Al alcanzar cualquiera, parar y dejar el día marcado como incompleto.")
    print(f"  escribiría en: {args.madurez_db} (nunca en {args.marxol_db})")
    try:
        mdb = ro(args.madurez_db)
        for (t,) in mdb.execute("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name"):
            print(f"    tabla existente {t}: {mdb.execute(f'SELECT count(*) FROM {t}').fetchone()[0]} filas")
    except sqlite3.Error as e:
        print(f"    no se pudo abrir en solo lectura: {e}")
    print("    tablas nuevas propuestas: chain_migrations (una fila por migración del día), "
          "fase0b_runs (día, completo, CU); las llamadas a GeckoTerminal seguirían en api_calls")

    print("\n== Estado")
    print(f"  método válido con datos locales: {'sí' if valid else 'NO'}")
    print("  pendiente en cadena antes del modo real: (1) que tx de migración conocidas incluyan "
          f"{WITHDRAW_AUTHORITY[:8]}…; (2) fracción de su tráfico que son migraciones; (3) cuántas páginas "
          f"hay hasta {args.day}")


if __name__ == "__main__":
    main()
