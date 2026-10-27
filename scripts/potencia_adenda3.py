"""Potencia y efecto mínimo detectable para los mínimos de la adenda 3.

Post hoc, sin red. Lee el volcado de `marxol banco-fase-c --volcado DIR`
(DIR/tokens.tsv, solo marxol.db, V1–V3). Igual que potencia_adenda2_r.py: los dos
grupos se remuestrean de los retornos (a) a +30 min de V1–V3 AGREGADOS, sin
separar por D2, R1 ni R2 (ninguna de las tres se cruza aquí con retornos), y el
grupo que la regla marca se desplaza −d puntos (aditivo; aproximación).
Éxito = criterios 2 (MW unilateral, alfa de cada regla), 3 (≥ 10 pts) y 5 (IC
bootstrap 95 % > 0); el 4 no se simula.

Reglas (prevalencias por conteo, sin retornos):
- D2 (réplica, bloque A): alfa 0,01; descarta 37,8 % (V3, 28/74, banco-d2).
- R1 (bloque B): alfa 0,005; prevalencia 64 % (resultados/fase3_cprima.txt).
- R2 (bloque B): alfa 0,005; prevalencia 44 % (aprox. a +5 min); n/e los no SOL,
  29 % en marxol_vivo.db (resultados/cobertura_adenda3.txt): n_R2 = 0,71 · n.
`n` = tokens en la población de análisis (con T_entry2 y retorno evaluable).
MDE80 = menor d de la rejilla con potencia ≥ 0,80.
Con --efectivo, en vez de la rejilla, los n efectivos de los mínimos en tokens
evaluables (con T_entry2, ventanas completas y quote = SOL), §3 de la adenda 3:
A ≥ 900 y B ≥ 1 800 evaluables en SOL; la población principal de D2 y R1 incluye
además los evaluables en otro quote (SOL = 65,9 % de los tokens contados en A),
y la sensibilidad sin recuperación conserva el 44 %.
Uso: python3 -B scripts/potencia_adenda3.py DIR [--efectivo]
"""
import random
import sys

import potencia_adenda2 as p

p.SIMS = 200
p.BOOT = 200
SEED = 20261027
D_GRID = [0.10, 0.11, 0.12, 0.13, 0.14, 0.15, 0.175, 0.20, 0.25]
N_GRID = [100, 200, 300, 450, 600, 750, 900, 1200, 1500]
SOL = 0.659
SIN_REC = 0.44
EFECTIVO = [
    ("D2 (A), solo SOL = mínimo", 0.01, 0.378, 900),
    ("D2 (A), principal con otros quotes", 0.01, 0.378, round(900 / SOL)),
    ("D2 (A), principal sin recuperación", 0.01, 0.378, round(SIN_REC * 900 / SOL)),
    ("R1 (B), solo SOL = mínimo", 0.005, 0.64, 1800),
    ("R1 (B), principal con otros quotes", 0.005, 0.64, round(1800 / SOL)),
    ("R1 (B), principal sin recuperación", 0.005, 0.64, round(SIN_REC * 1800 / SOL)),
    ("R2 (B), solo SOL = principal", 0.005, 0.44, 1800),
    ("R2 (B), sin recuperación", 0.005, 0.44, round(SIN_REC * 1800)),
]
REGLAS = [("D2 (A)", 0.01, 0.378, 1.0), ("R1 (B)", 0.005, 0.64, 1.0), ("R2 (B)", 0.005, 0.44, 0.71)]


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
    print("POTENCIA Y MDE PARA LOS MÍNIMOS DE LA ADENDA 3. Post hoc, sin red; D2/R1/R2 no se cruzan con retornos.")
    print(f"retornos (a) +30 min de V1–V3 agregados: n {len(pool)}; {p.SIMS} simulaciones × {p.BOOT} réplicas; semilla {SEED};"
          " criterios 2, 3 y 5 (el 4 no se simula)")
    if "--efectivo" in sys.argv:
        print("\nN EFECTIVO DE LOS MÍNIMOS (tokens evaluables; --efectivo)")
        for nombre, alfa, prev, n in EFECTIVO:
            p.ALFA = alfa
            n_v = round(n * prev)
            rng = random.Random(SEED)
            cells = [f"{100 * d:.1f}:{potencia(pool, d, n - n_v, n_v, rng):.2f}" for d in (0.10, 0.11, 0.12)]
            print(f"  {nombre:38} n {n:5} ({n - n_v} / {n_v}): potencia a d = [{' '.join(cells)}]")
            sys.stdout.flush()
        return
    for nombre, alfa, prev, frac in REGLAS:
        p.ALFA = alfa
        print(f"\n{nombre}: alfa {alfa}, marcados {100 * prev:.1f} %, fracción evaluable {frac}")
        for n in N_GRID:
            m = round(n * frac)
            n_v = round(m * prev)
            n_f = m - n_v
            if min(n_v, n_f) < 20:
                print(f"  n {n:5} ({n_f}/{n_v}): criterio 1 no se cumple")
                continue
            rng = random.Random(SEED)
            cells, mde = [], None
            for d in D_GRID:
                pw = potencia(pool, d, n_f, n_v, rng)
                cells.append(f"{100 * d:.1f}:{pw:.2f}")
                if pw >= 0.80:
                    mde = d
                    break
            print(f"  n {n:5} ({n_f} no marcados / {n_v} marcados): MDE80 "
                  f"{'> 25' if mde is None else f'{100 * mde:.1f}'} pts   [{' '.join(cells)}]")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
