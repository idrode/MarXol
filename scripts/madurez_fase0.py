#!/usr/bin/env python3
"""Tokens maduros ("segunda vida"), fase 0: viabilidad de datos — POST HOC, EXPLORATORIO, SIN PRE-REGISTRO.
No analiza RSI ni whales como señal. Solo red hacia la API pública de GeckoTerminal (sin clave). Sin Alchemy.
marxol.db solo en lectura; los resultados van a madurez.db y las respuestas a madurez_cache/ (no versionados).

Reproducir (desde la raíz del repo; reanudable, todo lo pedido queda en caché):
    python3 -B scripts/madurez_fase0.py fetch          # descarga (≈ 6 s por llamada nueva)
    python3 -B scripts/madurez_fase0.py report > resultados/madurez_fase0.txt   # sin red

Ritmo: 10 llamadas/min (la documentación de la API pública dice "approximately 10 calls/minute"; el límite
propio de la tarea es 20). Ante un 429, espera de 60 s y reintento, hasta 4 veces; cada intento cuenta. Tope
de la fase: 600 llamadas reales (no cacheadas), contadas en madurez.db (api_calls). Al alcanzarlo se para.

Definiciones (fijadas antes de descargar):
- Velas: /ohlcv/hour?aggregate=1&limit=1000&currency=token (precio en unidades del quote, sin la deriva
  SOL/USD), paginando con before_timestamp hasta llegar a la creación del pool, a una página vacía o a 6
  páginas por pool.
- Horas de historia = desde pool_created_at hasta la última vela. % de horas sin vela = 1 − velas / horas de
  ese intervalo. % de horas con volumen 0 = velas con volumen 0 / velas.
- "El historial llega a la graduación": primera vela ≤ pool_created_at + 1 h (el pool PumpSwap se crea en la
  migración).
- Segunda vida (candidatas, sin elegir ninguna): con X ∈ {50, 70, 90} % y D ∈ {7, 14, 30} días. t1 = primera
  hora en que low ≤ (1 − X) · máximo de high desde el inicio. Cumple si en (t1, t1 + D días] alguna vela tiene
  high ≥ 2 × el mínimo de low en [t1, esa vela]. n/e si no cumple y la historia tras t1 no llega a D días.
"""
import argparse, collections, hashlib, json, os, random, sqlite3, statistics, sys, time, urllib.error, urllib.request

BASE = "https://api.geckoterminal.com/api/v2"
CACHE = "madurez_cache"
DB = "madurez.db"
MAX_CALLS = 600
PER_MIN = 10
SEED = 20261009
DROPS = [0.50, 0.70, 0.90]
DAYS = [7, 14, 30]
MAX_PAGES = 6


class Cap(Exception):
    pass


def db():
    d = sqlite3.connect(DB)
    d.executescript("""
    CREATE TABLE IF NOT EXISTS api_calls (ts REAL, url TEXT, status INTEGER);
    CREATE TABLE IF NOT EXISTS pools (address TEXT, source TEXT, dex TEXT, base_token TEXT, created_at TEXT,
        reserve_usd REAL, vol24_usd REAL, PRIMARY KEY (address, source));
    """)
    return d


def get(d, path, **params):
    q = "&".join(f"{k}={v}" for k, v in params.items() if v is not None)
    url = f"{BASE}{path}" + (f"?{q}" if q else "")
    fn = os.path.join(CACHE, hashlib.sha256(url.encode()).hexdigest() + ".json")
    if os.path.exists(fn):
        return json.load(open(fn))
    for attempt in range(5):
        n = d.execute("SELECT count(*) FROM api_calls").fetchone()[0]
        if n >= MAX_CALLS:
            raise Cap(f"tope de {MAX_CALLS} llamadas alcanzado")
        last = d.execute("SELECT max(ts) FROM api_calls").fetchone()[0] or 0
        time.sleep(max(0, last + 60 / PER_MIN - time.time()))
        req = urllib.request.Request(url, headers={"Accept": "application/json;version=20230203", "User-Agent": "marxol-fase0"})
        status, body = None, None
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                status, body = r.status, json.load(r)
        except urllib.error.HTTPError as e:
            status = e.code
            try:
                body = json.load(e)
            except Exception:  # noqa: BLE001
                body = None
        except (urllib.error.URLError, TimeoutError) as e:
            status = -1
            print(f"error de red: {e}", file=sys.stderr)
        d.execute("INSERT INTO api_calls VALUES (?,?,?)", (time.time(), url, status))
        d.commit()
        if status == 200:
            os.makedirs(CACHE, exist_ok=True)
            json.dump(body, open(fn, "w"))
            return body
        if status in (401, 404):
            # Se cachea: 404 = no existe; 401 = fuera de lo que da la API gratuita (p. ej. velas demasiado antiguas).
            os.makedirs(CACHE, exist_ok=True)
            json.dump({"_status": status, "body": body}, open(fn, "w"))
            return {"_status": status, "body": body}
        print(f"HTTP {status} en {url}; reintento {attempt + 1}", file=sys.stderr)
        time.sleep(60 if status == 429 else 10)
    raise RuntimeError(f"sin respuesta válida: {url}")


