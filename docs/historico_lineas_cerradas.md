# Histórico de líneas cerradas y registros de ejecución

Contenido movido **literalmente** desde `CLAUDE.md` el 2026-10-21 para bajar su tamaño, sin resumir ni editar. En `CLAUDE.md` queda, en el mismo sitio, un resumen con el veredicto y la cifra clave que apunta aquí.
Las referencias internas ("arriba", "sección 8", "esta sección"…) se refieren a la posición original en `CLAUDE.md`.

---

## H1c — velocidad de las wallets 1 compra + 1 venta (descartada en calibración)

*Origen: `CLAUDE.md`, sección 8 (línea 330 antes del traslado).*

### H1c — velocidad de las wallets de 1 compra + 1 venta (pre-registrada 2026-09-28, antes de calcularla)

Motivo: en el piloto (tramo de calibración, 40 tokens) el 82 % de las wallets con compra y venta hacen exactamente 1 + 1, y el filtro de bots de H1 (n ≥ 3) no las evalúa. El patrón 1 + 1 no es sospechoso en sí: un humano que compra y toma beneficio una vez hace exactamente eso. Lo que se quiere distinguir es la velocidad.

**Hipótesis** **[P, provisional, a recalibrar]**: entre las wallets con exactamente 1 compra y 1 venta, un intervalo de permanencia corto entre ambas (candidato inicial: **< 20 s**) está asociado con el patrón de pump-y-caída de H1, más que la mera repetición del patrón 1 + 1 en sí.

**Variante operacional `h1c`** del criterio H1: igual que `v1` (no se toca el filtro de CV ni el 15 %), más una exclusión adicional en el paso 3. Una wallet (no creador) con exactamente 2 trades en la ventana, 1 compra y 1 venta, y `|t_venta − t_compra| < 20 s` **[P]** se excluye entera como sospechosa de ejecución automática. Después siguen los pasos 4–6 sin cambios.

**Definiciones para evaluarla** (fijadas antes de calcular):
- **Pump-y-caída** (1 h): pico ≥ 2× el precio inicial y caída > 30 % desde el pico al cierre (lo contrario de "sostenido" dentro de los que tuvieron pump).
- **Criterio direccional** (tramo de calibración; no se espera significación con 40 tokens):
  - (a) La fracción de pares rápidos (< 20 s) entre las wallets 1 + 1 es mayor en los tokens con pump-y-caída que en el resto. Se reporta también el número de wallets 1 + 1 por token en ambos grupos, para ver si separa más la velocidad que el mero recuento.
  - (b) Con `h1c`, la proporción de tokens H1 = true que son pump-y-caída baja respecto a `v1`, sin perder como positivos los tokens con H1b = true.
  - H1c queda **apoyada** en calibración si se cumplen (a) y (b); **descartada** si no se cumple (a); **no concluyente** en otro caso. Se documenta igual en los tres casos.
- Se reporta la distribución completa de intervalos 1 + 1 (percentiles e histograma), para poder recalibrar el umbral sin volver a descargar.

**Resultado en calibración (2026-09-28, los 40 tokens del piloto, sin descargas nuevas): H1c DESCARTADA según el criterio pre-registrado** (no se cumplen ni (a) ni (b)). Con 40 tokens no se esperaba significación; esto es tramo de calibración, no validación.

- **Distribución** de intervalos 1 + 1 (761 wallets ajenas al creador): p5 = 2 s, p10 = 3, p25 = 9, **mediana 31 s**, p75 = 79, p90 = 113, p95 = 123, máx 290. Histograma: < 1 s 1.1 %, 1–3 s 6.4 %, 3–5 s 6.6 %, 5–10 s 11.3 %, 10–20 s 13.1 % (acumulado **< 20 s: 38.5 %**), 20–30 s 10.4 %, 30–60 s 17.0 %, 60–120 s 27.9 %, 120–300 s 6.3 %. El máximo está acotado por la ventana de 5 min: una venta posterior no se ve y esa wallet cuenta como "solo compra". La distribución está censurada por la derecha.
- **Concentración**: un solo token (`CRHnzej9…`) aporta 382 de los 761 pares (50 %) y 173 de los 212 del tramo 60–120 s. Las cifras agregadas reflejan sobre todo ese token.
- **(a) no se cumple**: fracción de pares rápidos (< 20 s) en tokens pump-y-caída 37.6 % agregado (mediana por token 75.6 %) frente a 43.5 % (mediana 100 %) en el resto. Lo que separa los grupos es el **número** de wallets 1 + 1 (mediana 18 frente a 2), no su velocidad. **Fragilidad, a registrar sin redefinir el criterio a posteriori**: sin `CRHnzej9…` el agregado de pump-y-caída sube a 66 % (175/264) y la dirección se invierte; las medianas del "resto" se calculan sobre tokens con 1–2 pares. Es decir, (a) no es evidencia sólida en ningún sentido con esta muestra.
- **(b) no se cumple**: H1 = true baja de 21 a 14 tokens de 40, pero la proporción de pump-y-caída entre los positivos no baja (v1 13/21 = 62 %; h1c 9/14 = 64 %). Además, 4 tokens pump-y-caída pasan a H1 = false y **se pierde como positivo el único token "sostenido"** (`7AiKtyuD…`); el graduado se mantiene. Graduación: 1/14 frente a 0/26, Fisher p = 0.35.
- **Lectura**: filtrar por velocidad a 20 s elimina muchas wallets (293 de 761) pero no mejora la señal frente a H1b en esta muestra. No se adopta `h1c`; `v1` sigue siendo el criterio. La distribución queda como referencia para la recalibración.

---

## T_entry (1.5×), H4–H7 y propuestas post hoc (entradas tempranas)

*Origen: `CLAUDE.md`, sección 8 (línea 354 antes del traslado).*

### Punto de entrada hipotético T_entry (fijado 2026-09-29, antes de calcular H4–H7)

Objetivo: indicadores medibles **antes** de entrar en la curva, sin esperar a la graduación, que separen los tokens que acaban en pump-y-caída (definición de H1c: pico ≥ 2× y caída > 30 % al cierre de 1 h) del resto.

- **Definición** **[P]**: T_entry es el **primer `TradeEvent` de la ventana temprana (5 min) tras el cual el precio de la curva es ≥ 1.5× el precio inicial**. Precio = `virtual_quote_reserves / virtual_token_reserves` (el mismo de H1b). El precio inicial sale del `CreateEvent`. Los trades se recorren en **orden de ejecución**, reconstruido encadenando reservas (ver la corrección más abajo).
- **Corrección de implementación (2026-09-29, tras el primer cálculo; no cambia el criterio)**: el texto pre-registrado decía "orden de almacenamiento `(slot, timestamp, firma, índice)`". Ese orden **no es el de ejecución dentro de un slot**: la firma es un hash y ordenar por ella baraja los trades del mismo slot. Con ese orden, varios tokens "entraban" con un único trade pequeño que en realidad se había ejecutado después de la compra grande, y H5 salía 11/14 frente a 2/8, Fisher p = 0.026. **Ese resultado era un artefacto y queda anulado.** El orden correcto se reconstruye encadenando reservas: las reservas de token antes de cada trade (`virtual_token_reserves` después ± `token_amount`) son las de después del anterior, empezando por las del `CreateEvent`. Encadena el 100 % de los 2 831 trades de los 40 tokens, sin huecos. Umbrales, población y criterio no cambian.
- **Por qué precio y no % de llenado ni segundos**: el múltiplo de precio no tiene unidades, así que sirve igual para curvas SOL, USDC u otros quotes (sección 5.5), y no necesita `initial_real_token_reserves`, que no se guarda por token. En curva de producto constante, 1.5× equivale a haber vendido `V0·(1 − 1/√1.5)` tokens ≈ **24.8 % de las reservas reales estándar**, unos 6.7 SOL reales en curvas SOL (las 40 del piloto tienen `V0 = 1.073e15`). Un T_entry por tiempo fijo haría que H6 (velocidad) no fuera medible. Además, 1.5× queda por debajo del 2× de la condición de pump, así que la entrada es anterior al resultado que se quiere predecir.
- **Qué se ve antes de entrar**: el trade que cruza el umbral **y** todos los anteriores (se entra justo después de observarlo). Nada posterior.
- **Sin entrada**: token que no llega a 1.5× dentro de los 5 min. Se reporta aparte (cuántos y cuántos de ellos acaban igualmente en pump-y-caída tras los 5 min) y no entra en las tablas de H4–H7.
- **Limitación conocida**: todo token con pump-y-caída ha pasado por 1.5×, pero puede hacerlo después de los 5 min. Esos casos caen en "sin entrada".

