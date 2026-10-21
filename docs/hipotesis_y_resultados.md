# Hipótesis H1, H1b, H2, H3 y resultados de las validaciones

Movido **literalmente** desde `CLAUDE.md` (antigua sección 8) el 2026-10-21. Las definiciones congeladas están en docs/validaciones_congeladas.md; los resúmenes de líneas cerradas, en `CLAUDE.md`.

Las referencias a "sección N" usan la numeración de `CLAUDE.md` anterior al 2026-10-21: secciones 3–7 y 9 → docs/referencia_tecnica.md; 8 → docs/hipotesis_y_resultados.md, docs/validaciones_congeladas.md y docs/exploracion_operadores.md; 10 → docs/estado_y_pasos.md; 11 → `CLAUDE.md`. El detalle de líneas cerradas está en docs/historico_lineas_cerradas.md.

---

## 8. Hipótesis a pre-registrar (antes de mirar datos)

Pre-registradas aquí antes de empezar el análisis, para evitar sesgo de confirmación (principio heredado de marXi, sección 2).

---

### H1 — señal temprana de actividad orgánica (redefinida 2026-09-28, antes de generar datos de trades)

Sustituye a la versión anterior de H1 ("la tasa de éxito de un mismo `creator` difiere entre operadores repetidores y de un solo lanzamiento", con éxito = graduación o mcap sostenido). Esa medida retrospectiva pasa a ser **H1b** (validación, ver abajo), no el criterio de entrada. La comparación repetidores vs. un solo lanzamiento queda para cuando H1 esté validada, usando H1 como variable por token.

**Hipótesis**: la presencia, en los primeros minutos de vida de un token, de actividad de compra y venta de varias wallets independientes (no el creador, no bots, no ballenas) es una señal temprana y accionable que anticipa el éxito retrospectivo del token (H1b).

> ⚠️ **Valores provisionales**: los tres umbrales numéricos marcados con **[P]** (ventana de **5 min**, tope por trade de **15 %**, mínimo de **3 wallets**) y los umbrales de bot marcados igual son valores iniciales, **no un criterio cerrado**. Se recalibrarán con datos reales. Para no sobreajustar contra H1b, la recalibración se hace solo sobre un tramo de calibración (los tokens más antiguos) y la validación contra H1b sobre tokens posteriores que no se usaron para recalibrar (partición temporal). Cada recalibración se anota aquí con fecha, valores viejos → nuevos y motivo.

**Criterio operacional**, por token (mint), en este orden:

1. **Ventana** **[P]**: `TradeEvent` con `CreateEvent.timestamp ≤ t < CreateEvent.timestamp + 300 s` (5 min). Los `timestamp` de los eventos tienen resolución de 1 s. Los intervalos del paso 3 se calculan sobre los `timestamp` ordenados, así que el orden entre trades del mismo segundo no los cambia; no hace falta el índice de tx dentro del bloque (que `getTransaction` no da). Orden de almacenamiento: `(slot, timestamp, firma, índice del evento)`.
2. **Excluir al creador**: descartar todo trade cuyo `user` pertenezca al conjunto de identidades de creador del mint: `CreateEvent.creator`, `CreateEvent.user` (el payer), el `creator` vigente de `BondingCurve` (y el `TradeEvent.creator` embebido en cada trade), y cualquier `creator`/`old_creator`/`new_creator` de `SetCreatorEvent`/`MigrateBondingCurveCreatorEvent` del mint. Se usa la unión, no solo el vigente: tras una migración a fee-sharing el creator vigente es una PDA `sharing_config` y dejaría pasar las compras de la wallet original.
3. **Excluir wallets con patrón de bot**: agrupar los trades restantes por `user`. Para wallets con **n ≥ 3 trades** en la ventana:
   - `CV_int` = coeficiente de variación (desviación típica / media) de los intervalos entre trades sucesivos, en segundos.
   - `CV_tam` = coeficiente de variación del tamaño del trade en unidades del `quote_mint` (`quote_amount`; el CV no tiene unidades, así que SOL y USDC son comparables).
   - **Bot si `CV_int < 0.10` o `CV_tam < 0.05`** **[P]**, o si la media de intervalos es 0 (todos los trades en el mismo segundo). Se usa CV y no varianza bruta porque la varianza escala con el tamaño y la frecuencia: una wallet que opera cada 10 s ± 1 s es igual de regular que una que opera cada 100 s ± 10 s. Referencia: la actividad humana/Poisson da CV_int ≈ 1; 0.10 es un orden de magnitud por debajo. Los umbrales se contrastarán con la distribución empírica de CV sobre los primeros datos (criterio de revisión: si el umbral cae por encima del percentil 25 de la distribución, está excluyendo demasiado).
   - Wallets con n < 3 trades no son evaluables por regularidad y **no se excluyen** por esta regla (limitación conocida: un bot que hace una compra y una venta no se detecta aquí).
   - La exclusión es de la wallet entera (todos sus trades de la ventana).
