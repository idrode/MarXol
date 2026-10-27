# BORRADOR — Adenda 2 al pre-registro de la fase 5 (2026-10-23)

**Sustituido por la adenda 3 (docs/adenda_3_2026-10-27.md), que incorpora sus cambios. No se aplica por separado.**

**Estado: borrador, sin commit y sin aprobar.** Sustituye al borrador del 2026-10-22 (C1 como razón principal, k = 3). Solo se puede fijar antes de calcular nada sobre el bloque B. De `marxol_vivo.db` no se ha leído ningún retorno.

## Cambios respecto a la adenda 1

1. **k = 2 (R1 y R2) y umbral de p = 0,01 / 2 = 0,005.** Es lo que ya fija la adenda 1; esta adenda no añade razones.
2. **C1 pasa a nota descriptiva y deja de ser razón de veto.** Motivo (post hoc, `marxol.db`):
   - No sobrevive al control por progreso de la curva en T_entry2 (docs/fase_c_control_progreso_2026-10-22.md, declarado antes de calcular): diferencia ponderada +1,8 pts [−1,5, +5,0], y en el tercil alto va en contra.
   - Dentro de "H6′ no", la diferencia de +9,2 pts baja a +1,6 pts (p = 0,13) si solo se comparan tokens con un progreso que C1 pueda alcanzar (resultados/c1_vs_h6_progreso.txt).
   - Lectura: C1 mide sobre todo el progreso de la curva, que ya recoge H6′ (rho de rangos llenado–progreso = 1,000).
3. **R1 usa el pagador** (`CreateEvent.user`), sin cambios. La comprobación del 2026-10-22 apoya que es la identidad correcta: GMGN atribuye el token al pagador en 20 de 20 tokens. En los 4 en que creator ≠ pagador, el creator no hizo ningún trade en la ventana temprana (docs/informe_noche_2026-10-22.md, §3).

## R1 y R2

Definiciones, población, n/e, datos y criterio: los de la adenda 1, sin cambios.
- R1: pagador con ≥ 5 creaciones en `[t0 − 7 d, t0)`.
- R2: resultado realista del creador en T_entry2 > 1 000 lamports.
- **"R VALIDADA" solo si se cumplen todos**: ≥ 20 tokens por grupo; Mann-Whitney unilateral (falso > verdadero) con p < 0,005; diferencia de medianas ≥ 10 pts; misma dirección sin el token de mayor peso; IC bootstrap al 95 % (2 000 réplicas, semilla 20260930, por creator) que excluye 0.
- Fecha: una sola evaluación, no antes del 2026-11-15 00:00 UTC.

## Notas descriptivas (sin criterio)

- **C1** (tenencia del creador en T_entry2 > 5 %): diferencia de medianas, por terciles de progreso con los cortes congelados de `marxol.db` (2,2061 % y 9,5404 %) y por H6′.
- **C2** (≥ 1 tx con compras de ≥ 2 cuentas antes de T_entry2): las mismas tablas.
- **Etiqueta "retorno ≤ −50 %"** a +30 min: prevalencia, y precisión y recall de R1, R2 y C1.
- **Amenaza a la validez**: desde el 2026-10-19 15:46 UTC el `TradeEvent` trae `quote_transfer_fee` (comisión Token-2022 del quote), que el retorno congelado no descuenta. `collect` no guarda ese campo, así que se informa cuántos tokens del bloque B cotizan en un quote distinto de SOL y, como sensibilidad, se repite R1 solo con los tokens en SOL. R2 ya excluye los demás por n/e.

## Potencia y n esperado

**Decisión conservadora sobre "la mitad del efecto observado".** R1 y R2 no tienen efecto observado: la adenda 1 los registró sin cruzarlos con ningún retorno, tampoco en `marxol.db`. Cruzarlos ahora para medirlo les quitaría esa condición. Por eso la potencia se da en función del efecto verdadero d, y como referencia se toma la mitad de los efectos que sí se conocen: C1, +16,6 → 8,3 pts; D2 en V3, +19,0 → 9,5 pts. Si quieres la cifra con el efecto propio de R1 y R2, es tu decisión, y hay que anotar que se han cruzado con retornos.

**n esperado** (docs/informe_noche_2026-10-22.md): ≈ 960 tokens en el bloque B, sin mayhem y con un token por creator.
- **R1**: prevalencia ≈ 64 % (tokens con ≥ 5 previos, resultados/fase3_cprima.txt), es decir ≈ 346 falso y 614 verdadero.
- **R2**: excluye los tokens no cotizados en SOL (≈ 23 %), con lo que n ≈ 740. Prevalencia ≈ 44 %, aproximada con la versión a +5 min (resultados/fase3_explora.txt), es decir ≈ 414 falso y 326 verdadero.
- La prevalencia de R2 en T_entry2 no está medida.

**Probabilidad de validar** (scripts/potencia_adenda2_r.py → resultados/potencia_adenda2_r.txt):
- Los dos grupos se remuestrean de los retornos (a) a +30 min de V1–V3 agregados, sin separar por R. El grupo verdadero se desplaza −d.
- Criterios simulados: el 2 (p < 0,005), el 3 (≥ 10 pts) y el 5 (IC). 300 simulaciones con 300 réplicas.

| escenario (falso / verdadero) | d +5,0 | **+8,3** (½ C1) | **+9,5** (½ D2) | +10,0 | +12,5 | +15,0 |
|---|---|---|---|---|---|---|
| R1, n 960 (346 / 614) | 0,00 | 0,04 | 0,33 | 0,50 | 0,98 | 1,00 |
| R1, n 480 (173 / 307) | 0,00 | 0,11 | 0,36 | 0,48 | 0,93 | 1,00 |
| R2, n 740 (414 / 326) | 0,00 | 0,06 | 0,30 | 0,53 | 0,99 | 1,00 |
| R2, n 370 (207 / 163) | 0,00 | 0,16 | 0,41 | 0,49 | 0,94 | 1,00 |

- **Efecto mínimo detectable (potencia ≥ 80 %): ≈ 11–12 pts para las dos razones**, con cualquier n de la tabla. La n no limita: el criterio 1 (≥ 20 por grupo) se cumple de sobra.
- **Lo que manda es el suelo de 10 pts (criterio 3).** Con un efecto verdadero de 10 pts, la probabilidad es ≈ 50 %. Por debajo cae deprisa, y cae más cuanto mayor es n, porque hay menos ruido que empuje la diferencia observada por encima de 10.
- Con la mitad de un efecto como los conocidos (8–10 pts), la probabilidad de validar es baja: 0,04–0,41.
- Limitaciones: el desplazamiento es aditivo con retornos acotados en −100 %; el bootstrap es por token y con 300 réplicas; no se simula el criterio 4; y se supone que el bloque B se parece a V1–V3.