### H4–H7: indicadores previos a T_entry (pre-registradas 2026-09-29, antes de calcularlas)

Datos: los 40 tokens del piloto, sin descargas nuevas. Es **tramo de calibración**: con n = 40 y una sola graduación no se espera significación. Se describe, no se concluye. No se toca el filtro de CV, el 15 % ni `v1`.

**Población de las tablas**: tokens con entrada. **Resultado**: pump-y-caída en 1 h (definición H1c, igual que en `marxol h1b`). Como secundario, los mismos cortes contra graduación en 1 h y pump sostenido en 1 h (H1b).

**Identidades de creador**: la misma unión que en el paso 2 de H1 (creator original, payer, cambios de creador, `creator` embebido en los trades).

- **H4 "dev vendió"**: alguna identidad de creador tiene un trade con `is_buy = false` en T_entry o antes. Se guardan el momento de la primera venta (s desde la creación) y el % del supply vendido (Σ `token_amount` de esas ventas / 10^15, el supply estándar; los 4 tokens mayhem se marcan porque su supply no está verificado). Señal **[P]**: al menos una venta, sin mínimo de %.
- **H5 "compra en el mismo slot que `create`"**: sin análisis de financiación (H3) no se puede saber qué wallets están ligadas al creador. Por eso se usa como proxy **cualquier wallet ajena al creador que compra en el slot de la creación** (bundle con wallets auxiliares o sniper del mismo slot: aquí no se distinguen). Señal H5 **[P]**: ≥ 1 wallet así. Se guardan el recuento de wallets y el % del supply comprado en ese slot. Como descriptivo aparte, **H5a**: el propio creador compra en el slot de creación (se espera casi universal por la compra inicial en la tx de `create`, y por eso no es la señal).
- **H6 "velocidad de llenado"**: segundos desde la creación hasta T_entry. Como T_entry está fijado en 1.5×, el progreso al llegar es el mismo para todos (~24.8 %) y la velocidad es solo el tiempo. Señal "llenado muy rápido" **[P]**: ≤ 10 s. Se guarda también la reserva real de quote al entrar (`virtual_quote − virtual_quote inicial`, en unidades del quote).
- **H7 "wallets 1 compra + 1 venta"**: número de wallets ajenas al creador cuyos trades hasta T_entry son exactamente 1 compra + 1 venta. Señal **[P]**: ≥ 3. **Aviso de circularidad**: H7 sale de observar estos mismos 40 tokens (ronda H1c: mediana 18 frente a 2 en la ventana completa). Evaluarla aquí solo dice si la separación sobrevive al restringir a trades anteriores a la entrada. **No puede apoyarla**: eso solo se puede hacer con tokens nuevos.

**Criterio de decisión** (común, fijado antes de mirar), sobre los tokens con entrada. p₁ = fracción de pump-y-caída con señal = true y p₀ con señal = false:
- **Candidata a validar con tokens nuevos**: p₁ − p₀ ≥ **20 puntos** **[P]** en la dirección esperada (señal → más pump-y-caída en las cuatro), con ≥ 3 tokens en cada grupo, y la misma dirección al quitar `CRHnzej9…`.
- **Descartada en calibración**: p₁ − p₀ ≤ 0 con los dos grupos ≥ 3 tokens.
- **No concluyente**: cualquier otro caso (0 < diferencia < 20 puntos, algún grupo con < 3 tokens, o cambio de signo sin `CRHnzej9…`).
- El test de Fisher y la odds ratio se reportan, pero no deciden (n = 40). Todo se reporta con y sin `CRHnzej9uJgwyiybSDqcndovAXGantb4v7e2hUihpump`, que aporta la mitad de los pares 1 + 1 de la ronda H1c. Se reporta la distribución de cada indicador, no solo el corte.

**Resultado en calibración (2026-09-29, 40 tokens del piloto, sin RPC, `marxol entry`, orden de ejecución corregido)**:

- **Cobertura**: 22 tokens con entrada (13 pump-y-caída) y 18 sin entrada. Ninguno de los 18 sin entrada es pump-y-caída ni llega a 2× en la hora. Llegar a 1.5× en 5 min es condición necesaria de todos los resultados de interés de la muestra: los 13 pump-y-caída, el graduado y el sostenido. Entre los que llegan, el 59 % acaba en pump-y-caída.
- **Hallazgo estructural: T_entry = 1.5× cae casi siempre en el slot de creación.** Pasa en 15 de 22 tokens (entrada a 0 s: 11 de 13 pump-y-caída y 4 de 9 del resto). Antes de entrar se ven 1–18 trades (mediana 3–4; 7 tokens con uno solo), y en esos 7 tokens la sola compra del creador en la tx de `create` ya pasa 1.5×. Consecuencias:
  - Un observador externo que reacciona al `CreateEvent` **no puede entrar en ese punto**: tendría que estar en el mismo slot, como un sniper o un bundle.
  - La información previa a la entrada es casi nula, así que H4, H6 y H7 **degeneran por diseño**. No se pueden leer como evidencia en ningún sentido.

| Hipótesis | Señal = true (p&c) | Señal = false (p&c) | Dif. | Fisher p (OR, IC95) | Sin CRHnzej9 | Veredicto pre-registrado |
|---|---|---|---|---|---|---|
| H4 dev vendió antes de entrar | 0/0 | 13/22 (59 %) | — | 1.00 | 0/0 vs 12/21 | No concluyente (degenerado: ningún dev vende antes de entrar) |
| H5 ≥ 1 wallet ajena compra en el slot de creación | 7/10 (70 %) | 6/12 (50 %) | +20 pts | 0.42 (2.33, 0.40–13.6) | 6/9 vs 6/12: +17 pts | **Candidata a validar, justo en el umbral** |
| H5a (descr.) el creador compra en el slot de creación | 13/22 | 0/0 | — | 1.00 | — | Universal en tokens con entrada, sin poder discriminante |
| H6 entrada en ≤ 10 s | 13/22 (59 %) | 0/0 | — | 1.00 | 12/21 vs 0/0 | No concluyente (degenerado: todas las entradas en ≤ 10 s) |
| H7 ≥ 3 wallets 1+1 antes de entrar | 0/0 | 13/22 (59 %) | — | 1.00 | 0/0 vs 12/21 | No concluyente (degenerado: máx. 1 par antes de entrar) |

Secundarios (graduación y pump sostenido en 1 h): con 1 caso de cada uno no aportan nada. Graduado y sostenido caen en H5 = false; Fisher p ≥ 0.33 en todos.

