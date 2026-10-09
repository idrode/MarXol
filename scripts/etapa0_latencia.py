#!/usr/bin/env python3
"""Etapa 0: margen real del aviso — POST HOC, SIN PRE-REGISTRO, SOLO DESCRIPTIVO.
Sin contrastes ni p-valores. No cambia ninguna definición ni veredicto. Solo
lee marxol.db (read-only, sin red, sin cuota). Reutiliza la carga de
`cribado_posthoc.py`.

Reproducir (desde la raíz del repo):
    cargo build --release && python3 -B scripts/etapa0_latencia.py > resultados/etapa0_latencia.txt

Pregunta: un grabador en vivo tiene que tener los datos de T_entry2 antes de
poder clasificar. ¿Cuántos segundos después de la creación cae T_entry2, y
cómo se comporta D2 si solo se pueden atender los tokens con T_entry2 <= X?

Definiciones:
- Retraso = T_entry2.timestamp - CreateEvent.timestamp (`entry_secs` de
  `marxol entry2 --check --json`, código congelado). Por definición >= 10 s.
- Poblaciones, en V1, V2 y V3 por separado, con la regla de la validación 3
  aplicada a los tres tramos: tokens con T_entry2; "sin mayhem" = se quitan
  los mayhem y se toma un token por creator (menor slot, empate por mint);
  "mayhem" = lo mismo dentro de los mayhem. En V3 "sin mayhem" es la población
  congelada (se comprueba contra `in_population`).
- D2 = >= 2 de H4'/H6'/H7' (definiciones congeladas). Si H6' es n/e y se
  cumple exactamente una de H4'/H7', el token es "no evaluable" (adenda 1 de
  la validación 3). En V3 se comprueba contra el `d2` congelado.
- Pump-y-caída (1 h) y retorno neto (a) a +30 min: los de cribado_posthoc,
  que se comprueban contra `entry2 --json` en V1.
- "Pasan" = D2 falso; "descartados" = D2 verdadero.
"""
import argparse, json, subprocess
import cribado_posthoc as cp

V3_FIRST, V3_LAST = 452_240_867, 452_242_654
CUTS = [15, 20, 30, 60]
XS = [20, 30, 60]


def pct(v, p):
    """Percentil por interpolación lineal (como numpy por defecto)."""
    v = sorted(v)
    if not v:
        return None
    k = (len(v) - 1) * p
    f = int(k)
    c = min(f + 1, len(v) - 1)
    return v[f] + (v[c] - v[f]) * (k - f)


def d2(sig):
    h4, h6, h7 = bool(sig["H4'"]), sig["H6'"], bool(sig["H7'"])
    if h6 is None:
        return None if h4 + h7 == 1 else h4 + h7 >= 2
    return h4 + h6 + h7 >= 2


def one_per_creator(ts):
    first = {}
    for t in sorted(ts, key=lambda t: (t["slot"], t["mint"])):
        first.setdefault(t["creator"], t)
    return list(first.values())