4. **Excluir trades de ballena** **[P]**: descartar cada trade individual cuyo `quote_amount` supere el **15 %** del volumen total (Σ `quote_amount`, compras y ventas) de los trades que quedan tras los pasos 2 y 3 en la ventana completa. El denominador es el de la ventana completa, no el acumulado hasta ese trade (con el acumulado, el primer trade sería siempre el 100 %). Es evaluable en t = fin de ventana, que es cuando se emite la señal. Una sola pasada, sin iterar.
5. **Wallets orgánicas**: de los trades restantes, contar las wallets con al menos un `is_buy = true` **y** al menos un `is_buy = false` dentro de la ventana.
6. **Señal**: `H1 = true` si wallets orgánicas **≥ 3** **[P]**.

Se persiste por token no solo el booleano sino los recuentos intermedios (trades totales, excluidos por creador/bot/ballena, wallets orgánicas), para poder recalibrar los umbrales sin volver a descargar los trades.

### H1b — validación retrospectiva de H1 (no es criterio de entrada)

Fijado 2026-09-28, antes de mirar datos de precio.

- **Ventanas**: **N = 1 h** desde `CreateEvent` **[P]** es la ventana **primaria**, la que valida H1, coherente con el objetivo de entrada temprana. **24 h** se mantiene como ventana **secundaria**, no descartada: sirve para `operator_tracker` a más largo plazo, pero no se usa para decidir si H1 sirve para entrar rápido.
- **Métricas**, calculadas en cada ventana:
  - **Graduación**: existe `CompletePumpAmmMigrationEvent` del mint dentro de la ventana.
  - **Pump sostenido**: sea `pico` el mcap máximo alcanzado dentro de la ventana y `cierre` el mcap al final de la ventana (último precio observado antes del cierre). Es "sostenido" si `cierre ≥ (1 − X) · pico` con **X = 30 %** **[P]** de caída máxima desde el pico. Así se distingue "hubo pump y aguantó" de "pumpeó y se desplomó justo antes de la marca", cosa que no hace comparar dos puntos fijos (creación vs. N).
  - **Condición de pump** **[P]**, propuesta al fijar la métrica y confirmada por Roi (2026-09-28): un token que nunca subió no cae desde su pico y saldría "sostenido" sin haber tenido pump. Por eso además se exige `pico ≥ 2 × mcap inicial` (mcap en el `CreateEvent`). Sin esa condición la métrica mide "no se desplomó", no "hubo pump real".
  - **mcap** = precio × `token_total_supply`, con precio = `virtual_quote_reserves / virtual_token_reserves` del último `TradeEvent` (en unidades del `quote_mint`: no mezclar tokens SOL y USDC en el mismo umbral absoluto; el criterio es relativo, así que funciona para ambos). Tras la graduación el precio sale del pool de PumpSwap (`effective_quote_reserves`, sección 4), no de la bonding curve.
- **Prueba**: sobre los tokens del tramo de validación, una tabla 2×2 por métrica (H1 × graduó en 1 h; H1 × pump sostenido en 1 h). Test exacto de Fisher, α = 0.05. Se reporta la tasa de éxito condicionada a H1 = true y a H1 = false, y la odds ratio con su intervalo de confianza. Las mismas tablas en 24 h se reportan como secundarias. Si no hay diferencia significativa, H1 se documenta como descartada con el mismo detalle que si se confirmara. Recordar que una asociación aquí es correlación, no causalidad (principio 6).
- **Datos que exige**: el pump sostenido necesita la trayectoria de precio de toda la ventana (1 h y 24 h), no solo los trades de los 5 min de H1. Implementado para 1 h (`marxol prices` + `marxol h1b`); 24 h pendiente.
- **Limitación de la implementación actual (2026-09-28)**: solo se descarga el precio de la bonding curve. Si la curva se completa dentro de la ventana, el precio posterior está en PumpSwap, que todavía no se lee. En ese caso "pump sostenido" queda **no evaluable** (se excluye de su tabla 2×2), no se da por cumplido. La graduación sí se mide siempre. Precio inicial = reservas virtuales del `CreateEvent`; cada punto posterior = reservas virtuales tras cada `TradeEvent`.