- **Nota sobre H5**: cumple el criterio con lo mínimo exigido: +20 puntos exactos, +17 sin CRHnzej9 (misma dirección) y grupos de 10 y 12 tokens. Una primera versión del script la marcaba "no concluyente" por redondeo en coma flotante (0.7 − 0.5 = 0.1999…); se corrigió con una tolerancia. **Es candidata débil**: un solo token que cambiase de grupo la dejaría por debajo del umbral, Fisher p = 0.42 y el IC de la odds ratio incluye 1 con holgura. Además, parte de la asociación puede ser mecánica: las compras del mismo slot son las que suben el precio.
- **Distribuciones** (tokens con entrada; p&c frente al resto; * = CRHnzej9):
  - Wallets ajenas en el slot de creación: [0×6, 1, 2, 3, 3, 3, 4, 4*] frente a [0×6, 2, 2, 4].
  - % del supply que compran: mediana 3.0 % frente a 0 %; máx. 19 %* frente a 16.7 %.
  - Segundos hasta T_entry: [0×11, 2, 2] frente a [0, 0, 0, 0, 1, 2, 3, 7, 10].
  - Wallets 1+1 antes de entrar: máx. 1 en ambos grupos.
  - H4: ninguna venta del dev antes de entrar.
- **Robustez de la variable de resultado**: el precio de cierre de H1b se ordena por `(timestamp, slot)` y no por encadenado de reservas. En 8 de 40 tokens hay varios trades en el último slot de la hora. En ninguno cambia la clasificación pump-y-caída con cualquier orden posible de ese slot.
- **Lectura**: el resultado principal es negativo sobre el diseño. **Un T_entry por múltiplo de precio no sirve**, porque cae en el slot de creación y no es alcanzable desde fuera. H4, H6 y H7 no han podido probarse: ni se apoyan ni se descartan. H5 pasa justo el criterio y queda como candidata débil para tokens nuevos.

### Propuestas nuevas surgidas de la ronda H4–H7 (2026-09-29; post hoc, NO evaluables sobre estos 40 tokens)

Se formularon después de ver los resultados de arriba. Evaluarlas sobre los mismos 40 sería circular, así que solo pueden probarse con tokens nuevos (tramo de validación, partición temporal). Todos los umbrales son **[P]**.

- **T_entry2 (entrada alcanzable)**: primer trade en un **slot posterior al de creación** con `t ≥ t0 + 10 s` **[P]**, sea cual sea el precio. Representa a un observador que reacciona al `CreateEvent` sin estar en su slot. Con esta entrada, H4, H6 y H7 tienen información previa real. Se re-pre-registran como **H4'/H6'/H7'** con los mismos cortes y el mismo criterio de decisión. H6' mide el múltiplo de precio alcanzado en T_entry2 (a más múltiplo, llenado más rápido), no el tiempo.
- **H8 "compra inicial grande del propio creador"**: % del supply comprado por identidades de creador en el slot de creación, con corte ≥ 15 % **[P]**. Motivo: en 7 de los 22 tokens con entrada, la compra del creador sola pasa 1.5×. Observado de pasada, sin tabla formal: 5 de esos 7 son pump-y-caída; de los otros 2, uno es el graduado. Por eso no se le da valor aquí.

---

## Validación 1: consumo real de RPC (registro de ejecución)

*Origen: `CLAUDE.md`, sección 8 (línea 526 antes del traslado).*

**Consumo real de RPC (Alchemy)**:
- Total **906 320 CU**: `windows` 740 720 (18 151 `getTransaction` + 367 `getSignaturesForAddress`) y `prices` 165 600 (3 784 + 356), en 22 658 peticiones.
- Es el **57 % de la estimación de 1.6M CU**, el 3 % del free tier mensual de 30M y el 36 % del tope de 2.5M fijado para la ronda.
- Por qué menos de lo estimado: la ventana temprana costó 53.9 `getTransaction` por token frente a los 106.5 del piloto. Los 40 del piloto eran más activos, y las tx fallidas se descartan sin pedirlas. El precio de la hora costó 11.2 por token y los listados de firmas 2.1 por token.
- Tiempo: ~40 min `windows` y ~25 min `prices`. Iba limitado por la latencia de cada petición (secuencial, ~5 req/s efectivas), no por las 25 req/s.
- Incidencias:
  - Alchemy devolvió **429** 8 veces, aun con ~5 req/s efectivas. Uno de ellos, en `getSignaturesForAddress`, cortó la ejecución. Se añadió un reintento con espera creciente para los 429 (`rpc::with_retry`), y un error al listar firmas deja ahora el token pendiente en vez de cortar la ejecución.
  - `rpc_usage.tokens` registraba 0 en una ejecución terminada en error (la fila de 14 677 `getTransaction` procesó 269 tokens). Se corrigió el código y esa fila.

---

## Robustez post hoc de la validación congelada (resultados)

*Origen: `CLAUDE.md`, sección 8 (línea 603 antes del traslado).*

### ROBUSTEZ POST HOC de la validación congelada (2026-09-30) — SIN VEREDICTO

**Post hoc, no pre-registrada.** Se hace después de ver los resultados de la validación. No confirma ni descarta nada y no toca los veredictos congelados. Las definiciones se fijaron antes de calcular y están en la sección 10. Solo `marxol.db`, sin red.
- **Cómo se reproduce**, desde la raíz del repo: `cargo build --release && python3 -B scripts/robustez_posthoc.py`. El script llama a `target/release/marxol entry2 --json` y cruza con `creations`, `completions` y `price_points`. Opciones `--marxol` y `--db`. La semilla (20260930) y las definiciones van en la cabecera. Se ejecutó dos veces con la salida idéntica byte a byte (2026-09-30).
- **Comprobaciones**: la población (b) reproduce exactamente las tablas congeladas. H6' es evaluable en los 251. La supervivencia es n/e en 1 token (`HpXRRsBW…`, completa la curva a los 252 s de T_entry2).

**n efectivo**:

| Subconjunto | Tokens | Creators | Bootstrap |
|---|---|---|---|
| (a) un token por creator | 210 | 210 | simple |
| (b) los 251 | 251 | 210 | clúster |
| (c) sin fábricas | 220 | 202 | clúster |
| (d) sin mayhem | 203 | 193 | clúster |

**1. Diferencia en pump-y-caída (puntos, señal − sin señal)**, con el IC bootstrap al 95 % entre corchetes. En ninguna réplica quedó un grupo vacío.

| | (a) 1 por creator | (b) 251 clúster | (c) sin fábricas | (d) sin mayhem |
|---|---|---|---|---|
| H4' | +21.4 [8.5, 35.1], p 0.0014 | +18.7 [5.5, 31.6], p 0.0036 | +20.0 [6.7, 33.7], p 0.0026 | +21.9 [8.9, 35.8], p 0.0012 |
| H5 | +35.0 [24.7, 45.1] | +29.7 [18.0, 40.7] | +33.4 [23.4, 43.8] | +34.8 [24.4, 44.8] |
| H6' | +46.7 [33.2, 59.7] | +45.0 [31.9, 58.5] | +46.2 [32.2, 59.6] | +47.6 [34.2, 60.7] |
| H7' | +34.7 [24.2, 45.6] | +32.5 [22.6, 42.7] | +35.0 [24.6, 45.3] | +35.2 [24.3, 46.3] |
| H8 | +56.4 [35.2, 75.0] | +56.7 [34.2, 76.2] | +56.5 [34.0, 74.3] | +56.7 [33.6, 77.0] |