def pool_rows(resp, source):
    out = []
    for p in resp.get("data") or []:
        a = p["attributes"]
        rel = p.get("relationships", {})
        out.append((a["address"], source, rel.get("dex", {}).get("data", {}).get("id"),
                    rel.get("base_token", {}).get("data", {}).get("id", "").removeprefix("solana_"),
                    a.get("pool_created_at"), float(a.get("reserve_in_usd") or 0),
                    float((a.get("volume_usd") or {}).get("h24") or 0)))
    return out


def marxol_graduated():
    m = sqlite3.connect("file:marxol.db?mode=ro", uri=True)
    return m.execute("SELECT g.mint, g.pool, g.timestamp, g.quote_mint, c.timestamp FROM migrations g "
                     "JOIN creations c USING (mint) ORDER BY g.timestamp").fetchall()


def iso(ts):
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(ts))


def parse_ts(s):
    return int(time.mktime(time.strptime(s[:19], "%Y-%m-%dT%H:%M:%S"))) - time.timezone if s else None


def dex_id(d):
    for page in range(1, 6):
        r = get(d, "/networks/solana/dexes", page=page)
        ids = [x["id"] for x in r.get("data") or []]
        hit = [i for i in ids if "pump" in i.lower()]
        if any(i in ("pumpswap", "pump-swap") for i in hit):
            return next(i for i in hit if i in ("pumpswap", "pump-swap")), hit
        if not ids:
            break
    return (hit[0] if hit else None), hit


def ohlcv(d, pool, created):
    """Todas las velas de 1 h del pool (paginando hacia atrás)."""
    rows, before = {}, None
    for page in range(MAX_PAGES):
        r = get(d, f"/networks/solana/pools/{pool}/ohlcv/hour", aggregate=1, limit=1000, currency="token",
                before_timestamp=before)
        if r.get("_status") == 404:
            return None, page + 1
        if r.get("_status") == 401:
            return [(ts,) + rows[ts] for ts in sorted(rows)], page + 1
        lst = ((r.get("data") or {}).get("attributes") or {}).get("ohlcv_list") or []
        for ts, o, h, l, c, v in lst:
            rows[int(ts)] = (float(o), float(h), float(l), float(c), float(v))
        if len(lst) < 1000 or (created and min(int(x[0]) for x in lst) <= created):
            return [(ts,) + rows[ts] for ts in sorted(rows)], page + 1
        before = min(int(x[0]) for x in lst)
    return [(ts,) + rows[ts] for ts in sorted(rows)], MAX_PAGES


def sample(d, grad_pools):
    """50 pools: los graduados de marxol.db (todos > 7 días) + pools de los listados por estratos de edad."""
    # Edad respecto a la primera llamada de la fase, para que la muestra sea la misma en fetch y en report.
    now = d.execute("SELECT min(ts) FROM api_calls").fetchone()[0]
    seen = {p for p in grad_pools}
    cand = collections.defaultdict(list)
    for addr, dex, created in d.execute("SELECT DISTINCT address, dex, created_at FROM pools"):
        if addr in seen or created is None or (dex or "").lower() not in ("pumpswap", "pump-swap"):
            continue
        age = (now - parse_ts(created)) / 86400
        b = "<1 d" if age < 1 else "1–7 d" if age < 7 else "7–30 d" if age < 30 else ">30 d"
        cand[b].append(addr)
    rng = random.Random(SEED)
    need = 50 - len(grad_pools)
    buckets = ["<1 d", "1–7 d", "7–30 d", ">30 d"]
    # Reparto: cuotas iguales por estrato, el sobrante a los estratos con más candidatos.
    pick = {}
    for b in buckets:
        cand[b] = sorted(set(cand[b]))
        rng.shuffle(cand[b])
    i = 0
    while sum(len(v) for v in pick.values()) < need and any(cand[b] for b in buckets):
        b = buckets[i % 4]
        if cand[b]:
            pick.setdefault(b, []).append(cand[b].pop())
        i += 1
    return pick