def fmt_ret(v):
    m = cp.med(v)
    return f"{cp.fr(m)} (n {len(v)})"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--marxol", default="target/release/marxol")
    ap.add_argument("--db", default="marxol.db")
    args = ap.parse_args()
    toks, _ = cp.load(args.marxol, args.db)
    n1, pd_bad, ret_bad = cp.controls(args.marxol, args.db, toks)

    # V3: entry2 --check --json (código congelado). cribado_posthoc no la carga.
    v3 = json.loads(subprocess.run([args.marxol, "--db", args.db, "entry2", "--check", "--json", "--tramo", "validacion3"],
                                   check=True, capture_output=True, text=True).stdout)
    for t in toks.values():
        if V3_FIRST <= t["slot"] <= V3_LAST:
            t["tramo"] = "V3"
    v3_frozen = {}
    for r in v3:
        t = toks[r["mint"]]
        assert t["tramo"] == "V3"
        t["e"] = r["entry2"]
        fill = t["e"]["fill_pct"]
        t["sig"] = {"H4'": t["e"]["dev_sold"], "H6'": None if fill is None else fill >= 13.65, "H7'": t["e"]["pairs"] >= 1}
        t["ret"] = {h: cp.ret_a(t, h) for h in (1800,)}
        v3_frozen[r["mint"]] = r

    for t in toks.values():
        if t["e"] is not None:
            t["d2"] = d2(t["sig"])
            t["secs"] = t["e"]["entry_secs"]
            assert t["secs"] == t["e"]["entry_ts"] - t["t0"]

    pops = {}
    for tr in ("V1", "V2", "V3"):
        withe = [t for t in toks.values() if t["tramo"] == tr and t["e"] is not None]
        created = sum(t["tramo"] == tr for t in toks.values())
        pops[tr] = (created, len(withe), one_per_creator([t for t in withe if not t["mayhem"]]),
                    one_per_creator([t for t in withe if t["mayhem"]]))

    # Controles de V3 contra lo congelado.
    v3_pop = {m for m, r in v3_frozen.items() if r["in_population"]}
    v3_pop_ok = v3_pop == {t["mint"] for t in pops["V3"][2]}
    v3_d2_bad = sum(toks[m]["d2"] != r["d2"] for m, r in v3_frozen.items())
    v3pass = [t["ret"][1800] for t in pops["V3"][2] if t["d2"] is False and t["ret"][1800] is not None]
    v3disc = [t["ret"][1800] for t in pops["V3"][2] if t["d2"] is True and t["ret"][1800] is not None]

    print("ETAPA 0: MARGEN REAL DEL AVISO — POST HOC, SIN PRE-REGISTRO, SOLO DESCRIPTIVO (sin p-valores)")
    print("controles:")
    print(f"  V1 (heredados de cribado_posthoc): p&c distinto de entry2 --json {pd_bad}/{n1}; retornos distintos {ret_bad}")
    print(f"  V3: población sin mayhem igual a la congelada (in_population): {v3_pop_ok}; D2 distinto del congelado: {v3_d2_bad}/{len(v3_frozen)}")
    print(f"  V3: retorno (a) +30 min, mediana pasan {cp.fr(cp.med(v3pass))} (n {len(v3pass)}), descartados "
          f"{cp.fr(cp.med(v3disc))} (n {len(v3disc)})  [resultados/validacion3_d2.txt: -5.6 % (46), -24.7 % (28)]")
    for tr, (created, ne, nm, mh) in pops.items():
        print(f"  {tr}: creaciones {created}, con T_entry2 {ne}; sin mayhem (1 por creator) {len(nm)}, mayhem (1 por creator) {len(mh)}")

    print("\n" + "=" * 100)
    print("1. RETRASO T_entry2 − creación (s)")
    print(f"  {'población':28} {'n':>4} {'p25':>6} {'med':>6} {'p75':>6} {'p90':>6} {'máx':>5}  " +
          "  ".join(f"≤{c:>2} s" for c in CUTS))
    for tr, (_, _, nm, mh) in pops.items():
        for label, sub in (("sin mayhem", nm), ("mayhem", mh)):
            s = [t["secs"] for t in sub]
            if not s:
                print(f"  {tr + ' ' + label:28} {0:>4}")
                continue
            cuts = "  ".join(f"{cp.pc(sum(x <= c for x in s), len(s)):>6}" for c in CUTS)
            print(f"  {tr + ' ' + label:28} {len(s):>4} {pct(s, .25):6.1f} {pct(s, .5):6.1f} {pct(s, .75):6.1f} "
                  f"{pct(s, .9):6.1f} {max(s):5}  {cuts}")
    print("  (≤15 s: porcentaje con T_entry2 a 15 s o menos de la creación; T_entry2 >= 10 s por definición)")

    print("\n" + "=" * 100)
    print("2. D2 RESTRINGIDO A LOS TOKENS CON T_entry2 ≤ X")
    print("  Clasificación D2 congelada (se mide en T_entry2, no cambia con X). Retorno = mediana del retorno neto (a) a")
    print("  +30 min; n = tokens con retorno evaluable. n/e = D2 no evaluable (adenda 1).")
    for tr, (_, _, nm, mh) in pops.items():
        for label, sub in (("sin mayhem", nm), ("mayhem (retornos no fiables)", mh)):
            print(f"\n  [{tr} {label}] n = {len(sub)}, p&c = {sum(bool(t['pd']) for t in sub)}")
            for x in XS + [None]:
                s = sub if x is None else [t for t in sub if t["secs"] <= x]
                tag = "todos" if x is None else f"≤{x} s"
                disc = [t for t in s if t["d2"] is True]
                keep = [t for t in s if t["d2"] is False]
                ne = len(s) - len(disc) - len(keep)
                pd = sum(bool(t["pd"]) for t in s)
                cap = sum(bool(t["pd"]) for t in disc)
                rd = [t["ret"][1800] for t in disc if t["ret"][1800] is not None]
                rk = [t["ret"][1800] for t in keep if t["ret"][1800] is not None]
                print(f"    {tag:6} tokens {len(s):3}/{len(sub)} ({cp.pc(len(s), len(sub)):>5})  descartados {len(disc):3} "
                      f"({cp.pc(len(disc), len(s)):>5})  pasan {len(keep):3}" + (f"  n/e {ne}" if ne else "") +
                      f"  p&c capturados {cap}/{pd}  retorno +30: descartados {fmt_ret(rd)}, pasan {fmt_ret(rk)}")
    # Tokens fuera de ≤ X: qué se pierde.
    print("\n  Tokens con T_entry2 > 60 s (los que un aviso a 60 s no ve), sin mayhem:")
    for tr, (_, _, nm, _) in pops.items():
        late = [t for t in nm if t["secs"] > 60]
        print(f"    {tr}: {len(late)} tokens, D2 descartados {sum(t['d2'] is True for t in late)}, "
              f"p&c {sum(bool(t['pd']) for t in late)}")

    print("\n" + "=" * 100)
    print("LÍMITES")
    print("- Post hoc, sin pre-registro: no confirma ni descarta nada y no cambia ningún veredicto ni definición.")
    print("- V1, V2 y V3 se han mirado antes (V1 y V2 para elegir D2). Estas cifras describen el margen temporal, no")
    print("  estiman la utilidad de D2 en un tramo nuevo.")
    print("- 'T_entry2 ≤ X' mide cuándo existe el dato en la cadena (timestamp de bloque, resolución 1 s). Un grabador en")
    print("  vivo lo ve con la latencia del stream además; esa latencia se mide en la Etapa 1 y no está sumada aquí.")
    print("- T_entry2 >= 10 s y en un slot posterior al de creación por definición: ningún token puede clasificarse antes.")
    print("- Los tokens sin T_entry2 (no tienen trade en un slot posterior a >= 10 s dentro de 5 min) quedan fuera, igual que")
    print("  en las validaciones; un aviso en vivo los vería como 'sin entrada'.")
    print("- La misma regla de población (mayhem fuera primero, un token por creator) en los tres tramos: en V1 y V2 no es")
    print("  exactamente la primaria de la validación 2 (allí el token por creator se elegía con mayhem incluidos).")
    print("- Tres franjas de 8 minutos (2026-09-28, 2026-09-30, 2026-10-01). n pequeños en los cortes de 20 s.")
    print("- Mayhem: precio que cambia fuera de los trades; sus retornos no son fiables.")


if __name__ == "__main__":
    main()
