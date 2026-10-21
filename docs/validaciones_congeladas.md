# Validaciones congeladas (V1, V2, V3)

**Estas definiciones no se modifican.** Cualquier cambio es una hipótesis nueva que exige pre-registro y tokens nuevos. Movidas **literalmente** desde `CLAUDE.md` (antigua sección 8) el 2026-10-21, junto con las definiciones de la robustez post hoc (antigua sección 10), que la validación 2 cita para la supervivencia y los filtros A/B/C.

Las referencias a "sección N" usan la numeración de `CLAUDE.md` anterior al 2026-10-21: secciones 3–7 y 9 → docs/referencia_tecnica.md; 8 → docs/hipotesis_y_resultados.md, docs/validaciones_congeladas.md y docs/exploracion_operadores.md; 10 → docs/estado_y_pasos.md; 11 → `CLAUDE.md`. El detalle de líneas cerradas está en docs/historico_lineas_cerradas.md.

---

### Validación de H4', H5, H6', H7', H8 — CONGELADO antes de descargar las 337 (2026-09-29 15:24 UTC)

Definiciones, umbrales y criterio fijados **antes de descargar ninguno de los 337 tokens pendientes**. Se evalúan **una sola vez** con `marxol entry2` sobre el tramo de validación. Cualquier cambio posterior es una hipótesis nueva que exige tokens nuevos. Sustituye a las propuestas post hoc anteriores (T_entry2, H4'/H6'/H7', H8), que quedan fijadas aquí.

**Tramos**:
- Calibración: los 40 del piloto, creaciones con slot ≤ **451 340 397**.
- Validación: creaciones con slot > 451 340 397. Son los 337 indexados el 2026-09-28 entre las 14:03 y las 14:11 UTC; el primero, en el slot 451 340 405.
- La frontera es limpia (no hay slots compartidos) y está en el código: `entry2::PILOT_LAST_SLOT`.
- `entry2` se niega a cruzar el piloto con el resultado (solo admite `--check`).

**T_entry2** (definición primaria única):
- Es el primer `TradeEvent` en **orden de ejecución** (encadenado de reservas, sección 11) que está en un **slot posterior al de creación** y tiene `timestamp ≥ t0 + 10 s`, buscado dentro de la ventana temprana `[t0, t0 + 300 s)`.
- **Anteriores a T_entry2** = los trades anteriores en orden de ejecución, sin incluirlo. Todos los indicadores se calculan solo con ellos.
- Estado en T_entry2 = reservas tras el último trade anterior (las del `CreateEvent` si no hay ninguno).
- Token sin ningún trade que cumpla la condición dentro de la ventana: **sin entrada**, excluido de las tablas y contado aparte. También se cuentan aparte los tokens con entrada pero sin ventana de precio completa.

**Indicadores** (dirección esperada en los cinco: señal = true ⇒ más pump-y-caída):

| | Definición | Umbral |
|---|---|---|
| H4' | Alguna identidad de creador (la unión del paso 2 de H1) vende antes de T_entry2. Se guarda también el % del supply vendido | ≥ 1 venta |
| H5 | Sin cambios respecto a la ronda anterior: ≥ 1 wallet ajena al creador compra en el slot de creación | ≥ 1 wallet |
| H6' | % de llenado en T_entry2 = `(V0 − V) / 793.1e12`, con `V` = `virtual_token_reserves` en T_entry2 y 793.1e12 las reservas reales iniciales estándar. Mide tokens vendidos, así que no depende del quote ni de los cambios de reservas virtuales de quote de los tokens mayhem. **n/e** si `V0 ≠ 1.073e15` (las reservas reales iniciales no se guardan por token) | **≥ 13.65 %** = mediana en los 28 tokens del piloto con T_entry2 (media de 11.84 y 15.46), calculada sin cruzar con el resultado |
| H7' | Wallets ajenas al creador con exactamente 1 compra + 1 venta antes de T_entry2 | **≥ 1** = mediana del piloto (n = 28), que **no es 0**: 12 tokens con 0 y la 14.ª y 15.ª observación valen 1 |
| H8 | % del supply (10^15) comprado por identidades de creador en el slot de creación. **Origen post hoc**: se formuló mirando el piloto (7 tokens con entrada a 1.5× en la sola compra del creador), así que solo se evalúa en tokens nuevos | **≥ 15 %** |

Mediana convencional: con n par, la media de los dos valores centrales.

**Resultado primario**: pump-y-caída con la definición vigente (H1c/H1b): pico ≥ 2× el precio inicial y caída > 30 % del pico al cierre, en `[t0, t0 + 1 h)`. Exige ventana de precio completa de ≥ 1 h.

**Criterio de éxito**, por hipótesis, sobre los tokens con T_entry2 y resultado evaluable. Es **CONFIRMADA** si se cumplen las cuatro condiciones:
1. ≥ 3 tokens en cada grupo.
2. p₁ − p₀ ≥ 20 puntos en la fracción de pump-y-caída.
3. **Fisher bilateral p < 0.01** (≈ 0.05 / 5, corrección por 5 hipótesis).
4. p₁ − p₀ > 0 también sin el **token de mayor peso**, definido como el token con más trades en su ventana de 5 min entre los que entran en las tablas (definición ciega al resultado).

Si falla alguna es **NO CONFIRMADA**, y se indica si la dirección es la opuesta. Si algún grupo tiene < 3 tokens es **NO EVALUABLE**. La odds ratio y su IC se reportan, pero no deciden.

**Resultado secundario** (descriptivo, sin veredicto): retorno neto desde T_entry2 a +10, +30 y +60 min.
- Fórmula: `(P_salida / P_entrada) · (1 − f)² − 1`.
  - `f = (fee_basis_points + creator_fee_basis_points) / 10⁴` del `TradeEvent` de T_entry2 (entrada y salida con la misma comisión).
  - `P_entrada` = precio en el estado de T_entry2.
  - `P_salida` = último punto de precio de la curva con `timestamp ≤ t_T_entry2 + Δ`.
- Posición marginal: sin impacto de precio. No se cuentan otras componentes (cashback, buyback, holder rewards).
- **n/e** si la curva se completa antes de la salida (el precio pasa a PumpSwap, que no se descarga), si la salida cae fuera del rango descargado o si no se conoce la comisión.
- Se reportan la mediana por grupo de cada indicador y Mann-Whitney bilateral (aproximación normal con corrección por empates y continuidad).
- Por eso `prices` descarga ahora 65 min (`[t0, t0 + 3 900 s)`): T_entry2 ≤ t0 + 5 min, así que +60 min queda cubierto. El cálculo de H1b sigue recortando a 1 h.
- Las comisiones solo se guardan desde esta versión, así que en el piloto el retorno es n/e (0 de 28 con comisión).

**Chequeo de no-degeneración en el piloto** (`marxol entry2 --check --tramo piloto`, sin calcular el resultado):
- 28 de 40 con T_entry2 y 12 sin entrada; orden de ejecución completo en 28/28.
- Grupos true/false: H4' 17/11, H5 16/12, H6' 14/14, H7' 16/12, H8 9/19. Ninguno degenerado.
- T_entry2: mediana a 12 s, 15.5 trades antes de entrar, múltiplo de precio mediano 1.31×.
- En 2 tokens mayhem el múltiplo es < 1: sus reservas virtuales de quote cambian fuera de los trades. No es un error.

---

### Validación 2 (réplica) — CONGELADO antes de descargar (2026-09-30 15:17 UTC)

Fijado **antes de indexar ni descargar ningún token del tramo**. Se evalúa **una sola vez**. Cualquier cambio posterior es una hipótesis nueva que exige tokens nuevos. Los puntos marcados "(confirmado por Roi)" se resolvieron al pre-registrar, porque el texto original era ambiguo.

**Muestra**:
- **S2** = el primer slot con `blockTime ≥ 1790730000` (2026-09-30 01:00:00 UTC), según `getSignaturesForAddress` de la PDA `mint-authority` (`TSLvdd1p…`). Se resuelve al descargar y se anota aquí con su número (confirmado por Roi).
- **Tramo "validación 2"**: las primeras **400 creaciones** exitosas (con `CreateEvent`) con slot ≥ S2, ordenadas por `(slot, mint lexicográfico)` (confirmado por Roi). Es otro día y otra franja horaria que la validación 1 (2026-09-28, 14:03–14:11 UTC).
- Quedan fuera el piloto (40) y la validación 1 (337).
- Si la indexación hacia atrás no llega a S2, o no hay 400 creaciones con slot ≥ S2: **parar y preguntar**.
- **Coste**: estimación ~1.1M CU; **tope de la ejecución 2M CU**. Al alcanzarlo, la ejecución se detiene.
  - Nota de ejecución, no forma parte del criterio: el `index` actual pide `getTransaction` de cada firma que recorre. Llegar a S2 desde la hora actual costaría del orden de 1.6M CU (unos 39 000 tokens creados desde las 01:00). Hay que recorrer solo las listas de firmas hasta S2 y pedir la transacción únicamente de las 400.
  - Descargar tarde encarece las ventanas de los tokens que siguieron activos (sección 10), así que la estimación de 1.1M es optimista.

**Definiciones sin cambios**:
- T_entry2 y H4', H5, H6', H7', H8 con sus umbrales, incluido 13.65 % en H6' ("Validación de H4', H5, H6', H7', H8 — CONGELADO").
- Pump-y-caída en 1 h (definición vigente).
- Supervivencia: ≥ 1 trade a más de 10 min de T_entry2; n/e si la curva se completa antes de T_entry2 + 10 min (sección 10).
- Los tokens mayhem forman población aparte en todo lo que sea retorno.

**Poblaciones**:
- **Primaria** (decide los veredictos): tokens con T_entry2, **un token por creator** (`CreateEvent.creator`), mayhem incluidos. Se queda el token de menor slot. Si hay empate, el de menor orden de ejecución; si no está disponible, el mint en orden lexicográfico.
- **Secundaria 1**: todos los tokens con T_entry2.
- **Secundaria 2**: todos los tokens con T_entry2 sin mayhem.

**Criterio de CONFIRMACIÓN EN RÉPLICA**, por separado para H4', H5, H6', H7' y H8, con pump-y-caída en 1 h como resultado. **CONFIRMADA EN RÉPLICA** si se cumplen todas:
1. En la población primaria:
   - ≥ 3 tokens en cada grupo;
   - p₁ − p₀ ≥ 20 puntos;
   - Fisher bilateral p < 0.01;
   - p₁ − p₀ > 0 también sin el **token de más trades**: el que tiene más trades en su ventana de 5 min entre los de la población primaria (definición ciega al resultado).
2. p₁ − p₀ > 0 en la secundaria 1 y en la secundaria 2.

Si no, **NO CONFIRMADA EN RÉPLICA**, indicando si la dirección es la opuesta. La odds ratio y su IC se reportan, pero no deciden.

**H10** (secundaria): los tokens con señal tienen **más** supervivencia que los que no la tienen, para cada una de H5, H6', H7' y H8. Se aplica **el mismo criterio de réplica completo**, con la supervivencia como resultado y los n/e excluidos: ≥ 20 puntos, Fisher p < 0.01, ≥ 3 por grupo y misma dirección sin el token de más trades, todo en la primaria, más la misma dirección en las dos secundarias (confirmado por Roi). Como referencia post hoc de la validación 1 (sección 8, robustez): las diferencias fueron +19, +15, +19 y +30 puntos.

**No entran en esta ronda**: H11 y H12. H9 no se pre-registra como "mediana > 0": queda descriptiva.

**Condiciones de parada** (informar y detenerse, sin cambiar definiciones ni umbrales):
- **Algún grupo con < 3 tokens** en alguna hipótesis (H4'–H8 o H10), en cualquiera de las tres poblaciones (confirmado por Roi).

**Baja potencia**: si hay menos de **250 tokens con T_entry2** (secundaria 1), la evaluación **no se detiene**: se hace igual, y todo veredicto NO CONFIRMADA EN RÉPLICA se etiqueta **"baja potencia"**. Con esa etiqueta, el resultado no se interpreta como descarte.

**Ejecución, fases 1 y 2 (2026-09-30)**: S2 = slot 451 810 013. **Tramo validación 2 = creaciones con slot en [451 810 013, 451 811 800]**: exactamente 400, de 01:00:00 a 01:07:58 UTC.
400 ventanas y 400 precios completos; total **1 354 600 CU**. `--check`: 283 tokens con T_entry2 (no aplica baja potencia) y ningún grupo < 3. Detalle literal: docs/historico_lineas_cerradas.md

**Revisión 2026-09-30 15:48 UTC, hecha antes de indexar ningún token del tramo** (0 creaciones con `timestamp ≥ 1790730000` en `marxol.db`): la condición "menos de 250 tokens con T_entry2" pasó de condición de parada a la etiqueta de baja potencia de arriba. Motivo (Roi): parar después de descargar desperdicia la cuota y no protege de nada. No se cambió nada más.

**Resultados descriptivos (sin veredicto)**, solo sin mayhem:
- **Horizontes**: +5 s, +30 s, +5 min, +30 min y +60 min desde T_entry2.
- **Retorno neto en dos versiones**:
  - **(a)** La definición congelada de la validación 1 (`entry2::returns`): último punto con `timestamp ≤ t_T_entry2 + h`, ordenado por `(timestamp, slot)`.
  - **(b)** Igual que (a), pero el precio de salida es el del **último trade en orden de ejecución** (reconstruido encadenando reservas) con `timestamp ≤ t_T_entry2 + h`.
    - Mismo precio de entrada, misma comisión y mismas reglas de n/e que (a).
    - Si el slot del punto de salida no se puede reconstruir, ese token queda n/e en ese horizonte para (b) (confirmado por Roi).
    - Se reporta cuántos slots y cuántos tokens por horizonte no se pudieron reconstruir.
- **Por horizonte y por grupo señal / sin señal** de H4', H5, H6', H7' y H8, y para todos: mediana del retorno neto (a) y (b), y fracción con retorno neto > 0.
- **Métricas de filtro A, B y C** (definiciones de la sección 10) en **las tres poblaciones** (confirmado por Roi):
  - Precisión, cobertura, sacrificio y número de tokens no evitados, con "malo" principal = pump-y-caída.
  - "Malo" secundario = retorno neto a +30 min < 0, solo sin mayhem.
  - Retorno del resto = mediana del retorno neto a +30 min de los no evitados, sin mayhem, en las versiones (a) y (b).
  - Intervalo bootstrap: 2 000 réplicas, semilla 20260930, percentiles 2.5–97.5. Remuestreo simple en la primaria y por clúster de creator en las secundarias.

---

### Validación 3: filtro de descarte D2 — CONGELADO antes de descargar (2026-10-01 16:39:58 UTC)

Fijado **antes de indexar ni descargar ningún token del tramo** (0 creaciones con `timestamp ≥ 1790845200` en `marxol.db` al congelar). Se evalúa **una sola vez**. Cualquier cambio posterior es una hipótesis nueva que exige un pre-registro y tokens nuevos.

**Origen**:
- D2 salió de análisis post hoc sobre V1 y V2 (`resultados/cribado_posthoc.txt` y `cribado_posthoc2.txt`), así que ambas están contaminadas para elegirlo. La validación 3 es la prueba fuera de muestra.
- No pretende demostrar una entrada rentable: los tokens que pasan el filtro siguen con retorno mediano negativo. **Mide solo si el filtro separa grupos.**

**Filtro D2 (congelado)**:
- Un token se descarta si cumple al menos 2 de H4', H6' y H7', con las definiciones ya congeladas en esta sección ("Validación de H4', H5, H6', H7', H8 — CONGELADO", incluido el umbral de 13.65 % de H6').
- Las señales se calculan con las definiciones congeladas. La unión de identidades de creador (paso 2 de H1) incluye información posterior a T_entry2: 4 tokens en V2 y 3 en V1, sin cambiar ninguna señal. No se modifica. En V3 se reporta, de forma descriptiva, cuántas señales cambiarían si la unión se limitara a lo anterior a T_entry2.

**Tramo**:
- Creaciones con `timestamp` en **[1790845200, 1790845680)**, es decir, del 2026-10-01 entre las 09:00:00 y las 09:08:00 UTC, con el fin excluido. De esa franja no se ha descargado ningún dato de ventanas ni de precios.
- Se indexa con `index-tramo --from 1790845200` y un `--count` holgado. **El tramo se cierra por timestamp, no por el count.**
- El rango de slots resultante se anota aquí antes de descargar ventanas o precios. No se amplía ni se cambia después.

**Población (decide el veredicto)**:
- Primero se excluyen los tokens mayhem. Entre los restantes con T_entry2, se toma **un token por creator** con la misma regla que la validación 2:
  - La identidad es `CreateEvent.creator`.
  - Se queda el token de menor slot. Si hay empate, el de menor orden de ejecución; si no está disponible, el mint en orden lexicográfico.
- "El creador es el del `CreateEvent`" se aplica solo a la población y a esta selección, no a las señales.
- Los tokens mayhem se reportan en una tabla aparte, sin criterio.

**Resultado principal**:
- Retorno neto desde T_entry2 a **+30 min**, versión (a) (la definición congelada de `entry2::returns`).
- Comparación: tokens que **pasan** el filtro frente a tokens **descartados**.
- Los tokens sin precio de salida en el horizonte se tratan igual que en la validación 2: son n/e si la curva se completa en o antes de la salida, si la salida cae fuera del rango descargado o si no se conoce la comisión. Se cuentan aparte y se excluyen de la comparación.

**Criterio de éxito**: se declara **"D2 SEPARA GRUPOS"** solo si se cumplen TODAS las condiciones, sobre los tokens de la población que entran en la comparación:
1. Al menos 20 tokens descartados y al menos 20 que pasan (incluye el mínimo de 3 por grupo de la validación 2).
2. Mann-Whitney **unilateral** (pasan > descartados) con p < 0.01. Se usa la misma aproximación normal, con corrección por empates y de continuidad, que la implementación actual (`entry2::mann_whitney`), en versión unilateral.
3. Diferencia de medianas (pasan − descartados) de al menos **10 puntos** de retorno neto. El umbral está por debajo, a propósito, de los ~25 puntos observados en V2.
4. La misma dirección (diferencia de medianas > 0) sin el **token de mayor peso (definición congelada)**: el token con más trades en su ventana de 5 min, entre los tokens de la comparación.
5. Bootstrap de 2 000 réplicas, semilla 20260930, remuestreando tokens (equivale al clúster de creator, con un token por creator). El IC al 95 % de la diferencia de medianas (percentiles 2.5–97.5) **excluye 0**.

Veredictos posibles:
- Si falla el criterio 1: **"NO CONCLUYENTE POR POTENCIA"**. No se lee como negativo. No se amplía el tramo. Repetirlo con otro tramo exige un pre-registro nuevo, y el resultado de este se conserva en el registro.
- Si se cumple el criterio 1 y falla cualquiera de los criterios 2–5: **"NO CONFIRMADO"**. No se reinterpreta.

**Secundario (descriptivo, sin criterio)**:
- Las mismas tablas a +5 s, +30 s, +5 min y +60 min.
- Porcentaje de tokens descartados.
- Versión (b) del retorno (definición de la validación 2).
- Fracción con retorno neto > 0.
- Pump-y-caída capturados, con el momento del primer 2x respecto a T_entry2 (antes, en su slot o después; como en `cribado_posthoc2`).
- Cuántas señales H4' y H7' cambiarían con identidades de creador limitadas a lo anterior a T_entry2.
- La tabla de los tokens mayhem.

**Lo que NO se concluye**:
- Un veredicto positivo no implica que los tokens que pasan sean rentables, ni que el filtro sirva fuera de estas condiciones de mercado.
- Solo **"NO CONFIRMADO"** cierra D2 como filtro de descarte. **"NO CONCLUYENTE POR POTENCIA"** no lo cierra.

**Costes y límites**:
- Se estima en torno a 1.4 M CU por tramo.
- Cubre otra franja horaria, pero sigue siendo una sola ráfaga de unos 8 minutos.

#### Adenda operativa (anterior a indexar y descargar) — redactada 2026-10-01 17:00:21 UTC

Solo concreta casos que el texto congelado de la validación 3 no cubre. **No cambia ningún criterio** ni ninguna definición. Redactada antes de indexar y descargar ningún token del tramo.

1. **H6' no evaluable.** Si H6' no es evaluable en un token y se cumple exactamente una de H4'/H7', el token es "no evaluable": se excluye de la comparación y se cuenta aparte. No bloquea el veredicto.
2. **Empate en el token de mayor peso** (más trades en su ventana de 5 min): se desempata por menor slot y, después, por mint lexicográfico.
3. **Datos que faltan.**
   - Si faltan las ventanas o los precios de un token tras 3 intentos de descarga, el token es n/e "sin datos": se excluye de la comparación y se cuenta aparte.
   - Si faltan creaciones del tramo (indexado incompleto), no se calcula el veredicto hasta completarlo.
4. **Borde del tramo.**
   - La población se define siempre por el timestamp del `CreateEvent` en la base, en [1790845200, 1790845680).
   - El listado por blockTime se hace con un margen de 120 s en `--until`.
   - Si hay diferencias en el borde, se reportan y no cambian el tramo.
5. **Prueba previa.** La primera ejecución de `index-tramo --until` usa un `--count` pequeño para comprobar el comportamiento, antes de la ejecución completa.
6. **Detalles de cálculo.**
   - Criterio 3 con tolerancia 1e-9.
   - "El IC excluye 0" se lee como estricto.
   - En el bootstrap se descartan las réplicas con un grupo vacío y se reporta cuántas fueron.
7. **Token sin ventana temprana que precede a otros de su creator** (añadido 2026-10-01 17:04:43 UTC, antes de indexar y descargar). Es un token sin ventana temprana tras 3 intentos que precede al token que se habría elegido para su creator. No se puede saber si tenía T_entry2, y por tanto qué token representa al creator. El creator queda fuera de la comparación y se cuenta aparte como "sin datos". No entra el siguiente token del creator. (Redacción ajustada 2026-10-01 17:06:50 UTC, antes de indexar y descargar: antes decía "que es el primero de un creator con más tokens en el tramo". Solo alinea el texto con la regla ya implementada; no cambia ningún criterio).

#### Ejecución, fases 1 y 2 (2026-10-06 y 2026-10-08, sin mirar el resultado)

S3 = slot 452 240 867. **Tramo validación 3 = 196 creaciones con timestamp en [1790845200, 1790845680), slots [452 240 867, 452 242 654]**; las 42 del margen quedan fuera, sin diferencias de borde. Este rango no se amplía ni se cambia.
196 ventanas y 196 precios completos; total **970 440 CU**. `--check`: población de 75 tokens (29 descartados, 46 pasan), 0 no evaluables y 0 sin datos. Detalle literal: docs/historico_lineas_cerradas.md

---

   **Comprobación de robustez de la validación congelada — POST HOC, SIN VEREDICTO, EJECUTADA 2026-09-30** (definiciones fijadas antes de calcular; resultado en la sección 8, "ROBUSTEZ POST HOC"). Solo `marxol.db`, sin red. No cambia ningún veredicto congelado ni ninguna definición previa.
   - **Población base**: los 251 tokens de validación con T_entry2 (los de las tablas congeladas). Quedan fuera los 40 del piloto y los 86 sin T_entry2.
   - **Un token por creator**: el de menor slot de creación del creator dentro de los 251. Empate: menor orden de ejecución dentro del slot; si no está disponible, mint en orden lexicográfico. En la práctica no hay empates entre los 210 creators.
   - **Fábrica**: creator con ≥ 5 tokens entre las 377 creaciones indexadas (9 creators, 31 tokens de los 251).
   - **Supervivencia** (definición de H10): ≥ 1 trade a más de 10 min de T_entry2. **Decisión confirmada por Roi**: si la curva se completa antes de T_entry2 + 10 min, n/e (1 token), la misma regla que para el retorno.
   - **Retorno neto**: la definición de la validación congelada. Medianas a +10, +30 y +60 min; solo +30 lleva intervalo.
   - **Mayhem**: las métricas de retorno se reportan solo sin mayhem. Las de pump-y-caída y supervivencia, con todos y sin mayhem.
   - **"Malo" principal** = pump-y-caída (definición congelada). **"Malo" secundario** = retorno neto a +30 min < 0. "Bueno" = el complementario en cada caso.
   - **Filtros "evitar"**:
     - A: señal en cualquiera de H5, H6', H7', H8.
     - B: señal en ≥ 2 de las cuatro.
     - C: señal en H5 o H7' (las dos no mecánicas).
     - Además, cada señal por separado, con H4' aparte.
   - **Métricas del filtro**:
     - Precisión: fracción de "malos" entre los evitados.
     - Cobertura: fracción de los "malos" que se evita.
     - Sacrificio: fracción de los "buenos" que se evita.
     - Retorno del resto: mediana del retorno a +30 entre los no evitados, sin mayhem, con IC bootstrap por clúster.
     - Número de tokens no evitados.
   - **Subconjuntos de las tablas 2×2** (sin combinarlos): (a) un token por creator; (b) los 251; (c) los 251 sin fábricas; (d) los 251 sin mayhem.
   - **Bootstrap**: 2 000 réplicas, semilla 20260930, percentiles 2.5–97.5.
     - En (a), remuestreo simple de tokens.
     - En (b), (c) y (d), remuestreo de creators con reemplazo, con todos sus tokens (clúster). **Decisión confirmada por Roi**: (c) y (d) también llevan intervalo.
     - Una réplica con algún grupo vacío se descarta para esa estadística, y se reporta cuántas.