def fetch(args):
    d = db()
    try:
        did, hits = dex_id(d)
        print(f"dex PumpSwap: {did} (ids con 'pump': {hits})")
        for sort in ("h24_volume_usd_desc", "h24_tx_count_desc"):
            for page in range(1, 11):
                r = get(d, f"/networks/solana/dexes/{did}/pools", page=page, sort=sort, include="base_token")
                rows = pool_rows(r, f"dex:{sort}")
                d.executemany("INSERT OR REPLACE INTO pools VALUES (?,?,?,?,?,?,?)", rows)
                if not rows:
                    break
        for page in range(1, 11):
            r = get(d, "/networks/solana/new_pools", page=page, include="base_token")
            rows = pool_rows(r, "new_pools")
            d.executemany("INSERT OR REPLACE INTO pools VALUES (?,?,?,?,?,?,?)", rows)
            if not rows:
                break
        d.commit()
        grad = marxol_graduated()
        pools = [g[1] for g in grad]
        r = get(d, f"/networks/solana/pools/multi/{','.join(pools)}", include="base_token")
        d.executemany("INSERT OR REPLACE INTO pools VALUES (?,?,?,?,?,?,?)", pool_rows(r, "marxol"))
        mints = [g[0] for g in grad]
        get(d, f"/networks/solana/tokens/multi/{','.join(mints)}", include="top_pools", include_inactive_source="true")
        d.commit()
        pick = sample(d, pools)
        chosen = pools + [a for b in pick.values() for a in b]
        # Datos de pool (liquidez) de la muestra de listados ya vienen en pools; velas de todos.
        created = {a: parse_ts(c) for a, c in d.execute("SELECT address, created_at FROM pools")}
        for a in chosen:
            ohlcv(d, a, created.get(a))
        # D: un ejemplo de trades y de token info (campos disponibles).
        get(d, f"/networks/solana/pools/{pools[0]}/trades")
        get(d, f"/networks/solana/tokens/{mints[0]}/info")
        top = d.execute("SELECT address, base_token FROM pools WHERE source LIKE 'dex:h24_volume%' "
                        "ORDER BY vol24_usd DESC LIMIT 1").fetchone()
        if top:
            get(d, f"/networks/solana/pools/{top[0]}/trades")
            get(d, f"/networks/solana/tokens/{top[1]}/info")
    except Cap as e:
        print(f"PARADA: {e}")
    n = d.execute("SELECT count(*), sum(status=429), sum(status=200) FROM api_calls").fetchone()
    print(f"llamadas reales acumuladas {n[0]} (429: {n[1]}, 200: {n[2]})")


# ---------------- informe (sin red) ----------------
def cached(path, **params):
    q = "&".join(f"{k}={v}" for k, v in params.items() if v is not None)
    url = f"{BASE}{path}" + (f"?{q}" if q else "")
    fn = os.path.join(CACHE, hashlib.sha256(url.encode()).hexdigest() + ".json")
    return json.load(open(fn)) if os.path.exists(fn) else None


LIMITED = set()  # pools cuya paginación de velas acabó en 401 (historia gratuita agotada)


