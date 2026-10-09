#!/usr/bin/env python3
"""Benchmark de streaming (Etapa 1) — logsSubscribe de Alchemy sobre el programa
pump.fun. Solo lectura sobre la cadena: ni wallets, ni firma, ni envío de
transacciones. Solo biblioteca estándar (cliente WebSocket RFC 6455 mínimo, sin
compresión, así que los bytes contados son los que llegan por la red).

Base aparte: marxol.bench.db (no se versiona). marxol.db no se toca.
La clave sale de .env (ALCHEMY_API_KEY). Nunca se imprime ni se registra: los
mensajes de error se limpian de la URL.

Uso (desde la raíz del repo):
    python3 -B scripts/bench_stream.py listen --run prueba10 --minutes 10 --max-cu 300000
    python3 -B scripts/bench_stream.py analyze --run prueba10 [--completeness]

listen
- Una conexión, logsSubscribe {"mentions": [programa pump]} con commitment
  confirmed. Cada notificación se guarda en stream_msgs (firma, slot, err, bytes
  en la red, hora local de recepción en ns).
- Reconecta con espera creciente si cae; cada conexión queda en stream_sessions
  con su motivo de cierre.
- Reanudable: el plazo se fija al crear el run (stream_runs.deadline). Volver a
  lanzar el mismo --run sigue hasta ese plazo.
- Tope de CU: corta cuando la estimación CONSERVADORA (0.04 CU/byte, la tarifa
  genérica de suscripciones de Alchemy) alcanza --max-cu. La tarifa específica
  de Solana en la misma página es 0.0002 CU/byte (200x menos). La página da las
  dos sin aclarar cuál aplica; se registran los bytes y se reportan las dos.

analyze
- Mensajes y bytes por segundo, % de fallidas, duplicados, sesiones y caídas,
  desglose por hora.
- Latencia: recepción local − blockTime del slot (getBlockTime, 20 CU) en una
  muestra de slots repartida por todo el run. blockTime tiene resolución de 1 s
  (entera), así que cada latencia lleva un error de [0, 1) s.
- --completeness: en una ventana de 2 min del run, compara las firmas del stream
  con getSignaturesForAddress del programa (40 CU por página de 1 000).
- Todo el consumo (HTTP y estimación del stream) va a rpc_usage de la base
  bench, con el mismo esquema que marxol.db.
"""
import argparse, base64, json, os, random, socket, sqlite3, ssl, struct, sys, time, urllib.request

PUMP = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
WS_HOST = "solana-mainnet.streaming.alchemy.com"
HTTP_HOST = "solana-mainnet.g.alchemy.com"
CU_PER_BYTE_GENERIC = 0.04     # "All other subscriptions" (compute-unit-costs)
CU_PER_BYTE_SOLANA = 0.0002    # "Solana: WebSocket Subscriptions" (misma página)
CU = {"getBlockTime": 20, "getSignaturesForAddress": 40}

SCHEMA = """
CREATE TABLE IF NOT EXISTS stream_runs (run TEXT PRIMARY KEY, started REAL, deadline REAL, max_cu REAL, commitment TEXT);
CREATE TABLE IF NOT EXISTS stream_sessions (run TEXT, session INTEGER, opened REAL, closed REAL, reason TEXT,
    handshake_bytes INTEGER, PRIMARY KEY (run, session));
CREATE TABLE IF NOT EXISTS stream_msgs (run TEXT, session INTEGER, recv_ns INTEGER, slot INTEGER, signature TEXT,
    failed INTEGER, bytes INTEGER);
CREATE INDEX IF NOT EXISTS stream_msgs_run_slot ON stream_msgs (run, slot);
CREATE TABLE IF NOT EXISTS stream_other (run TEXT, session INTEGER, recv_ns INTEGER, kind TEXT, bytes INTEGER);
CREATE TABLE IF NOT EXISTS block_times (slot INTEGER PRIMARY KEY, block_time INTEGER);
CREATE TABLE IF NOT EXISTS rpc_usage (
    run_at INTEGER NOT NULL, command TEXT NOT NULL, provider TEXT NOT NULL,
    get_transaction INTEGER NOT NULL, get_signatures INTEGER NOT NULL,
    units INTEGER NOT NULL, tokens INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS stream_usage (run_at INTEGER, run TEXT, command TEXT, ws_bytes INTEGER,
    get_block_time INTEGER, get_signatures INTEGER, cu_generic REAL, cu_solana REAL);
"""


