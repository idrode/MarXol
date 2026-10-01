#!/usr/bin/env python3
"""Cribado, segunda pasada — POST HOC, SIN PRE-REGISTRO, SOLO DESCRIPTIVO
(CLAUDE.md sección 8). Sin contrastes ni p-valores. Solo lee marxol.db
(read-only, sin red, sin cuota). Reutiliza las definiciones y la carga de
`cribado_posthoc.py`.

Reproducir (desde la raíz del repo):
    cargo build --release && python3 -B scripts/cribado_posthoc2.py > resultados/cribado_posthoc2.txt

Población: primaria (un token por creator con T_entry2: menor slot, empate por
mint) en V1 (210) y en V2 (197), por separado; mayhem siempre aparte.
(La primera pasada usó los 251 de V1; aquí V1 también es primaria.)

Parte B, "pico de 2x": primer punto de precio de la curva con precio >= 2x el
inicial dentro de [t0, t0 + 1 h). "Antes de T_entry2" = en un slot anterior al
de T_entry2. T_entry2 es el primer trade de su slot en orden de ejecución, así
que los trades de su slot son T_entry2 o posteriores ("en el slot de T_entry2"
se cuenta aparte, como después).
"""
import argparse, collections, sqlite3
import cribado_posthoc as cp

FILTERS = [
    ("D2  (>= 2 de H4'/H6'/H7')", lambda s: sum(bool(s[k]) for k in ("H4'", "H6'", "H7'")) >= 2),
    ("D2b (H4' y H7')", lambda s: bool(s["H4'"]) and bool(s["H7'"])),
    ("Dany (H4' o H7')", lambda s: bool(s["H4'"]) or bool(s["H7'"])),
    ("H6' sola", lambda s: bool(s["H6'"])),
]
CAPTURE = FILTERS[:3]


def primary(toks, tramo):
    first = {}
    for t in sorted((t for t in toks.values() if t["tramo"] == tramo and t["e"] is not None), key=lambda t: (t["slot"], t["mint"])):
        first.setdefault(t["creator"], t)
    return list(first.values())


def filter_table(sub, name, fn):
    disc = [t for t in sub if fn(t["sig"])]
    keep = [t for t in sub if not fn(t["sig"])]
    n = len(sub)
    malos = sum(t["pd"] for t in sub)
    md = sum(t["pd"] for t in disc)
    good = [t for t in sub if t["ret"][1800] is not None and t["ret"][1800] > 0]
    gd = sum(fn(t["sig"]) for t in good)
    print(f"  [{name}] descartados {len(disc)}/{n} ({cp.pc(len(disc), n)})  pasan {len(keep)}")
    print(f"    p&c capturados {md}/{malos} ({cp.pc(md, malos)}), escapados {malos - md}  precisión {md}/{len(disc)} ({cp.pc(md, len(disc))})  "
          f"buenos: descartados {gd}, pasan {len(good) - gd} (de {len(good)})")
    cp.group_line("descartados", disc)
    cp.group_line("pasan", keep)


def peak_2x(t, p0):
    """Slot y segundos del primer punto con precio >= 2x en la primera hora."""
    for ts, sl, pr in t["pts"]:
        if t["t0"] <= ts < t["t0"] + 3600 and pr >= 2.0 * p0:
            return sl, ts
    return None