def cached_ohlcv(pool, created):
    rows, before = {}, None
    for page in range(MAX_PAGES):
        r = cached(f"/networks/solana/pools/{pool}/ohlcv/hour", aggregate=1, limit=1000, currency="token",
                   before_timestamp=before)
        if r is None or r.get("_status") == 404:
            return (None if r else "sin caché"), page
        if r.get("_status") == 401:
            LIMITED.add(pool)
            return [(ts,) + rows[ts] for ts in sorted(rows)], page + 1
        lst = ((r.get("data") or {}).get("attributes") or {}).get("ohlcv_list") or []
        for ts, o, h, l, c, v in lst:
            rows[int(ts)] = (float(o), float(h), float(l), float(c), float(v))
        if len(lst) < 1000 or (created and min(int(x[0]) for x in lst) <= created):
            return [(ts,) + rows[ts] for ts in sorted(rows)], page + 1
        before = min(int(x[0]) for x in lst)
    return [(ts,) + rows[ts] for ts in sorted(rows)], MAX_PAGES


def closes(c):
    """La misma serie con high = low = close (variante sin mechas)."""
    return [(ts, cl, cl, cl, cl, v) for ts, o, h, l, cl, v in c]


def second_life(c, x, days):
    """True / False / None (n/e)."""
    if not c:
        return None
    peak, t1, i1 = 0.0, None, None
    for i, (ts, o, h, l, cl, v) in enumerate(c):
        peak = max(peak, h)
        if l <= (1 - x) * peak:
            t1, i1 = ts, i
            break
    if t1 is None:
        return False  # no hay caída: no cumple
    lo = float("inf")
    for ts, o, h, l, cl, v in c[i1:]:
        if ts > t1 + days * 86400:
            return False
        lo = min(lo, l)
        if h >= 2 * lo and ts > t1:
            return True
    return False if c[-1][0] >= t1 + days * 86400 else None


def has_drop(c, x):
    peak = 0.0
    for ts, o, h, l, cl, v in c:
        peak = max(peak, h)
        if l <= (1 - x) * peak:
            return True
    return False


def pct(v, p):
    v = sorted(v)
    if not v:
        return float("nan")
    k = (len(v) - 1) * p
    f = int(k)
    return v[f] + (v[min(f + 1, len(v) - 1)] - v[f]) * (k - f)