---

### Resultado de la validación congelada (2026-09-29, `marxol entry2`, ejecutado una sola vez)

**Alcance, antes que nada**: los 337 tokens de validación salen de **una única ráfaga de 8 minutos de un solo día** (creaciones del 2026-09-28 entre las 14:03 y las 14:11 UTC). Validan **dentro de esa ráfaga**. No generalizan a otros momentos, días ni regímenes de mercado: para eso hacen falta muestras de otras fechas y horas. Una asociación aquí es correlación, no causalidad (principio 6).

**Datos**:
- 337 ventanas tempranas y 337 ventanas de precio de 65 min completas, con 0 incompletas en ambas.
- Los 6 fallos puntuales de descarga (429) se reintentaron y completaron. La exclusión por incompleto quedó vacía.
- 86 tokens sin T_entry2, excluidos. **251 en tablas, 63 pump-y-caída (25 %)**.
- 0 tokens con T_entry2 sin ventana de precio. Orden de ejecución completo y comisión conocida en 251/251.
- Token de mayor peso: `9trcArtUpUePNEH6CvnFNDQAd8U9KqcjvPpN4z7LZFDm`.
- Los 40 de calibración no entran en nada de esto.

**Veredicto primario** (criterio congelado: dif. ≥ 20 pts, Fisher p < 0.01, ≥ 3 por grupo, misma dirección sin el token de mayor peso):

| Hipótesis | n señal / sin señal | p&c con señal | p&c sin señal | Dif. | Fisher p | OR (IC95) | Sin mayor peso | Veredicto |
|---|---|---|---|---|---|---|---|---|
| H4' dev vendió antes de T_entry2 | 73 / 178 | 28/73 (38 %) | 35/178 (20 %) | +19 | 0.0036 | 2.54 (1.40–4.63) | +18 | **NO CONFIRMADA** (dif. < 20 pts) |
| H5 wallet ajena en el slot de creación | 125 / 126 | 50/125 (40 %) | 13/126 (10 %) | +30 | < 0.0001 | 5.79 (2.95–11.40) | +29 | **CONFIRMADA** |
| H6' llenado ≥ 13.65 % | 65 / 186 | 38/65 (58 %) | 25/186 (13 %) | +45 | < 0.0001 | 9.06 (4.74–17.34) | +44 | **CONFIRMADA** |
| H7' ≥ 1 wallet 1+1 antes de entrar | 122 / 129 | 51/122 (42 %) | 12/129 (9 %) | +33 | < 0.0001 | 7.00 (3.50–14.03) | +32 | **CONFIRMADA** |
| H8 creador ≥ 15 % en el slot de creación | 18 / 233 | 14/18 (78 %) | 49/233 (21 %) | +57 | < 0.0001 | 13.14 (4.14–41.72) | +55 | **CONFIRMADA** |

H4' queda NO CONFIRMADA por 1 punto por debajo del umbral de 20, aunque p = 0.0036. **No se reinterpreta**: el criterio exige las cuatro condiciones.

**Resultado secundario** (retorno neto desde T_entry2, mediana con señal frente a sin señal; Mann-Whitney):

| | +10 min | +30 min | +60 min |
|---|---|---|---|
| H4' | −12.2 % vs −9.0 % (p 0.10) | −13.9 % vs −10.0 % (p 0.08) | −15.6 % vs −10.2 % (p 0.06) |
| H5 | −12.2 % vs −8.6 % (p 0.024) | −14.3 % vs −8.7 % (p 0.004) | −15.2 % vs −8.9 % (p 0.002) |
| H6' | −36.1 % vs −6.4 % (p < 0.001) | −37.1 % vs −7.6 % (p < 0.001) | −37.5 % vs −7.7 % (p < 0.001) |
| H7' | −13.9 % vs −8.6 % (p 0.015) | −16.8 % vs −8.6 % (p 0.001) | −16.8 % vs −8.6 % (p < 0.001) |
| H8 | −25.5 % vs −9.1 % (p 0.017) | −29.1 % vs −10.2 % (p 0.001) | −29.2 % vs −10.3 % (p < 0.001) |