def main():
    ap = __import__("argparse").ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--marxol", default="target/release/marxol")
    ap.add_argument("--db", default="marxol.db")
    args = ap.parse_args()
    toks, _ = cp.load(args.marxol, args.db)
    n1, pd_bad, ret_bad = cp.controls(args.marxol, args.db, toks)
    db = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    q = lambda s, *a: db.execute(s, a).fetchall()
    p0 = {m: qr / tr for m, qr, tr in q("select mint, quote_reserves, token_reserves from price_points where kind = 'create'")}
    pops = [("V2 primaria (cifra honesta)", primary(toks, "V2")), ("V1 primaria (señales elegidas aquí, optimista)", primary(toks, "V1"))]

    print("CRIBADO, SEGUNDA PASADA — POST HOC, SIN PRE-REGISTRO, SOLO DESCRIPTIVO (sin contrastes ni p-valores)")
    print(f"controles (heredados de cribado_posthoc): p&c distinto de entry2 --json {pd_bad}/{n1}; retornos distintos {ret_bad}")
    for name, pop in pops:
        print(f"  {name}: n = {len(pop)} (sin mayhem {sum(not t['mayhem'] for t in pop)}, mayhem {sum(t['mayhem'] for t in pop)}); "
              f"H6' n/e {sum(t['sig'][chr(72) + '6' + chr(39)] is None for t in pop)}")

    print("\n" + "=" * 100)
    print("PARTE A. D2 SIN H6'")
    for name, pop in pops:
        for mh in (False, True):
            sub = [t for t in pop if t["mayhem"] == mh]
            print(f"\n## {name} — {'MAYHEM (aparte; retornos no fiables)' if mh else 'sin mayhem'}: n = {len(sub)}, "
                  f"p&c = {sum(t['pd'] for t in sub)}")
            for fname, fn in FILTERS:
                filter_table(sub, fname, fn)
            print("  Solapamiento por pares (n · p&c en la celda)")
            for a, b in (("H4'", "H6'"), ("H4'", "H7'"), ("H6'", "H7'")):
                cell = collections.defaultdict(list)
                for t in sub:
                    cell[(bool(t["sig"][a]), bool(t["sig"][b]))].append(t["pd"])
                f = lambda k: f"{len(cell[k]):3} · {sum(cell[k])}/{len(cell[k])} ({cp.pc(sum(cell[k]), len(cell[k]))})"
                print(f"    {a}×{b}:  {a} sí/{b} sí {f((True, True))}   {a} sí/{b} no {f((True, False))}   "
                      f"{a} no/{b} sí {f((False, True))}   {a} no/{b} no {f((False, False))}")

    print("\n" + "=" * 100)
    print("PARTE B. REALIZABILIDAD TEMPORAL")
    print("\nB1. Pump-y-caída: ¿el primer 2x ocurre antes o después de T_entry2?")
    timing = {}
    for name, pop in pops:
        for mh in (False, True):
            pdt = [t for t in pop if t["mayhem"] == mh and t["pd"]]
            c = collections.Counter()
            for t in pdt:
                pk = peak_2x(t, p0[t["mint"]])
                es = t["e"]["entry_slot"]
                kind = "sin 2x" if pk is None else "antes" if pk[0] < es else "en el slot de T_entry2" if pk[0] == es else "después"
                timing[t["mint"]] = kind
                c[kind] += 1
                # Pico máximo de la hora (además del primer 2x).
                w = [p for p in t["pts"] if t["t0"] <= p[0] < t["t0"] + 3600]
                top = max(p[2] for p in w)
                t["max_before"] = next(p for p in w if p[2] == top)[1] < es
            mb = sum(t["max_before"] for t in pdt)
            print(f"  {name} — {'MAYHEM' if mh else 'sin mayhem'}: p&c {len(pdt)}; primer 2x antes de T_entry2 {c['antes']}, "
                  f"en su slot {c['en el slot de T_entry2']}, después {c['después']}"
                  + (f", sin 2x {c['sin 2x']}" if c["sin 2x"] else "")
                  + f"   (pico máximo de la hora antes de T_entry2: {mb})")
            ms = sorted(t["e"]["entry_multiple"] for t in pdt)
            if ms:
                print(f"      múltiplo de precio en T_entry2 de esos p&c: mediana {cp.med(ms):.2f}x, máx {ms[-1]:.2f}x")

    print("\nB2. Capturas restringidas a los p&c con el primer 2x en el slot de T_entry2 o después (evitables en la práctica)")
    for name, pop in pops:
        for mh in (False, True):
            sub = [t for t in pop if t["mayhem"] == mh]
            late = [t for t in sub if t["pd"] and timing.get(t["mint"]) in ("después", "en el slot de T_entry2")]
            early = [t for t in sub if t["pd"] and timing.get(t["mint"]) == "antes"]
            print(f"  {name} — {'MAYHEM' if mh else 'sin mayhem'}: p&c evitables {len(late)}, ya con 2x antes de entrar {len(early)}")
            for fname, fn in CAPTURE:
                cl = sum(fn(t["sig"]) for t in late)
                ce = sum(fn(t["sig"]) for t in early)
                print(f"    {fname:26} evitables capturados {cl}/{len(late)} ({cp.pc(cl, len(late))}), escapados {len(late) - cl}   "
                      f"(ya con 2x: {ce}/{len(early)})")

    print("\nB3. ¿Las señales usan solo datos hasta T_entry2?")
    print("  Código (src/entry2.rs, compute): H4', H7' y H6' se calculan con `before` = los trades anteriores a T_entry2 en")
    print("  orden de ejecución, y H6' con las reservas tras el último de ellos. Los trades son anteriores. PERO el conjunto")
    print("  de identidades de creador (`creator_ids`, store::creator_identities) es la unión de: creator y payer de la")
    print("  creación, TODOS los cambios de creator del mint y el `creator` embebido en TODOS los trades guardados de la")
    print("  ventana de 5 min, también los posteriores a T_entry2. Eso es información posterior a T_entry2. Afecta a")
    print("  H4' (quién cuenta como dev) y a H7' (quién se excluye de los pares). H6' no depende de identidades.")
    print("  No se corrige. Efecto medido recalculando H4'/H7' con identidades limitadas a lo anterior a T_entry2")
    print("  (creator y payer de la creación, cambios con slot < slot de T_entry2, creator embebido en trades con slot <")
    print("  slot de T_entry2):")
    for name, pop in pops:
        diff = collections.Counter()
        extra = 0
        for t in pop:
            m, es = t["mint"], t["e"]["entry_slot"]
            full = {r[0] for r in q("select creator from creations where mint=?1 union select user from creations where mint=?1 "
                                    "union select new_creator from creator_changes where mint=?1 "
                                    "union select old_creator from creator_changes where mint=?1 and old_creator is not null "
                                    "union select creator from early_trades where mint=?1 and creator is not null", m)}
            strict = {r[0] for r in q("select creator from creations where mint=?1 union select user from creations where mint=?1 "
                                      "union select new_creator from creator_changes where mint=?1 and slot<?2 "
                                      "union select old_creator from creator_changes where mint=?1 and slot<?2 and old_creator is not null "
                                      "union select creator from early_trades where mint=?1 and slot<?2 and creator is not null", m, es)}
            extra += bool(full - strict)
            before = q("select user, is_buy from early_trades where mint=?1 and slot<?2 and timestamp>=?3 and timestamp<?4",
                       m, es, t["t0"], t["t0"] + 300)
            if len(before) != t["e"]["trades_before"]:
                diff["trades anteriores != entry2 (orden incompleto)"] += 1
            for ids, tag in ((full, "full"), (strict, "strict")):
                dev = any(not b and u in ids for u, b in before)
                sides = collections.defaultdict(lambda: [0, 0])
                for u, b in before:
                    if u not in ids:
                        sides[u][0 if b else 1] += 1
                pairs = sum(v == [1, 1] for v in sides.values())
                t[f"h4_{tag}"], t[f"h7_{tag}"] = dev, pairs >= 1
            diff["H4' recalculada (identidades completas) != entry2"] += t["h4_full"] != t["sig"]["H4'"]
            diff["H7' recalculada (identidades completas) != entry2"] += t["h7_full"] != t["sig"]["H7'"]
            diff["H4' cambia con identidades estrictas"] += t["h4_strict"] != t["h4_full"]
            diff["H7' cambia con identidades estrictas"] += t["h7_strict"] != t["h7_full"]
        print(f"  {name}: tokens con identidades posteriores a T_entry2 en el conjunto: {extra}/{len(pop)}; "
              + "; ".join(f"{k}: {v}" for k, v in diff.items()))

    print("\n" + "=" * 100)
    print("LÍMITES")
    print("- Post hoc, sin pre-registro: no confirma ni descarta nada y no cambia ningún veredicto ni definición.")
    print("- Las señales y estas variantes se eligieron mirando datos (V1 y, en esta pasada, también V2): ninguna cifra")
    print("  de aquí es una estimación limpia. Cualquier filtro que se congele tiene que evaluarse en una ráfaga nueva.")
    print("- n pequeño: 33 p&c y 3 'buenos' en V2 sin mayhem; los mayhem son 23 tokens. No hay potencia para comparar")
    print("  filtros por pérdida de buenos.")
    print("- Dos ráfagas cortas de 8 min (2026-09-28 y 2026-09-30).")
    print("- 'Antes/después de T_entry2' se mide por slot; el pico de 2x usa el orden (timestamp, slot) de los puntos y")
    print("  no el orden de ejecución dentro de un slot posterior (no cambia el slot del primer 2x).")
    print("- H6' es en parte mecánica: mide cuánto ha subido ya el precio antes de entrar, lo mismo que se acerca al 2x")
    print("  de la definición de p&c.")
    print("- Las identidades de creador usan datos posteriores a T_entry2 (B3); el efecto medido está arriba.")
    print("- Mayhem: precio que cambia fuera de los trades; sus retornos y picos no son fiables.")


if __name__ == "__main__":
    main()
