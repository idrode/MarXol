#!/usr/bin/env python3
"""Robustez POST HOC de la validación congelada (CLAUDE.md, secciones 8 y 10).
EXPLORATORIA, SIN VEREDICTO: no cambia ningún veredicto congelado ni ninguna
definición previa. Solo lee marxol.db (sin red, sin cuota).

Reproducir (desde la raíz del repo):
    cargo build --release && python3 scripts/robustez_posthoc.py

Definiciones (fijadas 2026-09-30 antes de calcular; copia de CLAUDE.md sección 10):
- Población base: los 251 tokens de validación con T_entry2 (salida de
  `marxol entry2 --json`, la de las tablas congeladas).
- Un token por creator: menor slot de creación dentro de los 251; empate por
  orden de ejecución y, si no está disponible, mint lexicográfico.
- Fábrica: creator con >= 5 tokens entre las 377 creaciones indexadas.
- Supervivencia (H10): >= 1 trade a más de 10 min de T_entry2; n/e si la
  curva se completa antes de T_entry2 + 10 min.
- Retorno neto: el de la validación congelada; mediana a +10/+30/+60 min,
  solo +30 con intervalo. Métricas de retorno solo sin mayhem.
- "Malo" principal = pump-y-caída (definición congelada); secundario =
  retorno neto a +30 min < 0. "Bueno" = complementario.
- Filtros "evitar": A = cualquiera de H5/H6'/H7'/H8; B = >= 2 de las cuatro;
  C = H5 o H7'; además cada señal por separado, con H4' aparte. Un H6' n/e
  cuenta como sin señal en A/B/C (no ocurre en los 251).
- Subconjuntos 2x2: (a) un token por creator; (b) 251; (c) 251 sin fábricas;
  (d) 251 sin mayhem.
- Bootstrap: 2000 réplicas, semilla 20260930, percentiles 2.5-97.5; simple en
  (a), por clúster de creator (todos sus tokens) en (b), (c), (d) y en el
  retorno del resto. Réplicas con un grupo vacío se descartan y se cuentan.
"""
import argparse, json, sqlite3, subprocess, collections, math, random

SEED, B = 20260930, 2000


def load_entry2(marxol, db):
    """Filas por token de `marxol entry2 --json` (validación, 251 tokens)."""
    out = subprocess.run([marxol, "--db", db, "entry2", "--json"], check=True, capture_output=True, text=True).stdout
    return json.loads(out)


SIGS = {
    "H4'": lambda e: e["dev_sold"],
    "H5": lambda e: e["same_slot_wallets"] >= 1,
    "H6'": lambda e: None if e["fill_pct"] is None else e["fill_pct"] >= 13.65,
    "H7'": lambda e: e["pairs"] >= 1,
    "H8": lambda e: e["creator_slot_buy_pct"] >= 15.0,
}


def load_tokens(marxol, db_path):
    """Los 251 tokens con señales, filtros A/B/C, supervivencia y datos base."""
    db = sqlite3.connect(db_path)
    q = lambda s, *a: db.execute(s, a).fetchall()
    cr = {m: (c, s, bool(mh), t0) for m, c, s, mh, t0 in q("select mint,creator,slot,is_mayhem_mode,timestamp from creations")}
    comp = dict(q("select mint,timestamp from completions"))
    allc = collections.Counter(v[0] for v in cr.values())
    fab = {c for c, k in allc.items() if k >= 5}
    toks = []
    for r in load_entry2(marxol, db_path):
        m, e = r["mint"], r["entry2"]
        c, slot, mh, t0 = cr[m]
        et = e["entry_ts"]
        if m in comp and comp[m] <= et + 600:
            surv = None
        else:
            surv = q("select count(*) from price_points where mint=? and kind='trade' and timestamp>?", m, et + 600)[0][0] > 0
        s = {k: f(e) for k, f in SIGS.items()}
        core = [bool(s[k]) for k in ("H5", "H6'", "H7'", "H8")]  # n/e cuenta como sin señal en los filtros
        s["A"] = any(core)
        s["B"] = sum(core) >= 2
        s["C"] = bool(s["H5"]) or bool(s["H7'"])
        toks.append(dict(mint=m, creator=c, slot=slot, t0=t0, mayhem=mh, fab=c in fab, pd=r["pump_dump"],
                         ret=r["returns"], surv=surv, sig=s, e=e, completed_at=comp.get(m)))
    return toks, fab