**Lectura (obligatoria junto al veredicto)**:
- **Parte de la asociación primaria es mecánica.** Pump-y-caída se mide desde el precio **inicial**, no desde el de entrada. H6' (llenado alto) y H8 (compra grande del creador) miden precisamente cuánto ha subido ya el precio antes de T_entry2, así que el token está más cerca del "pico ≥ 2× el inicial" de la definición. Una parte del pump-y-caída ya ha ocurrido cuando se entra. H5 y H7' son menos directamente mecánicas, pero también se asocian a actividad temprana que mueve el precio.
- **La medida accionable es el retorno desde T_entry2.** Ahí la mediana es **negativa en todos los grupos**, con señal y sin ella, en los tres horizontes. Las señales confirmadas marcan tokens **peores para entrar** (retorno mediano más negativo), no mejores. Sirven como filtro de "evitar", no como señal de entrada. Ni siquiera el grupo sin señal tiene retorno mediano positivo (−6 % a −10 %).
- **La mayoría de los tokens muere enseguida.** En 166 de los 250 tokens con retorno evaluable, el retorno a +10 min es idéntico al de +60 min: no hay trades después de los 10 min. Las medianas reflejan sobre todo tokens que dejan de operar.

**POST HOC, NO PRE-REGISTRADO, SIN VEREDICTO** (añadido tras congelar; no decide nada):
- **(a) Con y sin mayhem**: la dirección se mantiene en los 203 no-mayhem en las cinco hipótesis. H4' sin mayhem sale +22 pts, p = 0.0012 (por encima de 20, pero es post hoc y no cambia el veredicto). En los 48 mayhem los grupos son minúsculos o vacíos: H6' y H8 tienen 0 tokens con señal, porque el llenado y la compra del creador se comportan distinto con reservas virtuales dinámicas.
- **(b) Retorno estratificado**:
  - **Los 36 tokens con comisión 0 bps en T_entry2 son todos mayhem** (los otros 12 mayhem tienen 125 bps). El estrato "0 bps" no mide un efecto de comisión: es un subconjunto de mayhem.
  - Retorno mediano: mayhem −16.8 % (0 bps: −56.7 %) frente a −9.9 % a −11.4 % en no-mayhem.
  - En los mayhem, el precio `virtual_quote / virtual_token` cambia fuera de los trades (sección 5.5 / piloto), así que **sus retornos no son fiables** como medida de la curva. En H5 aparecen valores extremos (+1162 %, +2306 %) con n = 1–2.
  - Dentro de no-mayhem y de 125 bps, el patrón de los cinco indicadores se repite (con señal, peor retorno).

**Hipótesis nuevas surgidas de aquí** (sin evaluar; necesitan otra ráfaga u otro día):
- **H9** (filtro de exclusión): los tokens sin ninguna de las señales H5/H6'/H7'/H8 tienen un retorno neto desde T_entry2 mejor que el resto. El pre-registro debe fijar si el objetivo es "mediana > 0" o solo "mejor que el resto".
- **H10** (supervivencia): la fracción de tokens sin trades después de +10 min desde T_entry2 difiere según las señales. Medir la muerte temprana directamente, no a través de la mediana.
- **Resultado desde el precio de entrada**: redefinir pump-y-caída con el pico y el cierre medidos respecto al precio en T_entry2, para quitar el componente mecánico. Es una definición nueva, que habría que pre-registrar antes de aplicarla a otra muestra.
- Tratar los tokens mayhem como población aparte (precio no comparable) en cualquier análisis futuro de retornos.

---

### Resultado de la validación 2 (réplica) (2026-10-01 15:21 UTC, `marxol entry2 --tramo validacion2`, ejecutada una sola vez)

**Alcance, antes que nada**: una sola franja de **~8 minutos** de otro día (creaciones del 2026-09-30 entre las 01:00:00 y las 01:07:58 UTC). Replica dentro de esa franja frente a la validación 1 (otra franja de 8 minutos, dos días antes). No generaliza más allá: otras horas, días o regímenes de mercado siguen sin probar. Una asociación aquí es correlación, no causalidad (principio 6).

**Ejecución**:
- Una sola ejecución, sin repetir. Salida completa en `resultados/validacion2_replica.txt`.
- Código del commit `039d4f0`. Antes de ejecutarlo se comprobó sobre la validación 1 (`scripts/verificar_replica.py`):
  - La primaria da n = 210.
  - Reproduce exactamente la robustez post hoc en poblaciones, diferencias, intervalos bootstrap y métricas de filtro. Fisher y OR coinciden hasta 1e-9.
  - Las salidas previas de `entry2` y de los dos scripts post hoc siguen idénticas byte a byte.
