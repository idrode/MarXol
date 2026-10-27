"""Potencia del criterio de la adenda 1/2 (borrador) para R1 y R2 en el bloque B.

Post hoc y exploratorio. Sin red. R1 y R2 NO se cruzan con ningún retorno: los
dos grupos se simulan remuestreando los retornos (a) a +30 min de V1–V3
agregados (sin separar por R), y el grupo con R = verdadero se desplaza -d
puntos (desplazamiento aditivo; aproximación). Las n por grupo salen del n
esperado del bloque B y de prevalencias por conteo (resultados/fase3_cprima.txt
y fase3_explora.txt).
Éxito = MW unilateral p < 0.005, diferencia observada >= 10 pts e IC
bootstrap 95 % > 0 (criterios 2, 3 y 5; el 4 no se simula).
Uso: python3 -B scripts/potencia_adenda2_r.py DIR  (DIR/tokens.tsv de banco-fase-c --volcado)
"""
import random
import sys

import potencia_adenda2 as p

p.ALFA = 0.01 / 2
SEED = 20261023


def potencia(pool, d, n_f, n_v, rng):
    ok = 0
    for _ in range(p.SIMS):
        f = rng.choices(pool, k=n_f)
        v = [x - d for x in rng.choices(pool, k=n_v)]
        ok += p.exito(f, v, rng)
    return ok / p.SIMS


def main():
    rows = [l.rstrip("\n").split("\t") for l in open(sys.argv[1] + "/tokens.tsv")][1:]
    pool = [float(r[10]) for r in rows]
    print("POTENCIA DE R1 Y R2 (adenda 2, borrador, k = 2). Post hoc, sin red; R1/R2 no se cruzan con retornos.")
    print(f"retornos agregados de V1–V3: n {len(pool)}; criterio simulado: MW unilateral p < {p.ALFA:.4f}, diferencia >= 10 pts, IC bootstrap 95 % > 0;"
          f" {p.SIMS} simulaciones, {p.BOOT} réplicas, semilla {SEED}")
    escenarios = [
        ("R1 (prev. 64 %), n 960", 960, 0.64),
        ("R1 (prev. 64 %), n 480", 480, 0.64),
        ("R2 (prev. 44 %, n/e no SOL 23 %), n 740", 740, 0.44),
        ("R2 (prev. 44 %), n 370", 370, 0.44),
    ]
    for nombre, n, prev in escenarios:
        n_v = round(n * prev)
        n_f = n - n_v
        rng = random.Random(SEED)
        cells = [f"d {100 * d:+.1f}: {potencia(pool, d, n_f, n_v, rng):.2f}" for d in (0.05, 0.083, 0.095, 0.10, 0.125, 0.15)]
        print(f"{nombre} (falso {n_f}, verdadero {n_v}): " + " · ".join(cells))


if __name__ == "__main__":
    main()