- H5, H6', H7' y H8 tienen Fisher p < 0.0001 en los cuatro subconjuntos.
- n señal / sin señal: en (a) H4' 69/141, H5 118/92, H6' 65/145, H7' 106/104, H8 18/192. En (d) H4' 69/134, H5 123/80, H6' 65/138, H7' 104/99, H8 18/185.
- La OR sube al quitar la seudorreplicación o los mayhem. En (b) H5 es 5.79 (2.95–11.4). En (a) es 10.18 (4.12–25.2) y en (d) 12.58 (4.32–36.6).
- **Lectura**: la asociación de las cuatro confirmadas no depende de la seudorreplicación por creator, de las fábricas ni de los mayhem. Los IC por clúster excluyen 0 con holgura.
- H4' queda en torno al umbral de 20 puntos: +18.7 a +21.9 según el subconjunto, con el IC excluyendo 0 en todos. **No cambia su veredicto congelado (NO CONFIRMADA).**

**2. Supervivencia y retorno por grupo**:
- **Supervivencia** (≥ 1 trade a más de 10 min de T_entry2), en (b): 38–61 % con señal frente a 24–34 % sin señal. **Las señales se asocian a MÁS supervivencia**, no a menos, en los cuatro subconjuntos. Ejemplos: H5 43 % frente a 24 %; H8 61 % frente a 32 %. Los tokens sin señal suelen morir sin actividad; los que tienen señal siguen operando, pero caen.
- **Retorno neto sin mayhem** (mediana a +30 min en (b), con IC por clúster):

| Señal | Con señal | Sin señal |
|---|---|---|
| H4' | −13.9 % [−23.7, −10.5] | −9.5 % [−11.4, −8.2] |
| H5 | −14.6 % [−24.9, −11.4] | −8.3 % [−9.9, −6.2] |
| H6' | −37.1 % [−41.6, −33.0] | −7.3 % [−8.6, −6.3] |
| H7' | −16.8 % [−25.0, −12.0] | −8.6 % [−10.1, −6.5] |
| H8 | −29.1 % [−48.1, −18.3] | −10.0 % [−11.5, −8.5] |

  - +10 y +60 min van casi iguales a +30.
  - En (a), (c) y (d) las cifras son prácticamente las mismas.
  - **Todos los IC están por debajo de 0**, también en el grupo sin señal.

**3. Métricas de filtro "evitar"** (sobre los 251; "malo" = pump-y-caída). Precisión, cobertura y sacrificio se dan con todos / sin mayhem. El retorno del resto es la mediana a +30 de los no evitados, sin mayhem, con IC por clúster.