- **Datos**:
  - 283 tokens con T_entry2: no aplica la etiqueta de baja potencia. 0 tokens sin ventana de precio.
  - Condición de parada (grupo < 3 en H4'–H8 y H10, tres poblaciones): **no se cumple**.
  - Supervivencia n/e: 2 tokens en la primaria, 2 en la secundaria 1 y 1 en la secundaria 2.
- **Poblaciones**:
  - Primaria: 197 tokens de 197 creators, 39 pump-y-caída (20 %).
  - Secundaria 1: 283 tokens de 197 creators, 55 pump-y-caída.
  - Secundaria 2: 220 tokens de 174 creators, 42 pump-y-caída.
  - Un creator tenía dos tokens con T_entry2 en su slot mínimo. El orden de ejecución entre creaciones no se guarda, así que el empate se resolvió por mint lexicográfico, como estaba pre-registrado.
- Token de más trades en la primaria: `2Avh9SfjB6Fpk8EsfcA8HWGVzY5Edt5J4gqE1CzBxsdA` (1 166 trades en 5 min, sin empate).

**Veredicto primario: pump-y-caída en 1 h.** Columnas de la primaria; IC bootstrap simple de la diferencia entre corchetes.

| Hipótesis | n señal / sin señal | p&c con señal | p&c sin señal | Dif. [IC boot] | Fisher p | OR (IC95) | Sin el de más trades | Sec. 1 dif. | Sec. 2 dif. | Veredicto |
|---|---|---|---|---|---|---|---|---|---|---|
| H4' | 59 / 138 | 21/59 (36 %) | 18/138 (13 %) | +22.5 [+9.6, +36.9] | 0.0007 | 3.68 (1.78–7.63) | +21.4 | +23.4 | +26.5 | **CONFIRMADA EN RÉPLICA** |
| H5 | 119 / 78 | 31/119 (26 %) | 8/78 (10 %) | +15.8 [+5.2, +25.6] | 0.0063 | 3.08 (1.33–7.13) | +15.2 | +13.0 | +19.4 | **NO CONFIRMADA EN RÉPLICA** (dif. < 20) |
| H6' | 56 / 141 | 29/56 (52 %) | 10/141 (7 %) | +44.7 [+31.7, +58.0] | < 0.0001 | 14.07 (6.14–32.25) | +43.8 | +43.3 | +47.7 | **CONFIRMADA EN RÉPLICA** |
| H7' | 102 / 95 | 31/102 (30 %) | 8/95 (8 %) | +22.0 [+12.1, +32.5] | 0.0001 | 4.75 (2.05–10.98) | +21.3 | +19.8 | +26.8 | **CONFIRMADA EN RÉPLICA** |
| H8 | 15 / 182 | 7/15 (47 %) | 32/182 (18 %) | +29.1 [+3.0, +56.1] | 0.0134 | 4.10 (1.39–12.12) | +29.5 | +26.9 | +27.2 | **NO CONFIRMADA EN RÉPLICA** (Fisher ≥ 0.01) |

Secundarias, n señal / sin señal, Fisher p y OR (IC95):

| Hipótesis | Secundaria 1 | Secundaria 2 |
|---|---|---|
| H4' | 87/196, p < 0.0001, OR 3.97 (2.15–7.32), boot clúster [+11.3, +35.6] | 70/150, p < 0.0001, OR 4.95 (2.43–10.06), boot clúster [+11.5, +40.0] |
| H5 | 143/140, p 0.0067, OR 2.37 (1.27–4.40), boot clúster [+4.0, +22.8] | 143/77, p 0.0003, OR 5.03 (1.89–13.40), boot clúster [+9.9, +29.9] |
| H6' | 69/214, p < 0.0001, OR 11.20 (5.75–21.82), boot clúster [+31.0, +55.9] | 67/153, p < 0.0001, OR 22.81 (9.30–55.95), boot clúster [+35.2, +59.7] |
| H7' | 129/154, p < 0.0001, OR 3.74 (1.97–7.09), boot clúster [+9.1, +30.6] | 117/103, p < 0.0001, OR 9.06 (3.40–24.14), boot clúster [+16.8, +36.6] |
| H8 | 25/258, p 0.0028, OR 3.82 (1.63–8.97), boot clúster [+1.5, +56.4] | 23/197, p 0.0039, OR 3.97 (1.60–9.83), boot clúster [−1.1, +58.9] |

- **H5 y H8 no se confirman en réplica, y no se reinterpretan.**
  - En H5 la dirección se mantiene en las tres poblaciones (+13 a +19 pts), pero la diferencia baja de +35 pts (validación 1, un token por creator) a +15.8 pts, por debajo de 20.
  - En H8 la diferencia es grande (+29 pts) con solo 15 tokens con señal, y Fisher p = 0.0134 no pasa 0.01. El IC bootstrap en la secundaria 2 llega a −1.1.
- H4', que en la validación 1 había quedado NO CONFIRMADA por 1 punto, aquí pasa todas las condiciones. Es otro criterio (primaria de un token por creator) y otra franja: **no reabre** el veredicto congelado de la validación 1.

**H10 (supervivencia: ≥ 1 trade a más de 10 min de T_entry2), mismo criterio de réplica**:

| Señal | n (prim.) | Superv. con señal | Superv. sin señal | Dif. [IC boot] | Fisher p | OR (IC95) | Sin el de más trades | Sec. 1 / Sec. 2 dif. | Veredicto |
|---|---|---|---|---|---|---|---|---|---|
| H5 | 118 / 77 | 40/118 (34 %) | 15/77 (19 %) | +14.4 [+1.6, +26.9] | 0.034 | 2.12 (1.07–4.19) | +13.9 | +15.2 / +14.0 | **NO CONFIRMADA** |
| H6' | 56 / 139 | 21/56 (38 %) | 34/139 (24 %) | +13.0 [−2.1, +28.1] | 0.079 | 1.85 (0.95–3.60) | +11.9 | +14.9 / +13.9 | **NO CONFIRMADA** |
| H7' | 100 / 95 | 31/100 (31 %) | 24/95 (25 %) | +5.7 [−6.7, +18.3] | 0.43 | 1.33 (0.71–2.49) | +5.0 | +7.2 / +8.1 | **NO CONFIRMADA** |
| H8 | 15 / 180 | 4/15 (27 %) | 51/180 (28 %) | −1.7 [−23.5, +25.2] | 1.00 | 0.92 (0.28–3.02) | −1.3 | −2.3 / −3.3 | **NO CONFIRMADA** (dirección opuesta) |

H10 queda **NO CONFIRMADA EN RÉPLICA** con las cuatro señales. Con H5, H6' y H7' la dirección es la de la validación 1 (más supervivencia con señal), pero más débil. Con H8 no hay diferencia.

**Descriptivo (sin veredicto), solo sin mayhem** (secundaria 2, n = 220):
- **Control**: (a) = (b) en los 1 278 token-horizontes cuyo slot de salida tiene un solo trade (0 discrepancias).
- **Slots de salida con varios trades** en (b):
  - Reconstruidos: 36 a +5 s, 31 a +30 s, 11 a +5 min, 10 a +30 min y 10 a +60 min.
  - **No reconstruidos (n/e en (b))**: 0, 0, 3, 2 y 3 tokens (un slot cada uno). Son slots posteriores a la ventana temprana, donde `price_points` no guarda importes para encadenar reservas.
- **Retorno neto mediano · fracción > 0**, (a) / (b):

| Horizonte | Todos | H4' sí / no | H5 sí / no | H6' sí / no | H7' sí / no | H8 sí / no |
|---|---|---|---|---|---|---|
| +5 s | −4.4 · 10 % / −4.5 · 10 % | −7.0 / −4.1 (b: −8.6 / −4.1) | −5.6 / −3.0 | −11.7 / −3.1 (b: −12.3) | −3.1 · 15 % / −5.6 · 4 % | −16.1 / −4.3 (b: −11.9) |
| +30 s | −5.6 · 14 % / −5.6 · 13 % | −10.4 / −5.5 (b: −13.4) | −6.8 / −3.5 | −25.1 / −4.4 (b: −27.5) | −5.9 · 21 % / −5.6 · 5 % | −27.6 / −5.6 |
| +5 min | −7.4 · 5 % / −7.5 · 4 % | −17.9 / −5.6 | −12.1 / −5.1 (b: −14.8) | −33.2 / −5.6 (b: −35.2) | −12.0 / −5.6 | −32.0 / −6.1 |
| +30 min | −8.7 · 1 % / −8.7 · 1 % | −18.7 / −6.0 | −16.3 / −5.3 | −37.1 / −5.6 (b: −38.0) | −16.3 / −5.6 | −32.6 / −7.7 |
| +60 min | −8.7 · 1 % / −8.7 · 1 % | −20.7 / −7.0 | −16.7 / −5.3 | −37.1 / −5.6 (b: −38.9) | −16.7 / −5.6 | −32.6 / −7.8 |

  - Cuando no se indica (b), coincide con (a) o difiere en menos de 1 punto. La tabla por grupo y horizonte completa, con n, está en la salida guardada.
  - Igual que en la validación 1: la mediana es negativa en todos los grupos y horizontes, y los grupos con señal pierden más. En horizontes cortos, la fracción con retorno > 0 es mayor con H7' (15–21 % frente a 4–5 %).
- **Filtros "evitar"**, malo principal = pump-y-caída. El retorno del resto es la mediana a +30 min de los no evitados sin mayhem, (a); (b) da lo mismo en todos los filtros.

| Población | Filtro | Evitados / no evitados | Precisión | Cobertura | Sacrificio | Retorno del resto [IC boot] (n) |
|---|---|---|---|---|---|---|
| Primaria | A | 138 / 59 | 25 % | 87 % | 66 % | −4.7 % [−5.6, −3.0] (38) |
| Primaria | B | 99 / 98 | 33 % | 85 % | 42 % | −5.6 % [−6.4, −5.1] (75) |
| Primaria | C | 137 / 60 | 25 % | 87 % | 65 % | −5.1 % [−5.6, −3.0] (39) |
| Secundaria 1 | A | 181 / 102 | 24 % | 80 % | 60 % | −5.4 % [−5.6, −3.1] (55) |
| Secundaria 1 | B | 118 / 165 | 34 % | 73 % | 34 % | −5.6 % [−5.7, −5.1] (102) |
| Secundaria 1 | C | 174 / 109 | 24 % | 76 % | 58 % | −5.6 % [−5.6, −3.4] (58) |
| Secundaria 2 | A | 165 / 55 | 24 % | 95 % | 70 % | −5.4 % [−5.6, −3.1] (55) |
| Secundaria 2 | B | 118 / 102 | 34 % | 95 % | 44 % | −5.6 % [−5.8, −5.0] (102) |
| Secundaria 2 | C | 162 / 58 | 25 % | 95 % | 69 % | −5.6 % [−5.6, −3.4] (58) |

  - Ningún filtro deja un resto con retorno mediano positivo: todos los IC quedan por debajo de 0, como en la validación 1.
  - **El malo secundario (retorno a +30 < 0) degenera otra vez.** De 219 tokens sin mayhem evaluables (217 en (b)), solo **3 son "buenos"** en la secundaria 1. Por eso salen la precisión del 98 % y el sacrificio del 67–100 %, que con n = 3 no se pueden interpretar.

**Lectura**:
- De las cuatro señales confirmadas en la validación 1, **H6' y H7' se replican**, y H4' también pasa el criterio de réplica. **H5 y H8 no se replican**: mantienen la dirección, pero H5 con una diferencia más pequeña y H8 con muy pocos tokens con señal.
- H6' sigue siendo en parte mecánica (sección 8, lectura de la validación 1).
- H10 no se replica.
- Como filtro de entrada nada cambia: las señales marcan tokens peores, no un punto de entrada con retorno esperado positivo.
- Todo esto vale para dos franjas de 8 minutos de dos días distintos, no más.

---

### Resultado de la validación 3 (2026-10-08, `marxol entry2 --tramo validacion3`, ejecutada una sola vez)

**Alcance, antes que nada**: una sola franja de 8 minutos (2026-10-01, 09:00–09:08 UTC), con 75 tokens en la población. Es una tercera franja y otra hora del día, pero no generaliza a otros regímenes de mercado. Una asociación aquí es correlación, no causalidad (principio 6). Salida completa en `resultados/validacion3_d2.txt`. Código del commit `b65f9ae`.

**Datos**:
- Población: 75 tokens (sin mayhem, un token por creator).
- 1 token con retorno (a) a +30 min n/e, excluido.
- **Comparación: 74 tokens (46 pasan, 28 descartados).**
- Token de mayor peso: `FuaR8Pz4aS81vUbb2JKfnmEGQaRGsJwDotkxNA22pump` (550 trades en 5 min, sin empate).

**Resultado principal: retorno neto (a) desde T_entry2 a +30 min, pasan frente a descartados.**

| Criterio | Valor | Umbral | |
|---|---|---|---|
| 1. Tokens por grupo | pasan 46, descartados 28 | ≥ 20 | cumple |
| 2. Mann-Whitney unilateral (pasan > descartados) | U = 967.5, p = 0.000159 | < 0.01 | cumple |
| 3. Diferencia de medianas | pasan −5.6 %, descartados −24.7 % → **+19.0 pts** | ≥ 10 | cumple |
| 4. Sin el token de mayor peso | +19.1 pts | > 0 | cumple |
| 5. IC bootstrap 95 % (2 000 réplicas, semilla 20260930) | [+11.2, +27.2], 0 réplicas descartadas | excluye 0 | cumple |

**VEREDICTO: D2 SEPARA GRUPOS.**

**Secundario (descriptivo, sin criterio)**:
- D2 descarta el 39 % de la población (29 de 75).
- Retorno neto mediano y fracción > 0, versión (a). La versión (b) difiere en menos de 3.5 puntos en todos los casos:

| Horizonte | Pasan (n = 46) | Descartados (n = 29; 28 a +30 y +60 min) |
|---|---|---|
| +5 s | −2.5 % · 13 % | −8.0 % · 10 % |
| +30 s | −3.1 % · 4 % | −6.7 % · 28 % |
| +5 min | −5.4 % · 4 % | −19.7 % · 17 % |
| +30 min | −5.6 % · 2 % | −24.7 % · 11 % |
| +60 min | −5.7 % · 0 % | −24.8 % · 4 % |

  - En (b) quedan sin reconstruir 0, 0, 1, 3 y 6 slots de salida en los cinco horizontes.
- **Pump-y-caída en 1 h**: 13 de 29 descartados frente a 1 de 46 que pasan. **D2 captura 13 de los 14 p&c.** El primer 2x de los capturados cae antes de T_entry2 en 7, en su slot en 1 y después en 5.
- Limitar las identidades de creador a lo anterior a T_entry2 cambiaría H4' y H7' en 0 tokens.
- **Mayhem, aparte**: 75 tokens con T_entry2. D2 solo descarta 1 (30 tokens con un token por creator, 0 descartados). p&c 17/75. El retorno mediano de los que pasan es **−55 % a +30 min**, con un 23 % de tokens con retorno > 0. En los mayhem, D2 no actúa y el retorno es mucho peor. Recordar que su precio no es comparable (sección 8).

**Lectura (obligatoria junto al veredicto)**:
- D2, fijado con V1 y V2, separa grupos fuera de muestra en una tercera franja: los descartados pierden unos 19 puntos más a +30 min.
- **No es una señal de entrada.** Los que pasan siguen con retorno mediano negativo (−5.6 % a +30 min, solo un 2 % con retorno > 0). Es aproximadamente la comisión de ida y vuelta más la inactividad. Como en V1 y V2, el filtro mejora el "evitar", no genera un "entrar".
- Parte de la captura de p&c es lo ya ocurrido antes de entrar (7 de 13 con el primer 2x anterior a T_entry2), como en V2 (sección 8, segunda pasada del cribado). Por eso el resultado que decide es el retorno desde T_entry2, no el p&c.
- La población es pequeña (75), porque casi la mitad de los tokens con T_entry2 de esta franja son mayhem, y D2 no se aplica a ellos.

---

### H2 y H3

- **H2**: los operadores que migran a `sharing_config` (`MigrateBondingCurveCreatorEvent`) tienen un perfil de comportamiento distinto (más profesionalizados, más colaborativos, más propensos a repetir lanzamientos) que los que no lo hacen. Señal nueva, sin equivalente en el modelo de Pons/Robinhood Chain.
- **H3**: el criterio estadístico de financiación externa vs. ingreso propio del negocio usado en marXi para Robinhood Chain probablemente necesita rehacerse desde cero para Solana — el patrón de wallets financiadoras aquí puede no parecerse al de Pons (muchos operadores compran SOL directamente en CEX o usan bridges distintos). No asumir que el mismo modelo estadístico traslada sin validarlo con datos de Solana.

Al descartar o confirmar cada hipótesis con datos reales, documentarlo aquí con el mismo detalle independientemente del resultado (principio 6 de la sección 2).