def report(args):
    d = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    now = d.execute("SELECT max(ts) FROM api_calls").fetchone()[0]
    ncalls, n429, n200, n404 = d.execute("SELECT count(*), sum(status=429), sum(status=200), sum(status=404) FROM api_calls").fetchone()
    P = print
    P("TOKENS MADUROS ('SEGUNDA VIDA'), FASE 0: VIABILIDAD DE DATOS — POST HOC, EXPLORATORIO, SIN PRE-REGISTRO")
    P("Fuente: API pública de GeckoTerminal (sin clave). marxol.db solo en lectura. Sin Alchemy, sin CU.")
    P(f"Llamadas reales: {ncalls} de un tope de {MAX_CALLS} (200: {n200}, 404: {n404}, 429: {n429}); ritmo {PER_MIN}/min.")
    P(f"Referencia temporal: última llamada {iso(now)}.")

    P("\n" + "=" * 100)
    P("A. UNIVERSO")
    P("A1. Listados de pools")
    for src in ("dex:h24_volume_usd_desc", "dex:h24_tx_count_desc", "new_pools"):
        rows = d.execute("SELECT created_at, dex, reserve_usd FROM pools WHERE source=?", (src,)).fetchall()
        ts = sorted(parse_ts(c) for c, _, _ in rows if c)
        dexes = collections.Counter(x for _, x, _ in rows)
        if not ts:
            P(f"  {src}: 0 pools")
            continue
        span_d = (ts[-1] - ts[0]) / 86400
        P(f"  {src}: {len(rows)} pools (10 páginas × 20 como máximo); creación más antigua {iso(ts[0])}, más reciente "
          f"{iso(ts[-1])}; intervalo {span_d * 24:.2f} h" + (f" → ≈ {len(ts) / span_d:,.0f} pools/día (todas las DEX)" if src == "new_pools" and span_d > 0 else ""))
        P(f"      DEX: {dict(dexes.most_common(6))}")
        if src.startswith("dex"):
            ages = [(now - t) / 86400 for t in ts]
            P(f"      edad (días): mediana {pct(ages, .5):.1f}, p10 {pct(ages, .1):.1f}, p90 {pct(ages, .9):.1f}")
    nps = d.execute("SELECT created_at FROM pools WHERE source='new_pools' AND dex IN ('pumpswap','pump-swap')").fetchall()
    if nps:
        t = sorted(parse_ts(c) for (c,) in nps)
        sp = (t[-1] - t[0]) / 86400
        P(f"  new_pools solo PumpSwap: {len(t)} pools en {sp * 24:.1f} h" + (f" → ≈ {len(t) / sp:.0f} graduaciones/día a PumpSwap" if sp > 0 else ""))
    P("  Historia de velas: con before_timestamp anterior a ~2026-03-14 la API gratuita responde 401 (B).")
    P("  Conclusión A1: los dos endpoints de listado devuelven como mucho 200 pools (10 páginas). 'Top pools por DEX'")
    P("  ordena por volumen o número de tx de las últimas 24 h, así que solo lista supervivientes activos. 'New pools'")
    P("  solo cubre las últimas horas. No hay forma de listar los pools de un periodo pasado con la API pública.")

    P("\nA2. Sesgo de supervivencia: los tokens de marxol.db que graduaron (migración a PumpSwap observada)")
    grad = marxol_graduated()
    got = {a: (r, v) for a, r, v in d.execute("SELECT address, reserve_usd, vol24_usd FROM pools WHERE source='marxol'")}
    toks = cached(f"/networks/solana/tokens/multi/{','.join(g[0] for g in grad)}", include="top_pools", include_inactive_source="true")
    tok_found = {x["attributes"]["address"] for x in (toks or {}).get("data") or []}
    in_lists = {a for (a,) in d.execute("SELECT address FROM pools WHERE source != 'marxol'")}
    P(f"  graduados en marxol.db: {len(grad)} (solo los que graduaron dentro de las ventanas de 65 min descargadas;")
    P("  los que graduaron más tarde no se conocen sin RPC)")
    with_c = 0
    for mint, pool, mts, qm, cts in grad:
        c, _ = cached_ohlcv(pool, mts)
        nv = len(c) if isinstance(c, list) else 0
        with_c += nv > 0
        r = got.get(pool)
        quote = "SOL" if qm and qm.startswith("1111") else (qm or "?")[:6]
        pm = f"sí, liquidez ${r[0]:,.0f}, vol24 ${r[1]:,.0f}" if r else "NO"
        P(f"   {mint[:8]}… pool {pool[:8]}… graduó {iso(mts)} quote {quote}  en pools/multi: {pm}", end="")
        P(f"  velas 1 h: {nv}  token en tokens/multi: {'sí' if mint in tok_found else 'NO'}  en listados top/new: {'sí' if pool in in_lists else 'no'}")
    P(f"  con pool en GeckoTerminal: {len(got)}/{len(grad)}; con alguna vela de 1 h: {with_c}/{len(grad)}; en los listados: "
      f"{sum(g[1] in in_lists for g in grad)}/{len(grad)}")
    dead = sum(1 for r, v in got.values() if r < 1000)
    P(f"  pools con liquidez < $1 000 que siguen apareciendo por dirección: {dead}/{len(got)} "
      f"(liquidez mediana ${pct([r for r, _ in got.values()], .5):,.0f})")
    lowl = d.execute("SELECT count(*), sum(reserve_usd < 1000) FROM pools WHERE source LIKE 'dex:%'").fetchone()
    P(f"  en 'top pools por DEX': {lowl[1]} de {lowl[0]} con liquidez < $1 000")

    P("\nA3. Otro camino para listar migraciones de un periodo (estimación escrita, NO ejecutada)")
    for line in A3.strip("\n").split("\n"):
        P("  " + line)

    P("\n" + "=" * 100)
    P("B. VELAS DE 1 HORA (muestra de 50 pools)")
    created = {a: parse_ts(c) for a, c in d.execute("SELECT address, created_at FROM pools")}
    liq = {a: r for a, r in d.execute("SELECT address, max(reserve_usd) FROM pools GROUP BY address")}
    grad_pools = [g[1] for g in grad]
    pick = sample(d, grad_pools)
    groups = [("graduados marxol.db", grad_pools)] + [(f"listados, edad {b}", v) for b, v in pick.items()]
    P(f"  muestra: {', '.join(f'{k} {len(v)}' for k, v in groups)} = {sum(len(v) for _, v in groups)} pools (semilla {SEED})")
    P("  ATENCIÓN: los pools de listados salen de 'top por volumen 24 h' y 'new pools': son supervivientes por construcción.")
    P("  La API omite las horas sin trades (include_empty_intervals=false), así que '%vol 0' es siempre 0 y la")
    P("  ausencia de actividad se mide como hora sin vela.")
    P("  horas = de la creación del pool a la última vela; %sin vela = 1 − velas/horas; %sin vela hoy = sobre las horas")
    P("  desde la creación hasta la referencia temporal; '401' = la paginación llegó al límite de historia gratuita.")
    P(f"  {'pool':10} {'grupo':20} {'edad d':>7} {'horas':>6} {'velas':>6} {'%sin vela':>9} {'%sin hoy':>8} {'%vol 0':>7} {'liquidez $':>12} {'llega a grad.':>13} págs")
    series, pages_used, summary = {}, [], []
    for g, lst in groups:
        for a in lst:
            cr = created.get(a)
            c, pg = cached_ohlcv(a, cr)
            pages_used.append(pg)
            if not isinstance(c, list) or not c:
                P(f"  {a[:8]}… {g:20} {'':>7} sin velas ({c if isinstance(c, str) else '404 o vacío'})")
                continue
            series[a] = c
            start = cr or c[0][0]
            if a in LIMITED:  # historia cortada por el 401: las horas se cuentan desde la primera vela obtenida
                start = c[0][0]
            hours = c[-1][0] // 3600 - start // 3600 + 1  # horas naturales (las velas van alineadas a la hora)
            nov = 1 - len(c) / hours if hours > 0 else float("nan")
            v0 = sum(x[5] == 0 for x in c) / len(c)
            reach = cr is not None and c[0][0] <= cr + 3600
            age = (now - (cr or c[0][0])) / 86400  # edad real del pool, aunque la historia esté cortada
            today = int(now) // 3600 - start // 3600 + 1
            nov_today = 1 - len(c) / today if today > 0 else float("nan")
            summary.append((g, age, hours, nov, v0, liq.get(a), reach, nov_today))
            P(f"  {a[:8]}… {g:20} {age:7.1f} {hours:6.0f} {len(c):6} {100 * nov:8.1f}% {100 * nov_today:7.1f}% {100 * v0:6.1f}% "
              f"{(liq.get(a) or 0):12,.0f} {'sí' if reach else 'NO':>13} {pg}" + (" 401" if a in LIMITED else ""))
    P("  Resumen por grupo (medianas):")
    for g, _ in groups:
        s = [x for x in summary if x[0] == g]
        if s:
            P(f"    {g:20} n {len(s):2}  horas {pct([x[2] for x in s], .5):6.0f}  %sin vela {100 * pct([x[3] for x in s], .5):5.1f}%  "
              f"%sin hoy {100 * pct([x[7] for x in s], .5):5.1f}%  %vol 0 {100 * pct([x[4] for x in s], .5):5.1f}%  liquidez ${pct([x[5] or 0 for x in s], .5):,.0f}  "
              f"llegan a la graduación {sum(x[6] for x in s)}/{len(s)}")
    old = [x for x in summary if x[1] > 7]
    P(f"  pools con > 7 días: {len(old)}; con la historia cortada por el límite gratuito (401): {len(LIMITED)}")

    P("\nB5. Coste de descargar 1 000 tokens")
    avg_pages = statistics.mean(p for p in pages_used if p) if pages_used else 1
    calls = 1000 * avg_pages + 1000 / 30
    P(f"  páginas de velas por pool en la muestra: media {avg_pages:.2f} (1 000 velas por página ≈ 41.7 días de historia)")
    P(f"  1 000 tokens ≈ {calls:,.0f} llamadas (velas + pools/multi de 30 en 30), más el listado del universo, que esta API")
    P(f"  no da (A1). A 10/min: {calls / 10 / 60:.1f} h; a 20/min: {calls / 20 / 60:.1f} h; a 30/min: {calls / 30 / 60:.1f} h.")
    P("  Cada mes más de historia añade ~0.72 páginas por token.")

    P("\n" + "=" * 100)
    P("C. TASA BASE DE 'SEGUNDA VIDA' (definiciones candidatas, sin elegir ninguna)")
    P("  AVISO: la definición final se fijará después, SIN mirar indicadores. Esta tabla solo dice qué tasas base son")
    P("  manejables. Ninguna combinación se elige por su resultado.")
    P("  Celda: cumplen / evaluables (%), y n/e = historia insuficiente tras la caída. Velas high/low, precio en quote.")
    for variant, tf in (("VELAS (high/low, definición candidata tal cual)", lambda c: c),
                        ("VARIANTE DESCRIPTIVA: solo cierres horarios (sin mechas; NO elegida, solo para ver el peso de las mechas)", closes)):
      P(f"\n  ### {variant}")
      for g, lst in [("toda la muestra", [a for _, l in groups for a in l]), ("graduados marxol.db (sin sesgo de supervivencia)", grad_pools),
                   ("listados (supervivientes)", [a for b in pick.values() for a in b])]:
        cs = [tf(series[a]) for a in lst if a in series]
        P(f"\n  [{g}] n con velas = {len(cs)}")
        P("    caída  " + "  ".join(f"{'D=' + str(dd) + ' d':>20}" for dd in DAYS) + "   con caída")
        for x in DROPS:
            cells = []
            for dd in DAYS:
                r = [second_life(c, x, dd) for c in cs]
                ev = [v for v in r if v is not None]
                yes = sum(ev)
                cells.append(f"{yes:3}/{len(ev):3} ({100 * yes / len(ev) if ev else 0:4.0f}%) n/e {len(r) - len(ev):2}")
            drop = sum(1 for c in cs if has_drop(c, x))
            P(f"    ≥{100 * x:.0f} %  " + "  ".join(f"{s:>20}" for s in cells) + f"   {drop}/{len(cs)}")

    P("\n" + "=" * 100)
    P("D. DATOS DE WHALES GRATIS (documentación + una respuesta de ejemplo; sin implementar)")
    tr = cached(f"/networks/solana/pools/{grad[0][1]}/trades") if grad else None
    top = d.execute("SELECT address, base_token FROM pools WHERE source LIKE 'dex:h24_volume%' ORDER BY vol24_usd DESC LIMIT 1").fetchone()
    tr2 = cached(f"/networks/solana/pools/{top[0]}/trades") if top else None
    for name, t in (("pool graduado de marxol.db", tr), ("pool con más volumen 24 h", tr2)):
        if t and t.get("data") is not None:
            data = t["data"]
            ts = sorted(parse_ts(x["attributes"]["block_timestamp"]) for x in data) if data else []
            keys = sorted(data[0]["attributes"].keys()) if data else []
            P(f"  trades, {name}: {len(data)} trades" + (f", de {iso(ts[0])} a {iso(ts[-1])}" if ts else "") + f"; campos: {', '.join(keys)}")
    info = cached(f"/networks/solana/tokens/{top[1]}/info") if top else None
    if info and info.get("data"):
        a = info["data"]["attributes"]
        P(f"  token info: campos {', '.join(sorted(a.keys()))}")
        if "holders" in a:
            P(f"    holders: {json.dumps(a['holders'])[:300]}")
    for line in D.strip("\n").split("\n"):
        P("  " + line)

    P("\n" + "=" * 100)
    P("LÍMITES")
    for line in LIMITS.strip("\n").split("\n"):
        P(line)


