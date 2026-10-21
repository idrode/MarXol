#!/usr/bin/env python3
"""Exploración de señales de "segunda vida" en los pools graduados de un día — POST HOC,
SIN PRE-REGISTRO, SIN VEREDICTO. Sin red: solo lee madurez.db (velas horarias USD de
madurez_vida.py --download).

    python3 -B scripts/madurez_explora.py [--day 2026-08-20] > resultados/madurez_explora.txt

Definiciones (fijadas antes de calcular ningún resultado, 2026-10-21):
- Rejilla: una vela por hora desde la primera vela del pool hasta el final del rango
  descargado (migración + 31 d). Horas sin vela: cierre anterior y volumen 0.
- Observación: la primera hora de cada día UTC (vela con ts = 00:00 UTC) con edad
  ts − migración en [3 d, 14 d]. Datos de la señal: hasta esa vela incluida.
- Universo: volumen de las 24 velas hasta la observación incluida ≥ 5 000 USD
  (sensibilidad: ≥ 1 000). Sin liquidez.
- Evento: máximo cierre de las 168 velas siguientes ≥ 2× el cierre de la observación.
  Solo observaciones con 168 velas posteriores dentro del rango descargado.
- RSI(14) de Wilder sobre los cierres de la rejilla (media simple de los 14 primeros
  cambios y suavizado (n − 1)/n). Pérdida media 0: 100 (50 si la ganancia también es 0).
- S1: RSI cruzó al alza 30 en las últimas 12 h: algún k en las 12 velas hasta la
  observación con RSI[k−1] < 30 ≤ RSI[k].
- S2: RSI ≥ 60 en la observación.
- S3: volumen de las 6 velas hasta la observación ≥ 3× la mediana del volumen horario de
  las velas anteriores a esas 6 (máx. 168, desde la primera vela del pool), y volumen
  de las 6 velas > 0 (confirmado por Roi: evita que una mediana 0 la haga trivial).
- S4: S3 y cierre de la observación > cierre de 6 velas antes.
- S5: S1 y S3.
- Bootstrap: 2 000 réplicas, semilla 20260930, remuestreo de tokens con reemplazo
  (todas sus observaciones). Lift = tasa con señal / tasa base del universo. Réplicas
  con 0 observaciones con señal o tasa base 0: descartadas y contadas.
- "Sin potencia": < 20 observaciones con señal o < 5 eventos.
"""
import argparse, random, statistics
import madurez_fase0b as f0

H = 3600
D = 24 * H
SEED = 20260930
REPS = 2000
UNIVERSES = [5_000, 1_000]
SIGNALS = ["S1", "S2", "S3", "S4", "S5"]


def rsi_wilder(closes, n=14):
    out = [None] * len(closes)
    if len(closes) <= n:
        return out
    ch = [closes[i] - closes[i - 1] for i in range(1, len(closes))]
    g = sum(max(c, 0) for c in ch[:n]) / n
    l = sum(max(-c, 0) for c in ch[:n]) / n

    def val(g, l):
        return 50.0 if g == 0 and l == 0 else 100.0 if l == 0 else 100 - 100 / (1 + g / l)
    out[n] = val(g, l)
    for i in range(n + 1, len(closes)):
        c = ch[i - 1]
        g = (g * (n - 1) + max(c, 0)) / n
        l = (l * (n - 1) + max(-c, 0)) / n
        out[i] = val(g, l)
    return out


def observations(db, day):
    migs = db.execute("SELECT pool, timestamp FROM chain_migrations WHERE day = ? ORDER BY slot, mint", (day,)).fetchall()
    obs, skipped = [], {"sin velas": 0, "sin 7 d posteriores": 0}
    for pool, t_m in migs:
        done = db.execute("SELECT complete FROM gt_ohlcv_done WHERE pool = ? AND timeframe = 'hour' AND currency = 'usd'",
                          (pool,)).fetchone()
        c = db.execute("SELECT ts, close, volume FROM gt_ohlcv WHERE pool = ? AND timeframe = 'hour' AND currency = 'usd' "
                       "ORDER BY ts", (pool,)).fetchall()
        if not done or done[0] != 1 or not c:
            skipped["sin velas"] += 1
            continue
        before = t_m + 31 * D + H                      # before_timestamp de la descarga
        t0, t_end = c[0][0], (before - 1) // H * H      # última hora que pudo descargarse
        by = {ts: (cl, v) for ts, cl, v in c}
        closes, vols, last = [], [], None
        for ts in range(t0, t_end + H, H):
            if ts in by:
                last, v = by[ts]
            else:
                v = 0.0
            closes.append(last)
            vols.append(v)
        rsi = rsi_wilder(closes)
        first_day = (t_m + 3 * D + D - 1) // D * D
        for h in range(first_day, t_m + 14 * D + 1, D):
            i = (h - t0) // H
            if i < 0:
                continue
            if i + 168 >= len(closes):
                skipped["sin 7 d posteriores"] += 1
                continue
            vol24 = sum(vols[max(0, i - 23):i + 1])
            fut = max(closes[i + 1:i + 169])
            cross = any(rsi[k - 1] is not None and rsi[k] is not None and rsi[k - 1] < 30 <= rsi[k]
                        for k in range(max(1, i - 11), i + 1))
            vol6 = sum(vols[max(0, i - 5):i + 1])
            prev = vols[max(0, i - 5 - 168):max(0, i - 5)]
            s3 = bool(prev) and vol6 > 0 and vol6 >= 3 * statistics.median(prev)
            s4 = s3 and i >= 6 and closes[i] > closes[i - 6]
            obs.append({"pool": pool, "age_d": (h - t_m) / D, "vol24": vol24, "event": fut >= 2 * closes[i],
                        "S1": cross, "S2": rsi[i] is not None and rsi[i] >= 60, "S3": s3, "S4": s4, "S5": cross and s3,
                        "path": closes[i:i + 170]})   # cierres desde la observación (madurez_cota.py)
    return obs, skipped, len(migs)