def lnf(n): return math.lgamma(n + 1)
def fisher(a, b, c, dd):
    r1, r2, c1 = a + b, c + dd, a + c; n = r1 + r2
    lp = lambda x: lnf(r1)+lnf(r2)+lnf(c1)+lnf(n-c1)-lnf(n)-lnf(x)-lnf(r1-x)-lnf(c1-x)-lnf(r2-c1+x)
    obs = lp(a)
    return min(1.0, sum(math.exp(lp(x)) for x in range(max(0, c1-r2), min(r1, c1)+1) if lp(x) <= obs + 1e-7))
def orci(a, b, c, dd):
    k = 0.5 if 0 in (a, b, c, dd) else 0
    a, b, c, dd = a+k, b+k, c+k, dd+k
    o = a*dd/(b*c); se = math.sqrt(1/a+1/b+1/c+1/dd)
    return o, math.exp(math.log(o)-1.96*se), math.exp(math.log(o)+1.96*se)
def med(v):
    v = sorted(v); n = len(v)
    return float("nan") if n == 0 else (v[n//2] if n % 2 else (v[n//2-1]+v[n//2])/2)
def rate(v): return float("nan") if not v else sum(v)/len(v)

def resamples(pop, kind, seed=SEED, B=B):
    rng = random.Random(seed)
    if kind == "simple":
        for _ in range(B):
            yield [pop[rng.randrange(len(pop))] for _ in pop]
    else:
        cl = collections.defaultdict(list)
        for t in pop: cl[t["creator"]].append(t)
        keys = sorted(cl)
        for _ in range(B):
            out = []
            for _ in keys: out += cl[keys[rng.randrange(len(keys))]]
            yield out

def ci(pop, kind, stat):
    vals, drop = [], 0
    for rs in resamples(pop, kind):
        v = stat(rs)
        if v is None or math.isnan(v): drop += 1
        else: vals.append(v)
    vals.sort()
    if not vals: return (float("nan"), float("nan"), drop)
    return vals[int(0.025*len(vals))], vals[min(len(vals)-1, int(0.975*len(vals)))], drop

def pct(x): return f"{100*x:+.1f}" if not math.isnan(x) else "  n/a"
def diff_pd(pop, k):
    g1 = [t["pd"] for t in pop if t["sig"][k] is True]; g0 = [t["pd"] for t in pop if t["sig"][k] is False]
    if not g1 or not g0: return None
    return rate(g1) - rate(g0)
def med30(pop, k, g):
    v = [t["ret"][1] for t in pop if not t["mayhem"] and t["sig"][k] is g and t["ret"][1] is not None]
    return med(v) if v else None

def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--marxol", default="target/release/marxol")
    ap.add_argument("--db", default="marxol.db")
    args = ap.parse_args()
    toks, FAB = load_tokens(args.marxol, args.db)
    # un token por creator: menor slot, luego mint lexicográfico (no hay empates)
    first = {}
    for t in sorted(toks, key=lambda t: (t["slot"], t["mint"])):
        first.setdefault(t["creator"], t)
    SUBSETS = {
        "(a) 1 por creator": (list(first.values()), "simple"),
        "(b) 251": (toks, "cluster"),
        "(c) sin fábricas": ([t for t in toks if not t["fab"]], "cluster"),
        "(d) sin mayhem": ([t for t in toks if not t["mayhem"]], "cluster"),
    }

    print("fábricas:", len(FAB), "creators;", sum(t["fab"] for t in toks), "tokens en 251;",
          "mayhem:", sum(t["mayhem"] for t in toks), "; H6' n/e:", sum(t["sig"]["H6'"] is None for t in toks),
          "; superv n/e:", sum(t["surv"] is None for t in toks))
    print("\n=== 1-2. TABLAS 2x2 POR SUBCONJUNTO (post hoc, sin veredicto) ===")
    for name, (pop, kind) in SUBSETS.items():
        print(f"\n## {name}: n={len(pop)} tokens, {len({t['creator'] for t in pop})} creators, bootstrap {kind}")
        for k in ("H4'", "H5", "H6'", "H7'", "H8"):
            g1 = [t for t in pop if t["sig"][k] is True]; g0 = [t for t in pop if t["sig"][k] is False]
            a = sum(t["pd"] for t in g1); b = len(g1)-a; c = sum(t["pd"] for t in g0); dd = len(g0)-c
            dif = rate([t["pd"] for t in g1]) - rate([t["pd"] for t in g0]) if g1 and g0 else float("nan")
            o, lo, hi = orci(a, b, c, dd)
            blo, bhi, drop = ci(pop, kind, lambda rs: diff_pd(rs, k))
            print(f"{k:4} n {len(g1)}/{len(g0)}  p&c {a}/{len(g1)} ({pct(rate([t['pd'] for t in g1]))}%) vs {c}/{len(g0)} ({pct(rate([t['pd'] for t in g0]))}%)"
                  f"  dif {pct(dif)} pts [boot {pct(blo)}, {pct(bhi)}; descartadas {drop}]  Fisher p={fisher(a,b,c,dd):.4f}  OR {o:.2f} ({lo:.2f}–{hi:.2f})")
            for lab, grp in (("señal", g1), ("sin señal", g0)):
                nm = [t for t in grp if not t["mayhem"]]
                sv = [t["surv"] for t in grp if t["surv"] is not None]; svn = [t["surv"] for t in nm if t["surv"] is not None]
                pdn = [t["pd"] for t in nm]
                rets = [[t["ret"][h] for t in nm if t["ret"][h] is not None] for h in range(3)]
                g = lab == "señal"
                rlo, rhi, rdrop = ci(pop, kind, lambda rs: med30(rs, k, g))
                print(f"      {lab:9} p&c {pct(rate([t['pd'] for t in grp]))}% (sin mayhem {pct(rate(pdn))}%, n={len(nm)})"
                      f"  superv {pct(rate(sv))}% (n={len(sv)}; sin mayhem {pct(rate(svn))}%, n={len(svn)})"
                      f"  ret sin mayhem +10 {pct(med(rets[0]))}  +30 {pct(med(rets[1]))} [{pct(rlo)}, {pct(rhi)}; desc {rdrop}]  +60 {pct(med(rets[2]))}  (n={len(rets[1])})")

    print("\n=== 3. MÉTRICAS DE FILTRO sobre los 251 (post hoc, sin veredicto) ===")
    nm = [t for t in toks if not t["mayhem"]]
    def filt(pop, k, bad):
        ev = [t for t in pop if t["sig"][k] is not None]
        avoid = [t for t in ev if t["sig"][k]]; keep = [t for t in ev if not t["sig"][k]]
        malos = [t for t in ev if bad(t)]; buenos = [t for t in ev if not bad(t)]
        prec = rate([bad(t) for t in avoid]); cov = sum(bad(t) for t in avoid)/len(malos) if malos else float("nan")
        sac = sum(not bad(t) for t in avoid)/len(buenos) if buenos else float("nan")
        return len(ev), len(avoid), len(keep), prec, cov, sac
    def rest30(rs, k):
        v = [t["ret"][1] for t in rs if not t["mayhem"] and t["sig"][k] is False and t["ret"][1] is not None]
        return med(v) if v else None
    for k in ("A", "B", "C", "H5", "H6'", "H7'", "H8", "H4'"):
        print(f"\n{k}")
        for lab, pop, bad in (("principal p&c, todos", toks, lambda t: t["pd"]),
                              ("principal p&c, sin mayhem", nm, lambda t: t["pd"]),
                              ("secundario ret30<0, sin mayhem", [t for t in nm if t["ret"][1] is not None], lambda t: t["ret"][1] < 0)):
            n, na, nk, p, c, s = filt(pop, k, bad)
            print(f"  {lab:32} evaluables {n}  evitados {na}  no evitados {nk}  precisión {pct(p)}%  cobertura {pct(c)}%  sacrificio {pct(s)}%")
        r = rest30(toks, k); lo, hi, drop = ci(toks, "cluster", lambda rs: rest30(rs, k))
        nrest = sum(1 for t in nm if t["sig"][k] is False and t["ret"][1] is not None)
        allr = med([t["ret"][1] for t in nm if t["ret"][1] is not None])
        print(f"  retorno +30 de los no evitados (sin mayhem, n={nrest}): {pct(r)}% [boot clúster {pct(lo)}, {pct(hi)}; desc {drop}]   (referencia sin filtrar: {pct(allr)}%)")


if __name__ == "__main__":
    main()