A3 = """
Camino on-chain (no ejecutado; sin coste real en esta fase):
- Cada graduación emite CompletePumpAmmMigrationEvent en la tx de `migrate` del programa pump (sección 4).
  Una dirección que solo toquen las migraciones permitiría listar con getSignaturesForAddress (40 CU por página
  de 1 000 firmas en Alchemy) y getTransaction (40 CU) solo para esas firmas. Candidatas, sin verificar: la
  wallet que llama a `migrate` (si pump usa una propia) o una PDA de PumpSwap usada solo al crear pools. La
  mint-authority sirvió así para las creaciones (sección 11).
- Volumen: con ~0.4–0.8 creaciones/s (35 000–70 000/día) y una graduación del orden del 1 % (16 de 1 015 en
  marxol.db solo en la primera hora), serían unos 350–1 000 migraciones al día.
- Coste estimado para 30 días (10 000–30 000 migraciones): getTransaction 0.4–1.2 M CU, más el listado. Con una
  dirección exclusiva, 10–30 páginas ≈ 400–1 200 CU. Si la dirección también tiene otro tráfico, se multiplica
  por la proporción de ruido (con el programa pump entero sería inviable: ~117 tx/s).
- Total ≈ 0.4–1.3 M CU por mes de migraciones (1–4 % del free tier mensual), más las velas, que pueden seguir
  saliendo de GeckoTerminal por dirección de pool (el pool viene en el propio evento).
- Alternativa sin RPC: indexadores públicos (p. ej. Dune) con tablas de migraciones. Fuera del alcance de esta fase.
"""

