#!/usr/bin/env python3
"""Comprobación del código de la validación 3 sobre la validación 2 (sin
veredicto). Solo lee marxol.db (sin red, sin cuota).

`marxol entry2 --tramo validacion2 --d2-prueba --json` aplica a V2 la
clasificación D2 y el retorno (a) a +30 min del código de la validación 3.
Con la población de `cribado_posthoc.py` (V2 primaria, sin mayhem) tiene que
dar exactamente los mismos tokens, el mismo número de descartados y la misma
diferencia de medianas (pasan − descartados).

Reproducir (desde la raíz del repo):
    cargo build --release && python3 -B scripts/verificar_d2_v2.py
"""
import argparse, json, subprocess, sys
import cribado_posthoc as cp


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--marxol", default="target/release/marxol")
    ap.add_argument("--db", default="marxol.db")
    args = ap.parse_args()
    rust = json.loads(subprocess.run([args.marxol, "--db", args.db, "entry2", "--tramo", "validacion2", "--d2-prueba", "--json"],
                                     check=True, capture_output=True, text=True).stdout)
    toks, _ = cp.load(args.marxol, args.db)
    v2 = sorted([t for t in toks.values() if t["tramo"] == "V2" and t["e"] is not None], key=lambda t: (t["slot"], t["mint"]))
    first = {}
    for t in v2:
        first.setdefault(t["creator"], t)
    pop = [t for t in first.values() if not t["mayhem"]]
    obs = [t for t in pop if t["ret"][1800] is not None]
    diff = cp.med([t["ret"][1800] for t in obs if not t["D2"]]) - cp.med([t["ret"][1800] for t in obs if t["D2"]])
    want = {"n": len(pop), "discarded": sum(t["D2"] for t in pop), "comparison": len(obs), "median_diff": diff,
            "mints": [t["mint"] for t in pop]}
    r = rust[0]
    fails = [f"{k}: rust {r[k]!r} != python {v!r}" for k, v in want.items() if r[k] != v]
    if fails:
        print("\n".join(fails))
        sys.exit(1)
    print(f"OK: D2 sobre V2 primaria sin mayhem: n {want['n']}, descartados {want['discarded']}, "
          f"comparación {want['comparison']}, diferencia de medianas {100 * diff:+.4f} pts (idéntica)")
    print(f"regla de la validación 3 sobre V2 (mayhem fuera primero): n {rust[1]['n']}, descartados {rust[1]['discarded']}, "
          f"misma población que la de cribado: {rust[1]['mints'] == want['mints']}")


if __name__ == "__main__":
    main()