def api_key():
    key = os.environ.get("ALCHEMY_API_KEY")
    if not key and os.path.exists(".env"):
        for line in open(".env"):
            k, _, v = line.strip().partition("=")
            if k == "ALCHEMY_API_KEY":
                key = v.strip().strip('"').strip("'")
    if not key:
        sys.exit("falta ALCHEMY_API_KEY (.env)")
    return key


def clean(msg, key):
    return str(msg).replace(key, "***")


def log(f, *a):
    line = time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime()) + " UTC " + " ".join(str(x) for x in a)
    print(line, file=f, flush=True)


# ---------- cliente WebSocket mínimo ----------
class WS:
    def __init__(self, host, path, timeout=60):
        raw = socket.create_connection((host, 443), timeout=timeout)
        self.s = ssl.create_default_context().wrap_socket(raw, server_hostname=host)
        self.rx = 0
        key = base64.b64encode(os.urandom(16)).decode()
        req = (f"GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
               f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n")
        self.s.sendall(req.encode())
        self.buf = b""
        while b"\r\n\r\n" not in self.buf:
            chunk = self.s.recv(4096)
            if not chunk:
                raise ConnectionError("cerrado durante el handshake")
            self.buf += chunk
        head, _, self.buf = self.buf.partition(b"\r\n\r\n")
        self.rx = len(head) + 4
        self.handshake_bytes = self.rx
        status = head.split(b"\r\n", 1)[0].decode(errors="replace")
        if " 101 " not in status + " ":
            raise ConnectionError(f"handshake: {status}")

    def _read(self, n):
        while len(self.buf) < n:
            chunk = self.s.recv(65536)
            if not chunk:
                raise ConnectionError("conexión cerrada por el servidor")
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        self.rx += n
        return out

    def send(self, payload, opcode=1):
        if isinstance(payload, str):
            payload = payload.encode()
        mask = os.urandom(4)
        n = len(payload)
        hdr = bytes([0x80 | opcode])
        if n < 126:
            hdr += bytes([0x80 | n])
        elif n < 65536:
            hdr += bytes([0x80 | 126]) + struct.pack(">H", n)
        else:
            hdr += bytes([0x80 | 127]) + struct.pack(">Q", n)
        self.s.sendall(hdr + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))

    def recv(self):
        """Devuelve (opcode, payload, bytes en la red) de un mensaje completo."""
        start = self.rx
        data, op0 = b"", None
        while True:
            b0, b1 = self._read(2)
            fin, op, n = b0 & 0x80, b0 & 0x0F, b1 & 0x7F
            if n == 126:
                n = struct.unpack(">H", self._read(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self._read(8))[0]
            if b1 & 0x80:
                mk = self._read(4)
                pl = bytes(b ^ mk[i % 4] for i, b in enumerate(self._read(n)))
            else:
                pl = self._read(n)
            if op >= 8:  # control: se devuelve tal cual
                return op, pl, self.rx - start
            op0 = op if op0 is None else op0
            data += pl
            if fin:
                return op0, data, self.rx - start

    def close(self):
        try:
            self.send(b"", opcode=8)
            self.s.close()
        except OSError:
            pass


# ---------- HTTP JSON-RPC ----------
def rpc(key, method, params):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    for attempt in range(5):
        req = urllib.request.Request(f"https://{HTTP_HOST}/v2/{key}", data=body, headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                out = json.load(r)
            if "error" in out:
                raise RuntimeError(f"{method}: {out['error']}")
            return out["result"]
        except urllib.error.HTTPError as e:
            if e.code == 429 and attempt < 4:
                time.sleep(2 ** attempt)
                continue
            raise RuntimeError(clean(f"{method}: HTTP {e.code}", key)) from None


def open_db(path):
    db = sqlite3.connect(path)
    db.executescript(SCHEMA)
    return db


def record_usage(db, run, command, ws_bytes, n_bt, n_sig):
    http = n_bt * CU["getBlockTime"] + n_sig * CU["getSignaturesForAddress"]
    gen, sol = http + ws_bytes * CU_PER_BYTE_GENERIC, http + ws_bytes * CU_PER_BYTE_SOLANA
    now = int(time.time())
    # rpc_usage.units = estimación conservadora (tarifa genérica); stream_usage guarda las dos.
    db.execute("INSERT INTO rpc_usage VALUES (?,?,?,?,?,?,?)", (now, command, "alchemy", 0, n_sig, round(gen), 0))
    db.execute("INSERT INTO stream_usage VALUES (?,?,?,?,?,?,?,?)", (now, run, command, ws_bytes, n_bt, n_sig, gen, sol))
    db.commit()


# ---------- listen ----------
def listen(args):
    key = api_key()
    db = open_db(args.db)
    row = db.execute("SELECT started, deadline, max_cu FROM stream_runs WHERE run=?", (args.run,)).fetchone()
    if row is None:
        now = time.time()
        db.execute("INSERT INTO stream_runs VALUES (?,?,?,?,?)", (args.run, now, now + 60 * args.minutes, args.max_cu, "confirmed"))
        db.commit()
        row = (now, now + 60 * args.minutes, args.max_cu)
    _, deadline, max_cu = row
    prev_bytes = db.execute("SELECT coalesce(sum(ws_bytes),0) FROM stream_usage WHERE run=? AND command='bench-stream'",
                            (args.run,)).fetchone()[0]
    session = db.execute("SELECT coalesce(max(session),0) FROM stream_sessions WHERE run=?", (args.run,)).fetchone()[0]
    lf = open(args.log, "a")
    log(lf, f"run {args.run}: plazo {time.strftime('%Y-%m-%d %H:%M:%S', time.gmtime(deadline))} UTC, tope {max_cu:.0f} CU "
            f"(conservador, {CU_PER_BYTE_GENERIC} CU/byte); bytes previos {prev_bytes}")
    total_bytes, backoff, stop = 0, 1, None
    last_report = time.time()
    while stop is None and time.time() < deadline:
        session += 1
        opened, sess_bytes, reason = time.time(), 0, "?"
        db.execute("INSERT INTO stream_sessions VALUES (?,?,?,?,?,?)", (args.run, session, opened, None, None, 0))
        db.commit()
        try:
            ws = WS(WS_HOST, f"/v2/{key}")
            sess_bytes += ws.handshake_bytes
            ws.send(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "logsSubscribe",
                                "params": [{"mentions": [PUMP]}, {"commitment": "confirmed"}]}))
            ws.s.settimeout(30)
            last_ping, pending = time.time(), []
            while True:
                if time.time() >= deadline:
                    reason = "plazo"
                    stop = "plazo"
                    break
                if (prev_bytes + total_bytes + sess_bytes) * CU_PER_BYTE_GENERIC >= max_cu:
                    reason = stop = "tope de CU"
                    break
                if time.time() - last_ping > 20:
                    ws.send(b"", opcode=9)
                    last_ping = time.time()
                op, pl, nb = ws.recv()
                t = time.time_ns()
                sess_bytes += nb
                if op == 9:
                    ws.send(pl, opcode=10)
                    continue
                if op == 8:
                    reason = f"close del servidor {pl[:2].hex()} {pl[2:].decode(errors='replace')}"
                    break
                if op != 1:
                    db.execute("INSERT INTO stream_other VALUES (?,?,?,?,?)", (args.run, session, t, f"op{op}", nb))
                    continue
                m = json.loads(pl)
                if m.get("method") == "logsNotification":
                    r = m["params"]["result"]
                    v = r["value"]
                    pending.append((args.run, session, t, r["context"]["slot"], v["signature"], int(v["err"] is not None), nb))
                else:
                    kind = "subscribe_ok" if "result" in m else "error" if "error" in m else "otro"
                    db.execute("INSERT INTO stream_other VALUES (?,?,?,?,?)", (args.run, session, t, kind, nb))
                    if kind == "error":
                        reason = stop = clean(f"error de suscripción: {m['error']}", key)
                        break
                    log(lf, f"sesión {session}: suscrito (id {m.get('result')})")
                if len(pending) >= 500 or time.time() - last_report > 5:
                    db.executemany("INSERT INTO stream_msgs VALUES (?,?,?,?,?,?,?)", pending)
                    db.commit()
                    pending = []
                if time.time() - last_report > 60:
                    tb = prev_bytes + total_bytes + sess_bytes
                    log(lf, f"sesión {session}: {tb / 1e6:.1f} MB acumulados, CU conservador {tb * CU_PER_BYTE_GENERIC:.0f}, "
                            f"Solana {tb * CU_PER_BYTE_SOLANA:.0f}")
                    last_report = time.time()
            db.executemany("INSERT INTO stream_msgs VALUES (?,?,?,?,?,?,?)", pending)
            ws.close()
            backoff = 1
        except Exception as e:  # noqa: BLE001 — cualquier caída se registra y se reconecta
            reason = clean(f"{type(e).__name__}: {e}", key)
            try:
                db.executemany("INSERT INTO stream_msgs VALUES (?,?,?,?,?,?,?)", pending)
            except NameError:
                pass
        db.execute("UPDATE stream_sessions SET closed=?, reason=?, handshake_bytes=? WHERE run=? AND session=?",
                   (time.time(), reason, sess_bytes, args.run, session))
        db.commit()
        total_bytes += sess_bytes
        log(lf, f"sesión {session} cerrada: {reason}; {sess_bytes / 1e6:.2f} MB")
        if reason.startswith("ConnectionError: handshake: HTTP/1.1 4"):
            stop = reason  # 401/403/429 en el handshake: no insistir
        if stop is None and time.time() < deadline:
            time.sleep(backoff)
            backoff = min(backoff * 2, 60)
    record_usage(db, args.run, "bench-stream", total_bytes, 0, 0)
    tb = prev_bytes + total_bytes
    log(lf, f"fin: {stop or 'plazo'}; esta ejecución {total_bytes / 1e6:.2f} MB; run acumulado {tb / 1e6:.2f} MB = "
            f"{tb * CU_PER_BYTE_GENERIC:.0f} CU (0.04/byte) o {tb * CU_PER_BYTE_SOLANA:.0f} CU (0.0002/byte)")