D = """
Según la documentación pública (swagger v2, versión 20230203):
- /pools/{pool}/trades: "the last 300 trades in the past 24 hours". trading_period 7d/30d, cursor y per_page
  son "Pro API only" (de pago). Gratis: como mucho 300 trades de las últimas 24 h por pool, sin paginar. Sirve
  para un detector en vivo (sondear cada pool vigilado cada pocos minutos), no para historia. Con 10 llamadas
  por minuto, vigilar N pools exige N/10 minutos por vuelta.
- Filtro trade_volume_in_usd_greater_than (gratis): permite pedir solo trades grandes del día.
- Holders: no hay LISTA de holders principales (direcciones) en la API pública. Lo que sí da gratis
  /tokens/{address}/info (ver la respuesta de arriba) es el número de holders y la distribución agregada
  (% del supply en el top 10, 11–20, 21–40 y resto), con su `last_updated`, además de
  developer_holding_percentage, is_honeypot, mint_authority y freeze_authority. Es una foto del momento:
  no hay historia, así que solo sirve si se pide periódicamente y se guarda. La lista de top holders con
  direcciones es, según CoinGecko, de la API de pago (conocimiento previo, no verificado en esta sesión,
  porque la red estaba limitada a GeckoTerminal).
- OHLCV gratis: solo agregados por hora o minuto, sin wallets.
- Profundidad real de los 300 trades: en el pool con más volumen cubrieron unos 3 min (ver arriba), y en un
  pool muerto, 0 trades. En pools activos, "las últimas 24 h" son en la práctica los últimos minutos.
- Conclusión: gratis se pueden detectar los trades grandes de los últimos minutos u horas por pool, con su
  wallet (tx_from_address), y la concentración agregada de holders del momento. No hay historia de trades,
  ni direcciones de holders, ni historia de la concentración. Para historia de whales hace falta on-chain (RPC)
  o un plan de pago.
"""