def rate(o):
    return sum(x["event"] for x in o) / len(o) if o else float("nan")


def boot(univ):
    by = {}
    for x in univ:
        by.setdefault(x["pool"], []).append(x)
    pools = sorted(by)
    rng = random.Random(SEED)
    base, lifts, drop = [], {s: [] for s in SIGNALS}, {s: 0 for s in SIGNALS}
    for _ in range(REPS):
        sample = [x for p in (rng.choice(pools) for _ in pools) for x in by[p]]
        b = rate(sample)
        base.append(b)
        for s in SIGNALS:
            sub = [x for x in sample if x[s]]
            if not sub or b == 0:
                drop[s] += 1
            else:
                lifts[s].append(rate(sub) / b)
    return base, lifts, drop


def ci(xs):
    if not xs:
        return "n/e"
    xs = sorted(xs)
    return f"[{xs[int(.025 * (len(xs) - 1))]:.2f}, {xs[int(.975 * (len(xs) - 1))]:.2f}]"


def ci_pct(xs):
    xs = sorted(xs)
    return f"[{xs[int(.025 * (len(xs) - 1))]:.1%}, {xs[int(.975 * (len(xs) - 1))]:.1%}]"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--day", default="2026-08-20")
    ap.add_argument("--madurez-db", default="madurez.db")
    args = ap.parse_args()
    db = f0.ro(args.madurez_db)
    obs, skipped, n_migs = observations(db, args.day)
    L = []
    p = L.append
    p('Señales de "segunda vida" — EXPLORACIÓN POST HOC, SIN PRE-REGISTRO, SIN VEREDICTO')
    p(f"Generado con: python3 -B scripts/madurez_explora.py --day {args.day}  (solo lee madurez.db, sin red)")
    p("Definiciones en la cabecera del script. Evento = máximo cierre en 7 d ≥ 2× el cierre de la observación.")
    p("")
    p(f"== 0. Datos: {n_migs} pools del {args.day}; sin velas {skipped['sin velas']}; "
      f"observaciones (día UTC, edad 3–14 d) {len(obs)} de {len({x['pool'] for x in obs})} pools; "
      f"descartadas por no tener 7 d posteriores {skipped['sin 7 d posteriores']}")
    p("")
    combos = 0
    for thr in UNIVERSES:
        univ = [x for x in obs if x["vol24"] >= thr]
        tag = "principal" if thr == UNIVERSES[0] else "sensibilidad"
        p(f"== Universo {tag}: volumen 24 h ≥ {thr:,} USD")
        if not univ:
            p("  vacío")
            p("")
            continue
        base, lifts, drop = boot(univ)
        ev = sum(x["event"] for x in univ)
        p(f"  observaciones {len(univ)}, tokens {len({x['pool'] for x in univ})}, eventos {ev}")
        p(f"  tasa base {rate(univ):.1%}  IC95 bootstrap por token {ci_pct(base)}")
        p(f"  edad de las observaciones: " + ", ".join(
            f"{d} d {sum(1 for x in univ if int(x["age_d"]) == d)}" for d in range(3, 14)))
        p("")
        p("  señal  obs  tokens  eventos  tasa     lift   IC95 lift (boot)   réplicas descart.  potencia")
        for s in SIGNALS:
            combos += 1
            sub = [x for x in univ if x[s]]
            e = sum(x["event"] for x in sub)
            lift = rate(sub) / rate(univ) if sub and rate(univ) else float("nan")
            pot = "sin potencia" if len(sub) < 20 or e < 5 else "ok"
            tr = f"{rate(sub):>6.1%}" if sub else "   n/e"
            p(f"  {s:<5} {len(sub):>4}  {len({x['pool'] for x in sub}):>6}  {e:>7}  {tr}  {lift:>5.2f}  "
              f"{ci(lifts[s]):<17}  {drop[s]:>17}  {pot}")
        p("")
    p(f"== Comparaciones: {len(SIGNALS)} señales × {len(UNIVERSES)} universos = {combos} combinaciones "
      f"(los dos universos se solapan: el de 5 000 está contenido en el de 1 000)")
    p("  Con 10 comparaciones, un IC95 que excluye 1 en una sola de ellas es compatible con el azar.")
    p("")
    p("== Límites")
    p("  - Un solo día de graduaciones (2026-08-20) y una sola fuente de velas (GeckoTerminal, sin verificar en cadena).")
    p("  - Post hoc, sin pre-registro: el universo y las señales se eligieron después de ver madurez_fase0b y madurez_vida.")
    p("  - El universo está tras el corte de las 72 h (edad ≥ 3 d): solo entran los que sobrevivieron a la caída inicial,")
    p("    y es un subconjunto pequeño y seleccionado de los 1 068.")
    p("  - Las observaciones del mismo token están correlacionadas (días consecutivos, ventanas de 7 d solapadas);")
    p("    el bootstrap por token lo tiene en cuenta en el IC, no en los recuentos.")
    p("  - El evento usa el máximo cierre horario: no es un retorno alcanzable. Sin comisiones, deslizamiento ni reglas de salida.")
    p("  - Rejilla rellenada: en horas sin vela el RSI ve cambio 0; en pools poco activos el RSI tiende a 50 y S1/S2 pierden sentido.")
    p("  - Precio USD de GeckoTerminal: en pools con quote SOL incluye la variación del SOL.")
    print("\n".join(L))


if __name__ == "__main__":
    main()