# ---------- analyze ----------
def q(db, s, *a):
    return db.execute(s, a).fetchall()


def pctl(v, p):
    v = sorted(v)
    if not v:
        return None
    k = (len(v) - 1) * p
    f = int(k)
    c = min(f + 1, len(v) - 1)
    return v[f] + (v[c] - v[f]) * (k - f)


def analyze(args):
    key = api_key()
    db = open_db(args.db)
    n_bt = n_sig = 0
    run = args.run
    (started, deadline, max_cu), = q(db, "SELECT started, deadline, max_cu FROM stream_runs WHERE run=?", run)
    sess = q(db, "SELECT session, opened, closed, reason, handshake_bytes FROM stream_sessions WHERE run=? ORDER BY session", run)
    live = sum((c or o) - o for _, o, c, _, _ in sess)
    n, nbytes, nfail, t0, t1 = q(db, "SELECT count(*), sum(bytes), sum(failed), min(recv_ns), max(recv_ns) FROM stream_msgs WHERE run=?", run)[0]
    wire = q(db, "SELECT coalesce(sum(ws_bytes),0) FROM stream_usage WHERE run=? AND command='bench-stream'", run)[0][0]
    ndist = q(db, "SELECT count(DISTINCT signature) FROM stream_msgs WHERE run=?", run)[0][0]
    span = (t1 - t0) / 1e9
    out = []
    say = out.append
    say(f"BENCHMARK DE STREAMING — run '{run}' (Alchemy logsSubscribe, mentions = programa pump, commitment confirmed)")
    say(f"inicio {time.strftime('%Y-%m-%d %H:%M:%S', time.gmtime(started))} UTC; primer mensaje → último: {span:.0f} s; "
        f"tiempo conectado {live:.0f} s")
    say(f"sesiones (conexiones) {len(sess)}; reconexiones {len(sess) - 1}")
    for s, o, c, r, hb in sess:
        say(f"  sesión {s}: {time.strftime('%H:%M:%S', time.gmtime(o))}–{time.strftime('%H:%M:%S', time.gmtime(c or o))} UTC, "
            f"{((c or o) - o):.0f} s, {hb / 1e6:.2f} MB, cierre: {r}")
    gaps = [(sess[i + 1][1] - sess[i][2]) for i in range(len(sess) - 1) if sess[i][2]]
    if gaps:
        say(f"  huecos entre sesiones: total {sum(gaps):.0f} s, máx {max(gaps):.0f} s")
    say(f"notificaciones {n} ({ndist} firmas distintas; duplicadas {n - ndist}); {n / span:.1f} msg/s")
    say(f"bytes en la red: {wire / 1e6:.2f} MB (incluye handshake, pings y respuestas); {wire / span / 1e3:.1f} kB/s; "
        f"{nbytes / n:.0f} bytes por notificación de media")
    say(f"transacciones fallidas (err != null): {nfail}/{n} ({100 * nfail / n:.1f} %)")

    # Desglose por hora (UTC).
    hours = q(db, "SELECT recv_ns / 3600000000000 h, count(*), sum(bytes), sum(failed), min(recv_ns), max(recv_ns) "
                  "FROM stream_msgs WHERE run=? GROUP BY h ORDER BY h", run)
    if len(hours) > 1:
        say("\nDesglose por hora (UTC): hora · msg/s · kB/s · % fallidas · CU/h (0.0002 / 0.04 por byte)")
        for h, c, b, f, a, z in hours:
            dur = max((z - a) / 1e9, 1)
            say(f"  {time.strftime('%m-%d %H:00', time.gmtime(h * 3600))}  {c / dur:7.1f}  {b / dur / 1e3:7.1f}  "
                f"{100 * f / c:5.1f} %  {b * CU_PER_BYTE_SOLANA:9.0f} / {b * CU_PER_BYTE_GENERIC:11.0f}  (cobertura {dur:.0f} s)")

    # Latencia: muestra de slots repartida por el run.
    slots = [s for (s,) in q(db, "SELECT DISTINCT slot FROM stream_msgs WHERE run=? ORDER BY slot", run)]
    rng = random.Random(20261009)
    sample = sorted(rng.sample(slots, min(args.latency_slots, len(slots))))
    known = dict(q(db, "SELECT slot, block_time FROM block_times"))
    for s in sample:
        if s not in known:
            try:
                known[s] = rpc(key, "getBlockTime", [s])
            except RuntimeError as e:
                known[s] = None
                print(clean(e, key), file=sys.stderr)
            n_bt += 1
            db.execute("INSERT OR REPLACE INTO block_times VALUES (?,?)", (s, known[s]))
    db.commit()
    lat, first_lat = [], []
    for s in sample:
        bt = known.get(s)
        if bt is None:
            continue
        rec = [r for (r,) in q(db, "SELECT recv_ns FROM stream_msgs WHERE run=? AND slot=?", run, s)]
        lat += [r / 1e9 - bt for r in rec]
        first_lat.append(min(rec) / 1e9 - bt)
    say(f"\nLatencia recepción local − blockTime ({len(first_lat)} slots muestreados con getBlockTime; {len(lat)} mensajes)")
    for name, v in (("todos los mensajes", lat), ("primer mensaje de cada slot", first_lat)):
        if v:
            say(f"  {name}: mediana {pctl(v, .5):.2f} s, p90 {pctl(v, .9):.2f} s, p99 {pctl(v, .99):.2f} s, "
                f"mín {min(v):.2f} s, máx {max(v):.2f} s")
    say("  (blockTime es entero en segundos: cada valor lleva un error de [0, 1) s; reloj local sincronizado por NTP)")

    # Completitud.
    if args.completeness:
        mid = t0 + (t1 - t0) // 2
        a_ns, b_ns = mid - 60 * 10**9, mid + 60 * 10**9
        lo, hi = q(db, "SELECT min(slot), max(slot) FROM stream_msgs WHERE run=? AND recv_ns BETWEEN ? AND ?", run, a_ns, b_ns)[0]
        # Rango cerrado de slots, dejando fuera los extremos (pueden estar a medias por orden de llegada).
        lo, hi = lo + 2, hi - 2
        stream = {s for (s,) in q(db, "SELECT signature FROM stream_msgs WHERE run=? AND slot BETWEEN ? AND ?", run, lo, hi)}
        anchor = q(db, "SELECT signature FROM stream_msgs WHERE run=? AND slot > ? ORDER BY slot DESC LIMIT 1", run, hi)
        before = anchor[0][0] if anchor else None
        api = {}
        while True:
            params = {"limit": 1000, "commitment": "confirmed"}
            if before:
                params["before"] = before
            page = rpc(key, "getSignaturesForAddress", [PUMP, params])
            n_sig += 1
            if not page:
                break
            for r in page:
                if lo <= r["slot"] <= hi:
                    api[r["signature"]] = r["err"] is not None
            before = page[-1]["signature"]
            if page[-1]["slot"] < lo:
                break
        missing = set(api) - stream
        extra = stream - set(api)
        mf = sum(api[s] for s in missing)
        say(f"\nCompletitud (ventana de ~2 min en mitad del run: slots [{lo}, {hi}], {hi - lo + 1} slots; "
            f"{n_sig} páginas de getSignaturesForAddress)")
        say(f"  getSignaturesForAddress: {len(api)} firmas; stream: {len(stream)}")
        say(f"  perdidas por el stream: {len(missing)} ({100 * len(missing) / max(len(api), 1):.2f} %; de ellas fallidas {mf}); "
            f"completitud {100 * (1 - len(missing) / max(len(api), 1)):.2f} %")
        say(f"  en el stream pero no en getSignaturesForAddress: {len(extra)}")

    # Consumo y extrapolación.
    record_usage(db, run, "bench-analyze", 0, n_bt, n_sig)
    rate = wire / span
    say(f"\nConsumo del stream: {wire / 1e6:.2f} MB → {wire * CU_PER_BYTE_SOLANA:,.0f} CU a 0.0002 CU/byte (tarifa Solana) "
        f"o {wire * CU_PER_BYTE_GENERIC:,.0f} CU a 0.04 CU/byte (tarifa genérica)")
    say(f"análisis: {n_bt} getBlockTime + {n_sig} getSignaturesForAddress = "
        f"{n_bt * CU['getBlockTime'] + n_sig * CU['getSignaturesForAddress']} CU")
    say(f"extrapolación a 24 h al ritmo medido ({rate / 1e3:.1f} kB/s = {rate * 86400 / 1e9:.1f} GB/día): "
        f"{rate * 86400 * CU_PER_BYTE_SOLANA:,.0f} CU/día (0.0002) o {rate * 86400 * CU_PER_BYTE_GENERIC:,.0f} CU/día (0.04)")
    say("  (el CU real lo dice el panel de Alchemy, no esta estimación; Alchemy no devuelve el CU por mensaje)")
    text = "\n".join(out)
    print(text)
    if args.out:
        open(args.out, "w").write(text + "\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--db", default="marxol.bench.db")
    sub = ap.add_subparsers(dest="cmd", required=True)
    li = sub.add_parser("listen")
    li.add_argument("--run", required=True)
    li.add_argument("--minutes", type=float, default=10)
    li.add_argument("--max-cu", type=float, default=300_000)
    li.add_argument("--log", default="marxol.bench.log")
    an = sub.add_parser("analyze")
    an.add_argument("--run", required=True)
    an.add_argument("--latency-slots", type=int, default=150)
    an.add_argument("--completeness", action="store_true")
    an.add_argument("--out")
    args = ap.parse_args()
    {"listen": listen, "analyze": analyze}[args.cmd](args)


if __name__ == "__main__":
    main()
