#!/usr/bin/env python3
"""Comprobación del criterio de réplica (validación 2) contra la robustez post
hoc de la validación 1. Solo lee marxol.db (sin red, sin cuota).

`marxol entry2 --tramo validacion --replica --json` aplica a la validación 1 el
código del veredicto de réplica. Sus tres poblaciones son los subconjuntos de
`robustez_posthoc.py`: primaria = (a) un token por creator, secundaria 1 = (b)
los 251, secundaria 2 = (d) sin mayhem. Tiene que reproducir exactamente
poblaciones, diferencias, intervalos bootstrap (misma semilla) y métricas de
filtro. Fisher y OR, salvo redondeo de coma flotante (1e-9 relativo): Python
usa lgamma y Rust suma logaritmos.

Reproducir (desde la raíz del repo):
    cargo build --release && python3 -B scripts/verificar_replica.py
"""
import argparse, json, math, subprocess, sys
import robustez_posthoc as rb

SIGS = ["H4'", "H5", "H6'", "H7'", "H8"]
fails = []


def same(label, x, y, exact=True):
    if x is None or y is None or (isinstance(x, float) and math.isnan(x)):
        ok = (x is None or (isinstance(x, float) and math.isnan(x))) and (
            y is None or (isinstance(y, float) and math.isnan(y)))
    elif exact:
        ok = x == y
    else:
        ok = math.isclose(x, y, rel_tol=1e-9, abs_tol=1e-12)
    if not ok:
        fails.append(f"{label}: rust {x!r} != python {y!r}")


def table(pop, k, out):
    a = b = c = d = 0
    for t in pop:
        s, o = t["sig"][k], out(t)
        if s is None or o is None:
            continue
        if s and o: a += 1
        elif s: b += 1
        elif o: c += 1
        else: d += 1
    return a, b, c, d


def diff(pop, k, out):
    a, b, c, d = table(pop, k, out)
    return None if a + b == 0 or c + d == 0 else a / (a + b) - c / (c + d)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--marxol", default="target/release/marxol")
    ap.add_argument("--db", default="marxol.db")
    args = ap.parse_args()
    rust = json.loads(subprocess.run([args.marxol, "--db", args.db, "entry2", "--tramo", "validacion", "--replica", "--json"],
                                     check=True, capture_output=True, text=True).stdout)
    toks, _ = rb.load_tokens(args.marxol, args.db)
    first = {}
    for t in sorted(toks, key=lambda t: (t["slot"], t["mint"])):
        first.setdefault(t["creator"], t)
    py_pops = [(list(first.values()), "simple"), (toks, "cluster"), ([t for t in toks if not t["mayhem"]], "cluster")]
    keys = ["primaria", "secundaria1", "secundaria2"]

    for (pop, _), rp in zip(py_pops, rust["populations"]):
        same(f"{rp['label']} mints", rp["mints"], [t["mint"] for t in pop])
    print("primaria:", rust["populations"][0]["tokens"], "tokens")
    same("n primaria", rust["populations"][0]["tokens"], 210)

    outcomes = {"pump-y-caída en 1 h": lambda t: t["pd"], "H10": lambda t: t["surv"]}
    for h in rust["hypotheses"]:
        oname = "pump-y-caída en 1 h" if h["outcome"].startswith("pump") else "H10"
        out = outcomes[oname]
        k = SIGS.index(h["name"].split(" ")[0] if oname != "H10" else h["name"].split(" × ")[1])
        for key, (pop, kind) in zip(keys, py_pops):
            r = h[key]
            a, b, c, d = table(pop, SIGS[k], out)
            lab = f"{h['name']} / {key}"
            same(lab + " tabla", [r["table"][x] for x in "abcd"], [a, b, c, d])
            same(lab + " dif", r["diff"], diff(pop, SIGS[k], out))
            same(lab + " Fisher", r["fisher_p"], rb.fisher(a, b, c, d), exact=False)
            for x, y, nm in zip(r["or"], rb.orci(a, b, c, d), ("OR", "OR lo", "OR hi")):
                same(f"{lab} {nm}", x, y, exact=False)
            lo, hi, drop = rb.ci(pop, kind, lambda rs: diff(rs, SIGS[k], out))
            same(lab + " boot", r["boot"], [lo, hi, drop])

    for f in rust["filters"]:
        pop, kind = py_pops[keys.index({"primaria (1 por creator)": "primaria", "secundaria 1 (todos)": "secundaria1",
                                        "secundaria 2 (sin mayhem)": "secundaria2"}[f["population"]])]
        lab = f"filtro {f['filter']} / {f['population']}"
        def rest(rs):
            v = [t["ret"][1] for t in rs if not t["mayhem"] and not t["sig"][f["filter"]] and t["ret"][1] is not None]
            return rb.med(v) if v else None
        same(lab + " resto (a)", f["rest_a"]["median"], rest(pop))
        same(lab + " resto (a) boot", f["rest_a"]["ci"], list(rb.ci(pop, kind, rest)))
        nm = [t for t in pop if not t["mayhem"]]
        for bl, bp, bad in (("malo principal = pump-y-caída", pop, lambda t: t["pd"]),
                            ("malo secundario (a) = ret +30 < 0, sin mayhem", [t for t in nm if t["ret"][1] is not None],
                             lambda t: t["ret"][1] < 0)):
            avoid = [t for t in bp if t["sig"][f["filter"]]]
            malos = [t for t in bp if bad(t)]
            buenos = len(bp) - len(malos)
            am = sum(bad(t) for t in avoid)
            m = f[bl]
            same(lab + " " + bl, [m["evaluable"], m["avoided"], m["kept"]], [len(bp), len(avoid), len(bp) - len(avoid)])
            same(lab + " precisión", m["precision"], am / len(avoid) if avoid else float("nan"))
            same(lab + " cobertura", m["coverage"], am / len(malos) if malos else float("nan"))
            same(lab + " sacrificio", m["sacrifice"], (len(avoid) - am) / buenos if buenos else float("nan"))

    if fails:
        print(f"{len(fails)} DISCREPANCIAS:")
        print("\n".join(fails))
        sys.exit(1)
    print("OK: la réplica sobre la validación 1 reproduce la robustez post hoc "
          f"({len(rust['hypotheses'])} hipótesis × 3 poblaciones, {len(rust['filters'])} filtros)")


if __name__ == "__main__":
    main()
