"""Potencia del criterio de la adenda 2 (borrador) para C1 en el bloque B.

Post hoc y exploratorio. Sin red. Lee el volcado de
`marxol banco-fase-c --volcado DIR` (DIR/tokens.tsv, solo marxol.db).

Simula el bloque B remuestreando con reemplazo los retornos (a) a +30 min de
V1–V3: n_sin del grupo sin C1 y n_con del grupo con C1, este desplazado para
que la diferencia de medianas verdadera sea `d` puntos (desplazamiento
aditivo; aproximación). Cuenta como éxito cumplir a la vez:
  MW unilateral p < alfa, diferencia observada >= 10 pts y IC bootstrap
  (percentil, 95 %) de la diferencia > 0.
El criterio 4 (sin el token de mayor peso) no se simula.
Uso: python3 -B scripts/potencia_adenda2.py DIR
"""
import math
import random
import sys

ALFA = 0.01 / 3
SIMS = 300
BOOT = 300
SEED = 20261022


def median(v):
    s = sorted(v)
    n = len(s)
    return s[n // 2] if n % 2 else (s[n // 2 - 1] + s[n // 2]) / 2


def mw_greater(x, y):
    """p unilateral (x > y), aproximación normal con corrección de empates."""
    allv = sorted([(v, 0) for v in x] + [(v, 1) for v in y])
    ranks = [0.0] * len(allv)
    i, tie = 0, 0.0
    while i < len(allv):
        j = i
        while j + 1 < len(allv) and allv[j + 1][0] == allv[i][0]:
            j += 1
        r = (i + j) / 2 + 1
        for k in range(i, j + 1):
            ranks[k] = r
        t = j - i + 1
        tie += t ** 3 - t
        i = j + 1
    n1, n2 = len(x), len(y)
    r1 = sum(r for r, (_, g) in zip(ranks, allv) if g == 0)
    u = r1 - n1 * (n1 + 1) / 2
    n = n1 + n2
    sd = math.sqrt(n1 * n2 / 12 * ((n + 1) - tie / (n * (n - 1))))
    z = (u - n1 * n2 / 2 - 0.5) / sd
    return 0.5 * math.erfc(z / math.sqrt(2))


def exito(sin, con, rng):
    d = median(sin) - median(con)
    if d < 0.10 - 1e-9 or mw_greater(sin, con) >= ALFA:
        return False
    bs = sorted(
        median(rng.choices(sin, k=len(sin))) - median(rng.choices(con, k=len(con))) for _ in range(BOOT)
    )
    return bs[int(0.025 * BOOT)] > 0


def potencia(sin0, con0, d, n_sin, n_con, rng):
    shift = (median(sin0) - median(con0)) - d
    con = [v + shift for v in con0]
    return sum(exito(rng.choices(sin0, k=n_sin), rng.choices(con, k=n_con), rng) for _ in range(SIMS)) / SIMS


def main():
    rows = [l.rstrip("\n").split("\t") for l in open(sys.argv[1] + "/tokens.tsv")][1:]
    sin0 = [float(r[10]) for r in rows if r[4] == "0"]
    con0 = [float(r[10]) for r in rows if r[4] == "1"]
    obs = median(sin0) - median(con0)
    print("POTENCIA DEL CRITERIO DE LA ADENDA 2 (BORRADOR), C1. Post hoc, sin red, solo marxol.db.")
    print(f"marxol.db: n sin C1 {len(sin0)}, con C1 {len(con0)}; diferencia observada {100 * obs:+.1f} pts; mitad {100 * obs / 2:+.1f} pts")
    print(f"criterio simulado: MW unilateral p < {ALFA:.4f}, diferencia >= 10 pts, IC bootstrap 95 % > 0; {SIMS} simulaciones, {BOOT} réplicas bootstrap, semilla {SEED}")
    prev = len(con0) / (len(con0) + len(sin0))
    for n in (240, 480, 720, 960):
        n_con = round(n * prev)
        n_sin = n - n_con
        rng = random.Random(SEED)
        cells = []
        for d in (obs / 2, 0.10, 0.125, 0.15, obs):
            cells.append(f"d {100 * d:+.1f}: {potencia(sin0, con0, d, n_sin, n_con, rng):.2f}")
        print(f"n {n:4} (sin {n_sin}, con {n_con}): " + " · ".join(cells))


if __name__ == "__main__":
    main()