LIMITS = """
- Post hoc y exploratorio: no confirma ni descarta nada y no cambia ningún veredicto ni definición.
- Sesgo de supervivencia en los listados: 'top pools' ordena por actividad de 24 h y 'new pools' solo tiene
  horas. Solo los graduados de marxol.db (n pequeño, seleccionados por graduar en la primera hora) no dependen
  de la supervivencia, y solo representan a los graduados rápidos de tres franjas de 8 minutos.
- Graduados lentos: marxol.db solo conoce las graduaciones dentro de las ventanas de 65 min descargadas.
- Muestra pequeña (50) y no aleatoria del universo; las tasas de C son órdenes de magnitud, no estimaciones.
- Velas de 1 h con huecos: una hora sin vela es una hora sin trades. High y low incluyen mechas de un solo trade,
  que inflan las caídas y las subidas 2×.
- Precio en unidades del quote (currency=token): sin la deriva SOL/USD; no comparable entre pools SOL y USDC.
- Historia de velas gratuita limitada: pedir velas con before_timestamp anterior a ~2026-03-14 (unos 209 días)
  devuelve HTTP 401. Los 4 pools más antiguos de la muestra quedan cortados ahí. Además, el script pagina
  como mucho 6 páginas por pool.
- Datos de GeckoTerminal no verificados on-chain (principio 1). La API está en beta y puede cambiar.
- El límite real fue ~10 llamadas/min (documentado), no 30.
"""


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("cmd", choices=["fetch", "report"])
    args = ap.parse_args()
    {"fetch": fetch, "report": report}[args.cmd](args)


if __name__ == "__main__":
    main()