| Filtro | Evitados / no evitados | Precisión | Cobertura | Sacrificio | Retorno del resto (n) |
|---|---|---|---|---|---|
| A (cualquiera de H5/H6'/H7'/H8) | 154 / 97 | 38 / 38 % | 92 / 98 % | 51 / 56 % | −6.9 % [−8.6, −4.8] (67) |
| B (≥ 2 de las cuatro) | 112 / 139 | 45 / 45 % | 79 / 93 % | 33 / 41 % | −7.8 % [−9.0, −6.3] (93) |
| C (H5 o H7') | 150 / 101 | 39 / 39 % | 92 / 98 % | 49 / 53 % | −7.6 % [−8.9, −5.6] (71) |
| H5 | 125 / 126 | 40 / 40 % | 79 / 93 % | 40 / 49 % | −8.3 % [−9.9, −6.2] (80) |
| H6' | 65 / 186 | 58 / 58 % | 60 / 72 % | 14 / 18 % | −7.3 % [−8.6, −6.3] (137) |
| H7' | 122 / 129 | 42 / 43 % | 81 / 85 % | 38 / 39 % | −8.6 % [−10.1, −6.5] (99) |
| H8 | 18 / 233 | 78 / 78 % | 22 / 26 % | 2 / 3 % | −10.0 % [−11.5, −8.5] (184) |
| H4' | 73 / 178 | 38 / 41 % | 44 / 53 % | 24 / 27 % | −9.5 % [−11.4, −8.2] (133) |
| Sin filtrar | — | — | — | — | −10.7 % (202) |

- **"Malo" secundario (retorno a +30 < 0, sin mayhem) casi degenera**: 196 de los 202 evaluables son "malos" y solo **6 son "buenos"** (+0.1 %, +2.6 %, +7.8 %, +23 %, +77 %, +81 %). La precisión sale 95–100 % para cualquier filtro, y el sacrificio (5 de 6 buenos en A, B, C, H5 y H7'; 0 en H6' y H8) no se puede interpretar con n = 6.

**Lectura (post hoc, sin veredicto)**:
- Las cuatro señales confirmadas **sobreviven** a un token por creator, a quitar fábricas y a quitar mayhem, con IC por clúster lejos de 0. La seudorreplicación que avisó la exploración de operadores no explica el resultado.
- **Como filtro de entrada no bastan.** Ningún filtro deja un resto con retorno mediano positivo. El mejor (A) sube la mediana de −10.7 % a −6.9 %, con el IC entero por debajo de 0, a costa de descartar el 56 % de los tokens "buenos" (pump-y-caída = no, sin mayhem). La mediana de los no evitados sigue siendo negativa, porque la comisión y la inactividad ya la hunden. Las señales mejoran el "evitar", no generan un "entrar".
- **H6' y H8 son las más selectivas** (sacrificio 14–18 % y 2–3 %), pero siguen siendo en parte mecánicas (sección 8, lectura de la validación).
- La supervivencia va **al revés de lo que suponía H10**: los tokens con señal siguen operando más, pero pierden más. Si se pre-registra H10, hay que fijar la dirección con esto en cuenta. Esa dirección es post hoc, así que solo se puede comprobar con otra ráfaga.

---

## Curva de retorno desde T_entry2 y máxima subida (post hoc)

*Origen: `CLAUDE.md`, sección 8 (línea 672 antes del traslado).*

### EXPLORACIÓN POST HOC (2026-09-30): curva de retorno desde T_entry2 y máxima subida — SIN VEREDICTO

**Post hoc, solo descripción.** No propone horizontes ni umbrales. Aplica las reglas de la robustez post hoc: los 251 tokens de validación, métricas de retorno **solo sin mayhem** (203 tokens) y retorno neto calculado como en la validación congelada.
- **Reproducir**: `cargo build --release && python3 -B scripts/curva_retorno_posthoc.py` (sin red).
- **Control**: los retornos a +10, +30 y +60 min coinciden exactamente con los de `marxol entry2 --json` en los 203 tokens (0 discrepancias).
- **Limitación**: el precio de salida es el último punto con `timestamp ≤ salida`, ordenado por `(timestamp, slot)` como en `entry2::returns`. Si en ese segundo y slot hay varios trades, el orden interno no está determinado. Pasa en 52 tokens a +5 s, 44 a +15 s, 42 a +30 s, 30 a +1 min, 22 a +2 min, 32 a +5 min, 23 a +10 min, 16 a +30 min y 15 a +60 min. **Los horizontes cortos son los más afectados.**

**1. Retorno neto mediano y fracción con retorno > 0 según el horizonte** (sin mayhem):

| Horizonte | Todos (n = 203) | H5 sí / no | H6' sí / no | H7' sí / no | H8 sí / no |
|---|---|---|---|---|---|
| +5 s | −3.0 % · 15 % | −2.7 · 21 % / −3.1 · 5 % | −3.8 · 26 % / −2.7 · 9 % | −2.6 · 23 % / −3.3 · 6 % | −2.8 · 28 % / −3.0 · 14 % |
| +15 s | −4.3 % · 17 % | −4.3 · 23 % / −3.9 · 8 % | −13.7 · 25 % / −3.1 · 13 % | −3.9 · 26 % / −4.3 · 7 % | −4.7 · 28 % / −4.1 · 16 % |
| +30 s | −5.3 % · 16 % | −5.7 · 22 % / −4.4 · 6 % | −22.3 · 22 % / −3.6 · 13 % | −5.5 · 23 % / −4.6 · 8 % | −5.1 · 22 % / −5.3 · 15 % |
| +1 min | −6.2 % · 14 % | −7.2 · 20 % / −4.5 · 5 % | −26.5 · 17 % / −4.4 · 13 % | −7.3 · 21 % / −5.3 · 7 % | −6.5 · 17 % / −6.2 · 14 % |
| +2 min | −7.6 % · 11 % | −9.5 · 15 % / −5.7 · 5 % | −30.0 · 12 % / −5.7 · 10 % | −9.2 · 16 % / −6.3 · 5 % | −9.1 · 17 % / −7.6 · 10 % |
| +5 min | −8.9 % · 7 % | −11.4 · 10 % / −7.3 · 4 % | −33.3 · 2 % / −5.8 · 10 % | −11.6 · 13 % / −7.8 · 2 % | −13.1 · 0 % / −8.6 · 8 % |
| +10 min | −9.9 % · 4 % | −12.2 · 7 % / −8.0 · 1 % | −36.1 · 0 % / −6.2 · 7 % | −13.3 · 8 % / −8.5 · 1 % | −25.5 · 0 % / −9.0 · 5 % |
| +30 min | −10.7 % · 3 % | −14.6 · 4 % / −8.3 · 1 % | −37.1 · 0 % / −7.3 · 4 % | −16.8 · 5 % / −8.6 · 1 % | −29.1 · 0 % / −10.0 · 3 % |
| +60 min | −11.4 % · 2 % | −16.4 · 3 % / −8.6 · 0 % | −37.5 · 0 % / −7.6 · 3 % | −18.3 · 4 % / −8.6 · 0 % | −29.2 · 0 % / −10.2 · 2 % |

- Cada celda da la mediana y la fracción de tokens con retorno > 0.
- n por grupo (sí / no): H5 123/80, H6' 65/138, H7' 104/99, H8 18/185. A +30 y +60 min se pierde 1 token (curva completada).
- La mediana es negativa en todos los horizontes y grupos, y se hace más negativa con el tiempo. A +5 s ya es −3 %, del orden de la comisión de ida y vuelta (125 bps × 2 ≈ 2.5 %).
- **En horizontes cortos la fracción con retorno > 0 es MAYOR con señal que sin ella** en H5, H6', H7' y H8, al revés que la mediana. Ejemplo: H5 a +5 s, 21 % frente a 5 %. Los tokens con señal tienen más dispersión, con subidas y caídas, mientras que los que no tienen señal se quedan quietos y pierden la comisión. Con señal, esa fracción cae a ≤ 5 % en +30 min.

**2. Máxima subida alcanzable tras T_entry2** (hasta +60 min, sin mayhem; 202 evaluables, n/e `5wXHg8Ed…` por completar la curva). Es el precio máximo de la curva en el slot de T_entry2 o después, frente al precio de entrada. Es una **cota a posteriori**: supone vender justo en el máximo, sin impacto de precio.

| | n | Múltiplo bruto p25 / p50 / p75 / p90 (máx) | Neto en el máximo p50 / p75 / p90 | Neto > 0 | s hasta el máximo p50 / p75 / p90 | Máx. en el slot de T_entry2 |
|---|---|---|---|---|---|---|
| Todos | 202 | 0.974 / 1.001 / 1.118 / 1.629 (3.63) | −2.4 / +8.7 / +58.8 % | 69 (34 %) | 1 / 31 / 159 | 94 (47 %) |
| H5 sí / no | 122 / 80 | 1.013 / 0.997 (p50) | −1.2 / −3.0 % (p50) | 47 % / 15 % | 3 / 0 (p50) | 48 / 46 |
| H6' sí / no | 65 / 137 | 1.054 / 1.000 | +2.7 / −2.5 % | 57 % / 23 % | 5 / 0 | 19 / 75 |
| H7' sí / no | 103 / 99 | 1.027 / 0.999 | +0.2 / −2.8 % | 50 % / 17 % | 5 / 0 | 36 / 58 |
| H8 sí / no | 18 / 184 | 1.001 / 1.000 | −2.4 / −2.5 % | 50 % / 33 % | 2 / 1 | 8 / 86 |

- **Cuándo ocurre el máximo** (todos): 0 s 99, 1–5 s 17, 6–15 s 20, 16–30 s 12, 31–60 s 12, 1–2 min 14, 2–5 min 13, 5–10 min 7, 10–30 min 8, 30–60 min 0.
- **La mitad de los máximos está en el mismo segundo que T_entry2** (94 en su propio slot). Muchas veces es el precio justo después del propio trade de entrada. Un observador que reacciona a ese trade difícilmente lo captura, así que la cota es optimista.
- En al menos el 25 % de los tokens el máximo posterior queda **por debajo** del precio de entrada: el precio no vuelve a subir nunca.
- **Los tokens con señal tienen más recorrido al alza** (p90 del múltiplo bruto de 1.87–1.97× con señal, frente a 1.16–1.25× en H5, H6' y H7' sin señal), además de peor mediana final. Es coherente con más volatilidad, no con mejor entrada. Es descriptivo: la cota se calcula a posteriori y no dice cómo capturarla.

---

## Validación 2: ejecución, fases 1 y 2 (registro de ejecución)

*Origen: `CLAUDE.md`, sección 8 (línea 756 antes del traslado).*

**Ejecución, fase 1: indexado del tramo (2026-09-30, `marxol index-tramo --from 1790730000 --count 400 --provider alchemy --max-rps 10 --max-requests 3000`)**:
- Solo se recorrieron las listas de firmas de `mint-authority` hasta S2: 29 páginas, 28 545 firmas con `blockTime ≥ 1790730000`. Las 1 629 fallidas se descartaron por `err`.
- Se pidió `getTransaction` solo de las 401 firmas exitosas necesarias; 1 no trae `CreateEvent`.
- **S2 = slot 451 810 013** (blockTime 1790730000 = 01:00:00 UTC exacto). La última firma anterior está en el slot 451 810 008 (blockTime 1790729999).
- **Creación n.º 400** en orden `(slot, mint)`: **slot 451 811 800**, mint `H6V4cjHgJ1j8rUbBzfVceevkx7aggaJTyBwyM5LZpump`, timestamp 1790730478. El slot frontera tiene 1 sola creación, así que no hizo falta desempatar, y no se indexó ninguna creación fuera del tramo.
- **Tramo validación 2 = creaciones con slot en [451 810 013, 451 811 800]**: exactamente 400, entre las 01:00:00 y las 01:07:58 UTC.
- **Consumo: 17 440 CU** (401 `getTransaction` + 35 `getSignaturesForAddress`, 6 de ellas reintentos). Es el 0.9 % del tope de 2M CU.
- Aún no se ha descargado ninguna ventana ni precio del tramo, y no se ha mirado ningún resultado.

**Ejecución, fase 2: descargas y chequeo (2026-09-30, sin mirar el resultado)**:
- `windows --limit 400 --provider alchemy --max-rps 10 --max-requests 40000`: **400 ventanas completas, 0 incompletas**, 0 errores ni 429. Consumo: 27 315 `getTransaction` + 518 `getSignaturesForAddress` = **1 113 320 CU**, en ~60 min.
- `prices --limit 400 … --max-requests 21731`: **400 ventanas de precio completas (65 min), 0 incompletas**, 0 errores. Consumo: 5 174 + 422 = **223 840 CU**, en ~25 min.
- Los 18 667 trades de la ventana temprana tienen su punto de precio, y las 400 creaciones tienen precio inicial.
- **Total de la validación 2: 1 354 600 CU** (68 % del tope de 2M). La estimación era de ~1.1M: salió un 23 % por encima, por descargar tarde.
- `entry2 --tramo validacion2` (slots [451 810 013, 451 811 800]) solo admite `--check` hasta que se implemente el veredicto de réplica. Tras la ingesta, la validación 1 sigue idéntica byte a byte (las mismas cinco salidas de la sección 11).
- **`entry2 --check --tramo validacion2`** (sin resultado):
  - 283 tokens con T_entry2 y 117 sin él. Como 283 ≥ 250, **no aplica la etiqueta de baja potencia**.
  - Comisión conocida en 283/283. Orden de ejecución completo en 282/283: la excepción es `DWJzFDAx…` (543 trades en la ventana, T_entry2 a 10 s con 16 trades antes). Se anota; no cambia ninguna definición.
  - Grupos señal / sin señal:

| Población | n (creators) | H4' | H5 | H6' | H7' | H8 |
|---|---|---|---|---|---|---|
| Primaria (1 por creator) | 197 (197) | 59 / 138 | 119 / 78 | 56 / 141 | 102 / 95 | 15 / 182 |
| Secundaria 1 (todos) | 283 (197) | 87 / 196 | 143 / 140 | 69 / 214 | 129 / 154 | 25 / 258 |
| Secundaria 2 (sin mayhem) | 220 (174) | 70 / 150 | 143 / 77 | 67 / 153 | 117 / 103 | 23 / 197 |

  - **Ningún grupo < 3**, así que no hay condición de parada en H4'–H8.
  - En H10 los grupos son los mismos menos los n/e de supervivencia. Esos n/e solo se ven al calcular, y la condición de parada se vuelve a comprobar entonces.
  - H6' es evaluable en los 283 tokens.
  - Medianas en T_entry2: 13 s, 6 trades antes y múltiplo de precio 1.067×.

---

## Cribado post hoc de descarte (dos pasadas)

*Origen: `CLAUDE.md`, sección 8 (línea 902 antes del traslado).*

### Cribado de descarte y repetición de creadores (2026-10-01) — POST HOC, SIN PRE-REGISTRO

Solo descriptivo, sin contrastes. Script: `scripts/cribado_posthoc.py`; salida: `resultados/cribado_posthoc.txt`. Mayhem siempre aparte.
- **D = H4' o H6' o H7'**, en V2 primaria sin mayhem (n = 174; es la cifra honesta, porque las señales se eligieron con V1): descarta el 68 %, captura **33/33 p&c** (precisión 28 %) y descarta 2 de 3 "buenos". En V1 sin mayhem: 64 % descartado y 51/53 p&c.
- **D2 = ≥ 2 de 3**, en V2 sin mayhem: descarta el 43 %, captura 30/33 p&c (precisión 41 %) y descarta 0 de 3 buenos. Con 3 buenos no hay potencia para medir la pérdida de buenos.
- Los que pasan cualquiera de los dos filtros siguen con retorno mediano negativo (−5.6 % a −5.9 % a +30 min).
- **Repetición de creadores**: solo **2 creators** aparecen en las dos ráfagas, los dos casi solo mayhem. El cribado "creador con p&c en V1" marca 0 tokens sin mayhem de V2 y no añade nada a D.
- Dentro de una ráfaga de 8 min no se puede cribar por creator sin fuga: el p&c de un token anterior solo se conoce 1 h después.

### Cribado, segunda pasada (2026-10-01) — POST HOC, SIN PRE-REGISTRO

Script: `scripts/cribado_posthoc2.py`; salida: `resultados/cribado_posthoc2.txt`. Población primaria (V1 210, V2 197), mayhem aparte. Las cifras siguientes son de V2 sin mayhem (n = 174, 33 p&c, 3 "buenos").
- **Sin H6', D2 no se sostiene.** D2b (H4' y H7') captura 18/33 p&c, frente a 30/33 de D2. Dany (H4' o H7') captura 32/33, pero descarta el 65 % y 2 de los 3 buenos. H6' sola captura 29/33 con precisión del 52 %. H6' es la que más pesa en D2.
- **La mayoría del p&c ya ha ocurrido al entrar.** En **27 de 33** p&c el primer 2x es anterior a T_entry2 (V1: 34 de 53), y en 18 el pico de la hora también. Entre los 6 posteriores, D2 captura 3 y Dany 5.
- Esto no quita valor al filtro para quien entra: los descartados por D2 pierden −31 % de mediana a +30 min, frente a −5.9 % de los que pasan. Pero el p&c medido desde el precio inicial mezcla lo ya ocurrido con lo evitable. El resultado relevante es el retorno desde T_entry2.
- **Identidades de creador con información posterior a T_entry2**: `creator_identities` incluye cambios de creator y el creator embebido en trades posteriores. Afecta a 4/197 (V2) y 3/210 (V1) tokens, pero no cambia ninguna H4' ni H7'. No se ha corregido.
- V2 también se ha mirado para elegir variantes: cualquier filtro que se congele necesita una ráfaga nueva.

---

## Validación 3: ejecución, fases 1 y 2 (registro de ejecución)

*Origen: `CLAUDE.md`, sección 8 (línea 997 antes del traslado).*

#### Ejecución, fase 1: indexado del tramo (2026-10-06, antes de descargar ventanas ni precios)

- **Prueba previa** (adenda, punto 5): `index-tramo --from 1790845200 --until 1790845800 --count 20 --provider alchemy --max-rps 10 --max-requests 1000`. Indexó las 20 creaciones más antiguas del tramo (slots 452 240 867–452 241 084), dejó el tramo registrado como incompleto (`tramo_index.complete = 0`) y costó 13 440 CU.
  - Una ejecución anterior, sin `MARXOL_RPC_URL`, falló al construir el cliente y no llegó a enviar ninguna petición. Aun así dejó en `rpc_usage` una fila de 1 `getSignaturesForAddress` (40 CU, `run_at 1791272510`), porque el presupuesto cuenta la petición antes de enviarla.
- **Ejecución completa**: la misma con `--count 400 --max-requests 1500`.
  - Listado de 271 páginas: 259 firmas con blockTime en [1790845200, 1790845800), 20 de ellas fallidas.
  - 219 `getTransaction` nuevas, 1 sin `CreateEvent`.
  - `tramo completo: 238 creaciones` en el listado con margen, slots [452 240 867, 452 243 116]. **0 creaciones con timestamp del `CreateEvent` ≠ blockTime de su firma**, así que no hay diferencias en el borde.
  - Consumo: 21 640 CU (219 `getTransaction` + 322 `getSignaturesForAddress`; las páginas de más son reintentos). El listado se repite entero en cada ejecución, porque las páginas no se guardan.
- **S3 = slot 452 240 867** (blockTime 1790845201). La última firma anterior está en el slot 452 240 861 (blockTime 1790845199).
- **Tramo validación 3 = 196 creaciones con timestamp en [1790845200, 1790845680)**:
  - **Slots [452 240 867, 452 242 654]**, timestamps de 1790845201 a 1790845678.
  - Las 42 creaciones del margen (timestamp en [1790845680, 1790845800), slots 452 242 691–452 243 116) quedan **fuera** del tramo.
  - El último token del tramo tiene timestamp 1790845678 y el primero del margen 1790845687: no hay empates en el corte.
  - Este rango no se amplía ni se cambia.
- **Actividad**: 196 creaciones en 8 minutos, frente a 400 en la validación 2 y 337 en la validación 1. Se anota como hecho; no cambia ningún criterio.

#### Ejecución, fase 2: descargas y chequeo (2026-10-08, sin mirar el resultado)

- `windows --limit 196 --provider alchemy --max-rps 10 --max-requests 25000`: **196 ventanas completas, 0 incompletas**, 11 445 trades. La cola va por timestamp, así que tomó exactamente los 196 del tramo y ninguno de los 42 del margen. Consumo: 17 122 `getTransaction` + 221 `getSignaturesForAddress` = **693 720 CU**.
- `prices --limit 196 … --max-requests 8000`: **196 ventanas de precio completas (65 min), 0 incompletas**, 5 732 puntos nuevos. Consumo: 6 172 + 205 = **255 080 CU**.
- Ningún token necesitó reintentos de la adenda (punto 3). **Total de la validación 3: 970 440 CU** (indexado 21 640 + 948 800), por debajo de la estimación de ~1.4M CU para un tramo de 400 tokens. Se descargó 7 días después de las creaciones.
- **`entry2 --check --tramo validacion3`** (sin ningún retorno):
  - Indexado completo hasta blockTime 1790845800; 0 diferencias de borde.
  - 196 creaciones: 38 sin T_entry2 y 158 con T_entry2, de las que **83 son no-mayhem y 75 mayhem**. Es una proporción de mayhem mucho mayor que en V1 (48/251) y V2 (63/283). Se anota como hecho; no cambia ningún criterio.
  - **Población: 75 tokens de 75 creators** (sin mayhem, un token por creator), sin empates de slot. **D2: 29 descartados, 46 pasan**, 0 no evaluables (adenda, punto 1) y 0 sin datos (adenda, punto 3).
  - Limitar las identidades de creador a lo anterior a T_entry2 cambiaría H4' en 0 tokens y H7' en 0.
  - Mayhem, aparte: 75 tokens con T_entry2 (30 con un token por creator); D2 descarta 1 y pasan 74.
  - Adenda, punto 7: 0 creators fuera.
  - Ambos grupos ≥ 20 antes de excluir los n/e del retorno a +30 min. Esos n/e solo se ven al calcular el veredicto, así que el criterio 1 se vuelve a comprobar entonces.

---

## Piloto: estado de datos, descriptivo, H1b, ronda H4–H7 y coste (registros de ejecución)

*Origen: `CLAUDE.md`, sección 10 (línea 1103 antes del traslado).*

   **Estado de los datos (2026-09-28)**: 377 creaciones indexadas (14:03–14:11 UTC), 40 con ventana completa (las 40 más antiguas), 0 incompletas, 0 errores. Es un piloto, no hay masa crítica. Coste de una ventana: 106.5 firmas de media (máx. 869), es decir, ~107 `getTransaction` por token. Con el RPC público, 40 ventanas tardaron 55 min (~83 s por token): las 377 llevarían ~9 h. Con Alchemy serían ≈ 4.3k CU por token (107 × 40 CU), sin contar las páginas de `getSignaturesForAddress`.
   **Descriptivo del piloto (40 tokens, sin H1b, sin valor estadístico)**: H1 = true en 21 de 40 (52 %). Wallets orgánicas = 0 en 14 tokens (7 sin ningún trade ajeno al creador). Exclusiones sobre 2 553 trades: 130 del creador, 71 de 13 wallets bot, 35 de ballena. **Hallazgo a vigilar**: de las 942 wallets que compran y venden en la ventana (antes de exclusiones), **775 (82 %) hacen exactamente 1 compra + 1 venta**, un patrón que el filtro de bots no puede evaluar (exige n ≥ 3). El filtro actual apenas actúa, y lo que cuenta como "orgánico" está dominado por ese patrón. Candidatos para la recalibración (no aplicados; se decide con la muestra de calibración, no con estos 40): tiempo de tenencia compra→venta, wallets que operan en muchos mints a la vez, financiación común. Una tasa de positivos del 52 % también anticipa baja especificidad frente a H1b.
   **H1b sobre el piloto (1 h, 40 tokens, 2026-09-28)**: 40 ventanas de precio completas, 386 puntos nuevos además de los 2 831 de la ventana temprana (10 min con el RPC público: tras los 5 primeros minutos casi no hay actividad). Estos 40 son los más antiguos, es decir, **tramo de calibración**: no pueden reutilizarse como validación.
   - Graduación en 1 h: 1 de 40 (2.5 %), con H1 = true. Tabla H1 × graduó: 1/20 vs. 0/19; Fisher p = 1.00; OR 2.85 (IC95 0.11–74). Sin potencia: con una sola graduación, ningún resultado podría ser significativo.
   - Pump sostenido en 1 h: 1 de 39 evaluables (el graduado es n/e), el mismo token. Fisher p = 1.00.
   - **Descriptivo, no es una prueba de H1**: pico ≥ 2× en 15 de 21 tokens con H1 = true y en 0 de 19 con H1 = false; pero de esos 15, 13 caen más del 30 % antes de la hora (caída mediana 67 %), 1 aguanta (el único "sostenido") y 1 gradúa (sostenido n/e). H1 detecta bien "hubo pump en los primeros minutos", pero no "el pump aguanta". Parte de esa asociación es mecánica: los mismos trades que cuentan como wallets orgánicas son los que mueven el precio.
   - Masa crítica: con una tasa de graduación en 1 h del orden del 2–3 %, hacen falta del orden de 1 000–1 500 tokens de validación para tener ~30 graduaciones.
   **Ronda H4–H7 (2026-09-29, calibración, sin RPC)**: ver sección 8. T_entry = 1.5× cae en el slot de creación en 15 de 22 tokens, así que H4, H6 y H7 degeneran. H5 queda como candidata débil, justo en el umbral. Se proponen T_entry2 (primer trade en un slot posterior y a ≥ 10 s de la creación) y H8 (compra inicial del creador ≥ 15 % del supply), solo para tokens nuevos.
   **Coste de descargar más ventanas** (medido en el piloto): ventana de 5 min = 106.5 firmas por token de media (máx. 869); hora de precio = 10.4 firmas más. En total ≈ 117 `getTransaction` + 1–2 páginas de `getSignaturesForAddress` por token.
   - Alchemy: ≈ 4.8k CU por token. Las **337 pendientes ≈ 1.6M CU** (5 % del free tier mensual de 30M) y ≈ 40k peticiones: ~27 min a 25 req/s, ~1.7 h con la pausa de 150 ms por defecto. Frente a ~9 h con el RPC público.
   - Masa crítica de 1 500 tokens: ≈ 7.2M CU (24 % del mes).
   - Helius (10 créditos por llamada de archivo): ≈ 1.2k créditos por token → 337 ≈ 400k (40 % del mes gratis); 1 500 ≈ 1.8M, **no cabe** en el free tier de 1M.
   - Salvedad: cuanto más tiempo pasa desde la creación, más firmas posteriores hay que paginar hacia atrás en la bonding curve hasta llegar a la creación. Descargar tarde encarece los tokens que siguieron activos.

---

## Reglas de salida corta (post hoc)

*Origen: `CLAUDE.md`, sección 10 (línea 1149 antes del traslado).*

   **Reglas de salida corta (2026-10-09) — post hoc, sin pre-registro** (`scripts/salidas_posthoc.py` → `resultados/salidas_posthoc.txt`, sin red). Se probaron 1 920 combinaciones (3 648 estimaciones): 27 reglas TP/SL/tiempo × 4 latencias × 3 grupos de D2 × V1/V2/V3/juntas, más horizontes fijos y mayhem aparte.
   - **La comisión domina.** Cuesta ≈ 2.7 pts por ida y vuelta. Con latencia ≥ 2 s, la mediana del movimiento bruto a +5, +30 y +120 s es **0.0 %** (el token mediano no opera), así que la mediana neta es −2.5 % (solo comisión). Con L = 0, el bruto mediano a +2 min es −3.9 % y en los descartados por D2 llega a −16.6 %.
   - **Juntas sin mayhem**: ninguna regla da mediana neta positiva, con ninguna latencia ni grupo. Las 15 estimaciones positivas sin mayhem (de 2 592) son todas de **V3, descartados por D2** (n = 28–29, TP 10 %, IC que incluye 0). En juntas, las mismas reglas dan −3.1 % a −4.6 %. Es el patrón esperable por comparaciones múltiples.
   - **No salibles** (ningún trade antes del límite de tiempo; en una curva se puede vender igual, pagando la comisión): juntas, 22–31 % con L = 0 y 27–45 % con L = 5–10 s. Entre los que pasan D2, 37–59 %.
   - Medias netas positivas aisladas en "pasan D2" (+1 % a +7 %), con IC que cruzan 0 y medianas negativas: son colas de pocos tokens, no una regla.

---

## Tokens maduros: fase 0, fase 0b, vida, señales y cota (post hoc)

*Origen: `CLAUDE.md`, sección 10 (línea 1154 antes del traslado).*

   **Tokens maduros ("segunda vida"), fase 0: viabilidad de datos (2026-10-09) — post hoc, sin pre-registro** (`scripts/madurez_fase0.py` → `resultados/madurez_fase0.txt`). Solo la API pública de GeckoTerminal: 131 llamadas, sin CU.
   - **Límite real de la API: unas 10 llamadas/min** (19 errores 429 incluso a ese ritmo). Los listados (top por DEX, new pools) dan como mucho 200 pools: "top" son supervivientes de 24 h y "new" cubre unos minutos. **No se puede listar el universo de un periodo pasado.** Las velas gratuitas no van más allá de ~2026-03-14: antes, la API devuelve 401.
   - **Sesgo de supervivencia**: los 16 graduados de `marxol.db` siguen apareciendo por dirección (16/16, 15 con velas), aunque estén muertos (4 con liquidez < $1 000). Ninguno sale en los listados. Su historia es de unas 140 h, con el **95 % de las horas sin vela**.
   - **Tasa base de segunda vida**: con high/low horarios es del 67–100 %, por mechas de un solo trade. Con cierres es del 0 % en los graduados de `marxol.db` y del 13–68 % en los supervivientes de los listados. La definición final se fija después, sin mirar indicadores.
   - **Whales gratis**: 300 trades por pool (unos 3 min en un pool activo), con wallet. Número de holders y distribución top 10/11–20/21–40, sin direcciones ni historia. 1 000 tokens de velas ≈ 1 450 llamadas ≈ 2.4 h a 10/min. El universo necesitaría la vía on-chain de migraciones (≈ 0.4–1.3 M CU/mes, estimado y no ejecutado).
   **Tokens maduros, fase 0b (2026-10-20) — post hoc, sin pre-registro** (`scripts/madurez_fase0b.py` → `resultados/madurez_fase0b.txt`; datos en `madurez.db`):
   - Las migraciones se listan con `getSignaturesForAddress` sobre `Global.withdraw_authority` (`39azUYFW…`): cuenta writable de `migrate`/`migrate_v2`, presente en las 3 tx conocidas comprobadas. El pool es siempre el PDA `["pool", 0u16, pool_authority, mint, quote]` de pump_amm (16/16 locales y 1 068/1 068 del día).
   - **2026-08-20 UTC: 1 068 migraciones** (1 036 SOL, 32 USDC), de 173 715 firmas del día en esa dirección: el 99 % son tx fallidas. Coste: 1 329 `getTransaction` (53 160 CU) más el listado, que desde el 2026-10-20 fueron 760 páginas (59 200 CU con reintentos). Total de la fase: 114 450 CU y 57 llamadas a GeckoTerminal.
   - Muestra de 40 pools (semilla 20260930), unos 61 días después: **muertos 33/40 (82 %, IC95 68–91 %)** (sin velas con volumen en 7 días). **Segunda vida 1/40 (2 %, IC95 0–13 %)** (cierre horario ≤ 20 % del máximo y después ≥ 2× ese mínimo). Solo 2 de 40 caen ≥ 80 %: la mayoría deja de operar en pocas horas (mediana de 6.5 velas horarias por pool), así que "muerto" es abandono, no desplome.
   - Un solo día, n = 40 y precio de GeckoTerminal sin verificar en cadena: es la tasa base de referencia, no una validación. Límites en el archivo de resultados.
   **Vida de los graduados (2026-10-21) — post hoc, sin pre-registro, solo descriptivo** (`scripts/madurez_vida.py` → `resultados/madurez_vida.txt`; velas horarias de GeckoTerminal de los 1 068 pools del 2026-08-20, 1 303 llamadas; saldos de vaults en cadena, ~230 CU):
   - Con volumen en las 24 h previas a cada edad: 97 % a 24 h, 49 % a 72 h, 34 % a 7 d, 29 % a 14 d y 10 % a 30 d. Con ≥ 5 000 USD/24 h: 485, 50, 28, 14 y 3 pools; con ≥ 25 000: 397, 33, 21, 10 y 2.
   - Liquidez solo la de hoy (~62 d, 2 × vault quote): mediana 211 USD; ≥ 1 000 en 135 pools, ≥ 5 000 en 17 y ≥ 25 000 en 4. La liquidez a 24 h / 72 h / 7 d no se puede reconstruir (k no se conserva en 6 de 8 pools comprobados; `reserve_in_usd` de GeckoTerminal es falso en pools muertos).
   - Universo orientativo: 7 d con ≥ 5 000 USD/24 h deja 28 pools/día (11 con liquidez actual ≥ 5 000). Cruzar con la liquidez de hoy usa información futura: no sirve como filtro en tiempo real.
   **Señales de "segunda vida" (2026-10-21) — post hoc, sin pre-registro** (`scripts/madurez_explora.py` → `resultados/madurez_explora.txt`; sin red, velas de `madurez_vida.py`). Una observación por token y día UTC con edad de 3–14 d. Evento: máximo cierre en 7 d ≥ 2× el cierre actual. Señales S1–S5 (RSI de Wilder y volumen de 6 h; definiciones en la cabecera del script). En S3 se añadió `vol6 > 0` (confirmado por Roi).
   - Universo con ≥ 5 000 USD en 24 h: 299 observaciones de 59 tokens, **tasa base 24.4 % (IC95 por token 17.6–31.0 %)**. Con ≥ 1 000 USD: 438 de 100 tokens, 19.6 % (14.3–24.9 %).
   - **Ninguna de las 10 combinaciones tiene un lift claramente distinto de 1**: todos los IC incluyen 1. S2 y S4 dan 1.13–1.14 [0.66–1.58]; S3 es exactamente 1.00 (se cumple en el 64–71 % de las observaciones y no discrimina). S5 queda sin potencia en los dos universos (2 y 3 eventos).
   - Un solo día, observaciones correlacionadas dentro de cada token y universo seleccionado tras el corte de las 72 h. El evento es un máximo a posteriori, sin comisiones ni salidas.
   **Cota superior de rentabilidad (2026-10-21) — post hoc, sin pre-registro, optimista a propósito** (`scripts/madurez_cota.py` → `resultados/madurez_cota.txt`; mismo universo y rejilla que `madurez_explora.py`). Reglas: entrada al cierre con 0 o 1 h de retraso, TP +50/+100 %, stop −20/−40 %/ninguno, 7 d, comisión 0.5/2.5 %; sin deslizamiento. 48 combinaciones en total.
   - **Las 48 medianas netas son negativas** (todas ≤ −17.1 %) y ningún IC de la mediana queda por encima de 0. La cota superior de la mediana es negativa. Tasa de acierto 20–45 %.
   - Medias netas de −7.6 % a +6.3 %. Solo 1 de 48 tiene el IC de la media por encima de 0 (≥ 5 000, 1 h, +50/−20 %, 0.5 %: +6.3 % [+0.7, +11.7]). Es compatible con comparaciones múltiples y depende de colas de pocos tokens. Sin liquidez histórica no se puede estimar el deslizamiento, así que no se da por válido.
