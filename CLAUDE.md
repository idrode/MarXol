# marXol

Proyecto hermano de [marXi](https://github.com/idrode/MarXi), mismo autor (Roi, GitHub `idrode`), misma filosofía, para **Solana** en vez de Robinhood Chain (EVM). Ruta local: `/home/ander/marXol`.

Este documento es la fuente de verdad del proyecto. Se actualiza en la misma sesión en que se confirma un hallazgo contra la chain real — nunca se deja un hallazgo verificado solo en una conversación de chat.

Desde el 2026-10-21, este archivo solo guarda lo necesario en toda sesión. El resto está en docs/ (índice abajo) y se lee bajo demanda. Cada hallazgo se registra, en la misma sesión, en el archivo de docs/ que corresponda (o aquí, si cambia el estado actual o una regla).

---

## Reglas duras

- **Solo lectura**: el rastreo es sobre direcciones de terceros. Sin wallet propia ni custodia de claves.
- **Sin trading**: marXol es un programa de búsqueda y análisis on-chain. El trading es un diseño dormido, no un objetivo.
- **Verificar on-chain antes de construir encima**: ningún program ID, umbral o parámetro se da por bueno por una doc o un blog. Los parámetros económicos se leen de `Global`, nunca se hardcodean.
- **Pre-registro**: hipótesis, umbrales y criterio de éxito se fijan y se anotan antes de mirar los datos. Cada evaluación congelada se ejecuta una sola vez. Un cambio posterior es una hipótesis nueva que exige tokens nuevos. Lo post hoc se etiqueta como tal y no decide nada.
- **Las definiciones congeladas no se modifican**: ver docs/validaciones_congeladas.md.
- **Registrar** cada hallazgo verificado en la misma sesión, con el mismo detalle si confirma o descarta. Correlación no es causalidad.

---

## 1. Propósito del proyecto

marXol es un **programa de búsqueda y análisis on-chain**, no un bot de trading. El trading queda como diseño dormido: no es un objetivo activo, aunque no se descarta como extensión futura si el análisis lo justifica.

El objetivo central es un módulo equivalente al `operator_tracker` de marXi: vigilar wallets/identidades concretas que despliegan tokens ("operadores" o "creadores"), analizar su historial de lanzamientos y financiación, y detectar patrones que distingan a los operadores que repiten éxito del ruido general del mercado.

### Por qué Solana y no seguir solo en Robinhood Chain

La actividad de lanzamientos en Robinhood Chain (el proyecto marXi original) se ha notado a la baja. Solana sigue siendo, con diferencia, la red con más volumen y velocidad de creación de memecoins — se han visto tokens completándose cada pocos segundos en plataformas como GMGN. El objetivo es aplicar la misma filosofía de análisis donde hay volumen real para medir patrones con solidez estadística.

### Interfaz: CLI directa desde el inicio

**Sin interfaz gráfica ni TUI desde el principio.** A diferencia de marXi (que usa ratatui), marXol empieza como CLI directa: comandos que consultan y analizan, nada de pantallas ni paneles interactivos. Esto es una decisión explícita, no una limitación temporal a resolver pronto — la prioridad es la lógica de análisis, no la interfaz.

### Self-custodial, sin custodia activa

Igual que marXi: no hay servidor de terceros custodiando claves, no hay custodia activa de clave privada. El módulo de rastreo de operadores es de solo lectura sobre direcciones de terceros — no necesita wallet propia para operar.

---

## 2. Filosofía heredada de marXi (se traslada tal cual)

Estos son principios de trabajo, no detalles técnicos — se aplican a cada decisión de este proyecto:

1. **Nunca confiar en una dirección/programa sin verificarlo on-chain antes de construir encima.** Ningún program ID, umbral de graduación, o parámetro económico se da por bueno solo porque lo diga una doc o un blog — se confirma contra la cuenta real en la chain (ver sección 7, "No verificado").
2. **Separar financiación de arranque de ingreso propio del creador con criterio estadístico explícito, no intuición.**
3. **Registrar cada hallazgo verificado en este `CLAUDE.md`, en la misma sesión en que se confirma** — nunca dejarlo solo en la conversación.
4. **Pre-registrar hipótesis y criterios de éxito antes de mirar los datos**, para evitar sesgo de confirmación (ver sección 8).
5. **Patrón de concurrencia**: una tarea de fondo por trabajo, comunicación solo por canal, un único punto que muta el estado.
6. **Distinguir siempre correlación de causalidad**; estar dispuesto a que una hipótesis se descarte con datos, y documentarlo igual de bien que un acierto.

---

## Estado actual

- **Datos** (`marxol.db`): piloto (40 tokens, calibración), V1 (337), V2 (400) y V3 (196), con ventanas tempranas y precios de 65 min completos.
- **Veredictos**: V1 confirmó H5, H6', H7' y H8 (H4' no, +19 pts). V2 (réplica) confirmó H4', H6' y H7'; H5, H8 y H10 no. V3: **D2 SEPARA GRUPOS** (+19.0 pts de retorno mediano a +30 min, p = 0.00016).
- **Lectura**: en todas, los tokens que pasan siguen con retorno mediano negativo. Las señales sirven para evitar, no para entrar. Solo valen para tres franjas de 8 minutos.
- **Herramienta**: `marxol check <mint>` aplica D2 a un token. Verificada sin red y en vivo, también con tokens recién creados (resultados/check_en_vivo_*.txt, check_temprano_2026-10-22.txt).
- **Pendiente**: benchmark de un día de WebSocket (tarifa por byte de Alchemy sin aclarar); `SetCreatorEvent` sin observar; H9–H13 sin pre-registrar.
- Detalle: docs/estado_y_pasos.md.

---

## Rumbo

REABIERTO (2026-10-22). Decisión del dueño. Nueva orientación: de "señal de entrada" a **veto/descarte y reputación de creador**, con datos validados.
- Punto de partida: entrar como comprador tiene mediana negativa (V3: −5.6 % los que pasan D2, −24.7 % los descartados, a +30 min). D2 es un filtro de descarte validado y `marxol check` lo implementa.
- Fases (cada una termina con resumen, tests y CU, y espera confirmación del dueño):
  1. Separar núcleo (librería) y CLI fina, sin cambiar comportamiento.
  2. Recolección continua 24/7 en base aparte (`marxol.db` queda congelada con V1–V3), con tope de CU diario y reanudación.
  3. Métricas: ingresos por comisión de creador (serie frente a puntuales), fábricas por fuente de fondos, quién captura el slot de creación.
  4. `marxol check` con razones medidas, su cifra histórica y su n (la línea de D2 y los códigos de salida no cambian).
  5. Validación fuera de muestra: definiciones congeladas antes de recoger los datos que las evalúan.
- Reglas: lenguaje neutral en la salida (hechos verificables, sin etiquetas); presupuesto Alchemy free tier (~1M CU/día), consumo registrado por comando; commits pequeños con tests.
- Se mantiene: revisar el IDL tras cada despliegue en ProgramData; check_en_vivo y check_temprano documentan los límites de uso de `check`.
- Estado previo (APARCADO, 2026-10-22) y su resumen: docs/estado_y_pasos.md.

---

## Líneas cerradas (resúmenes)

### H1c — velocidad de las wallets de 1 compra + 1 venta (pre-registrada 2026-09-28) — línea cerrada

**DESCARTADA en calibración** (2026-09-28, 40 tokens del piloto): no se cumplen ni (a) ni (b). Con `h1c`, H1 = true baja de 21 a 14 tokens, pero la proporción de pump-y-caída entre los positivos no baja (62 % → 64 %). `v1` sigue siendo el criterio.
**Pump-y-caída (1 h)**, definición fijada aquí y usada en todo lo posterior: pico ≥ 2× el precio inicial y caída > 30 % del pico al cierre.
Detalle literal (pre-registro, distribución de intervalos 1 + 1, resultado): docs/historico_lineas_cerradas.md

### Entrada a 1.5× (T_entry), H4–H7 y propuestas post hoc (2026-09-29) — línea cerrada

T_entry = primer trade de la ventana de 5 min con precio ≥ 1.5× el inicial. En calibración (40 tokens) cae en el slot de creación en 15 de 22 tokens, así que **no es alcanzable desde fuera**. H4, H6 y H7 degeneran, y H5 queda como candidata débil (+20 pts, Fisher p = 0.42; con el orden por firma salía p = 0.026, un artefacto anulado).
De aquí salieron T_entry2 y H8, fijadas en el congelado siguiente.
Detalle literal (definición, H4–H7, tablas y propuestas): docs/historico_lineas_cerradas.md

### Validación 1: consumo de RPC

**Consumo real de RPC (Alchemy)**: **906 320 CU** en 22 658 peticiones (57 % de la estimación de 1.6M), con 8 errores 429 que llevaron a añadir `rpc::with_retry`. Detalle literal: docs/historico_lineas_cerradas.md

### ROBUSTEZ POST HOC de la validación congelada (2026-09-30) — SIN VEREDICTO

Post hoc. H5, H6', H7' y H8 mantienen la asociación con un token por creator, sin fábricas y sin mayhem (IC bootstrap por clúster lejos de 0); H4' queda en +18.7 a +21.9 pts, sin cambiar su veredicto. Las señales se asocian a **más** supervivencia.
Ningún filtro A/B/C deja un resto con retorno mediano positivo (el mejor, A: −10.7 % → −6.9 % a +30 min). Definiciones en la sección 10; tablas literales: docs/historico_lineas_cerradas.md

### EXPLORACIÓN POST HOC (2026-09-30): curva de retorno desde T_entry2 y máxima subida — SIN VEREDICTO

Solo descripción (203 tokens sin mayhem). El retorno neto mediano es negativo en todos los horizontes y grupos (de −3.0 % a +5 s a −11.4 % a +60 min). La máxima subida posterior tiene una mediana de 1.001×, y la mitad de los máximos cae en el mismo segundo que T_entry2.
Detalle literal: docs/historico_lineas_cerradas.md

### Validación 2: ejecución

**Ejecución, fases 1 y 2 (2026-09-30)**: S2 = slot 451 810 013. **Tramo validación 2 = creaciones con slot en [451 810 013, 451 811 800]**: exactamente 400, de 01:00:00 a 01:07:58 UTC.
400 ventanas y 400 precios completos; total **1 354 600 CU**. `--check`: 283 tokens con T_entry2 (no aplica baja potencia) y ningún grupo < 3. Detalle literal: docs/historico_lineas_cerradas.md

### Cribado de descarte (2026-10-01, dos pasadas) — POST HOC, SIN PRE-REGISTRO, línea cerrada

En V2 primaria sin mayhem (n = 174), D2 (≥ 2 de H4', H6', H7') descarta el 43 % y captura 30/33 p&c. Sin H6' no se sostiene (18/33). En 27 de 33 p&c el primer 2x es anterior a T_entry2: lo que decide es el retorno desde T_entry2 (descartados −31 % frente a −5.9 % a +30 min).
Es el origen de D2, así que V1 y V2 quedan contaminadas para elegir filtros. Detalle literal: docs/historico_lineas_cerradas.md

### Validación 3: ejecución

S3 = slot 452 240 867. **Tramo validación 3 = 196 creaciones con timestamp en [1790845200, 1790845680), slots [452 240 867, 452 242 654]**; las 42 del margen quedan fuera, sin diferencias de borde. Este rango no se amplía ni se cambia.
196 ventanas y 196 precios completos; total **970 440 CU**. `--check`: población de 75 tokens (29 descartados, 46 pasan), 0 no evaluables y 0 sin datos. Detalle literal: docs/historico_lineas_cerradas.md

### Sección 10: piloto, salidas cortas y tokens maduros

**Piloto (2026-09-28/29), registros de ejecución**: 40 tokens de calibración. H1 = true en 21 de 40; H1b en 1 h sin potencia (1 graduado); la ronda H4–H7 degenera (sección 8).
Coste medido ≈ 117 `getTransaction` por token (≈ 4.8k CU en Alchemy). Detalle literal: docs/historico_lineas_cerradas.md

**Reglas de salida corta (2026-10-09) — post hoc, línea cerrada**: 1 920 combinaciones TP/SL/tiempo × latencia. Ninguna da mediana neta positiva en juntas sin mayhem: la comisión (≈ 2.7 pts por ida y vuelta) domina y el token mediano no opera. Detalle literal: docs/historico_lineas_cerradas.md

**Tokens maduros ("segunda vida", 2026-10-09 a 2026-10-21) — post hoc, línea cerrada**. Detalle literal: docs/historico_lineas_cerradas.md
- Fase 0: GeckoTerminal no permite listar el universo de un periodo pasado (~10 llamadas/min, velas solo desde ~2026-03-14).
- Fase 0b: 1 068 migraciones el 2026-08-20; en la muestra de 40, muertos 82 % y segunda vida 1/40.
- Vida: con volumen, 34 % a 7 d y 10 % a 30 d.
- Señales S1–S5: ningún lift distinto de 1 (tasa base 24.4 %).
- Cota: las 48 medianas netas son negativas.

---

## Índice de docs/

Las referencias a "sección N" dentro de docs/ usan la numeración antigua de este archivo: 3–7 y 9 → referencia_tecnica; 8 → hipotesis_y_resultados, validaciones_congeladas y exploracion_operadores; 10 → estado_y_pasos; 11 → aquí.

- docs/referencia_tecnica.md — pump.fun (program ID, PDAs, `Global`, graduación), identidad del creator, eventos del IDL, proveedores RPC y costes, límites de APIs, puntos no verificados, hallazgos operativos de mainnet y fuentes.
- docs/validaciones_congeladas.md — definiciones congeladas de V1, V2 y V3 (con adenda y tramos) y definiciones de la robustez post hoc que cita V2. No se modifican.
- docs/hipotesis_y_resultados.md — H1, H1b, H2 y H3, y resultados completos de V1, V2 y V3.
- docs/exploracion_operadores.md — exploración de operadores (2026-09-29) e hipótesis H11–H13 sin pre-registrar.
- docs/estado_y_pasos.md — siguientes pasos y estado detallado, Etapa 0 (margen del aviso) y `marxol check`.
- docs/historico_lineas_cerradas.md — detalle literal de las líneas cerradas y de los registros de ejecución.
- docs/diseno_record.md — diseño de `marxol record` (Etapa 2, sin código).

---

## 11. Estado de implementación y hallazgos operativos

CLI Rust (`cargo build`; binario `marxol`). RPC por `--rpc` / `MARXOL_RPC_URL` (por defecto el RPC público de mainnet); base local por `--db` / `MARXOL_DB` (por defecto `marxol.db`).

| Comando | Qué hace |
|---|---|
| `marxol verify` | Comprueba program ID ejecutable, IDL embebido, PDA `Global`, decodificación exacta de `Global` y umbral de graduación |
| `marxol global` | Parámetros económicos en vivo + graduación derivada |
| `marxol curve <mint>` | Deriva la PDA `["bonding-curve", mint]` y decodifica `BondingCurve` |
| `marxol tx <firma>` | Decodifica todos los eventos pump.fun de una transacción |
| `marxol scan [--address X] [--limit N] [--events A,B] [--all]` | Muestra eventos de las firmas recientes de una dirección (sin guardar) |
| `marxol index [--address X] [--limit N]` | Indexa eventos de identidad y ciclo de vida en SQLite, paginando hacia atrás y saltando firmas ya vistas |
| `marxol windows [--limit N] [--max-pages P] [--provider auto\|public\|alchemy\|helius] [--max-rps R] [--max-requests K]` | Para tokens indexados con la ventana temprana ya cerrada, recorre las firmas de su bonding curve hasta la creación y guarda los `TradeEvent` de la ventana; registra en `trade_windows` si la descarga fue completa |
| `marxol h1 [--variant v1\|h1c]` | Calcula la señal H1 (sección 8) sobre los tokens con ventana completa, la guarda en `h1_results` por variante e imprime el resumen y la distribución de intervalos 1 + 1 |
| `marxol prices [--limit N] [mismas opciones de ritmo]` | Para tokens con ventana temprana completa y la ventana de precio ya cumplida, guarda los puntos de precio de la bonding curve en `[t0, t0+65 min)` (1 h de H1b + 5 min para el retorno a +60 min desde T_entry2) (reutiliza los trades de la ventana temprana; pide el resto y la tx de creación) |
| `marxol h1b [--variant v1\|h1c]` | Calcula H1b en 1 h (graduación, pump sostenido) y la cruza con la variante de H1: tablas 2×2, test exacto de Fisher, odds ratio con IC 95 %, pump-y-caída por grupo y criterio (a) de H1c |
| `marxol entry` | T_entry (primer trade con precio ≥ 1.5× el inicial en 5 min, en orden de ejecución) e indicadores previos H4–H7, cruzados con pump-y-caída en 1 h: tablas 2×2 con y sin `CRHnzej9…`, veredicto del criterio pre-registrado y distribuciones. Sin RPC |
| `marxol entry2 [--check] [--tramo validacion\|validacion2\|piloto] [--replica]` | Validación congelada (sección 8): T_entry2, H4', H5, H6', H7', H8 contra pump-y-caída en 1 h (Fisher, criterio congelado, sin el token de mayor peso) y retorno neto desde T_entry2 a +10/+30/+60 min (Mann-Whitney). `--check` solo cuenta grupos, sin resultado. Sin `--check`, se niega a usar el piloto. Con `--tramo validacion2` (o `--replica` sobre `validacion`, solo como comprobación), aplica el criterio de réplica de la validación 2: tres poblaciones, H4'–H8 y H10, retornos (a)/(b) y filtros A/B/C con bootstrap. Sin RPC |
| `marxol check <mint\|URL> [--offline] [--provider P] [--max-rps R] [--max-requests K ≤ 400]` | Comprobación previa con D2 (validación 3). Lista la bonding curve hasta la creación, pide las tx de la ventana temprana y las ingiere en una base en memoria con `Store::ingest`, como `windows`. Calcula T_entry2, H4', H6' y H7' con `entry2::compute` y D2 con `v3::d2`, sin tocar las definiciones. Veredicto `EVITAR` / `SIN BANDERAS DE D2` / `NO EVALUABLE`, con aviso de mayhem y avisos fijos. Tope de 400 peticiones (16 000 CU), registradas en `rpc_usage`. Sale con 0 si hay veredicto, 3 si no es evaluable y 1 si falla la lectura. `--offline` evalúa con `--db` en solo lectura (`src/check.rs`) |
| `marxol operator <creator>` | Informe de un operador sobre lo indexado |
| `marxol stats` | Recuento de filas |

`--json` sirve para todos los comandos que imprimen eventos o cuentas.

**Módulos** (desde 2026-10-22, librería `marxol` en `src/lib.rs` y CLI fina en `src/main.rs`: clap, despacho y `check`):
- `src/ingest.rs`: descargas a la base local (`index`, `index-tramo`, `windows`, `prices`), con presupuesto y registro en `rpc_usage`.
- `src/report.rs`: informes de los comandos de análisis (`h1`, `h1b`, `entry`, `entry2`, `operator`) y salida de cuentas y tx decodificadas.
- `src/verify.rs`: `marxol verify` y lectura de cuentas del programa.
- `src/idl.rs`: decodificador Borsh genérico dirigido por `idl/pump.json` (embebido en el binario). No hay structs escritos a mano por evento. Si los bytes son más cortos que el IDL actual (eventos o cuentas históricos, de antes de que se añadieran campos al final), los campos que faltan se reportan en `missing`, nunca se rellenan con valores por defecto.
- `src/tx.rs`: `getTransaction` → inner instructions al programa pump cuyo primer account es el `event_authority` (PDA `["__event_authority"]`) → prefijo `e445a52e51cb9a1d` (sha256("anchor:event")[..8]) + discriminador de evento → Borsh. Incluye las cuentas cargadas desde lookup tables.
- `src/store.rs`: tablas `creations` (creator original, inmutable), `creator_changes` (una fila por evento, `kind ∈ {set_creator, migrate_to_sharing}`, nunca sobreescribe), `completions`, `migrations`, `seen_signatures`; para H1: `early_trades` (los `TradeEvent` de la ventana temprana de mints con creación conocida, con el `creator` embebido del trade = creator vigente en ese momento), `trade_windows` (cobertura por mint: solo con `complete = 1` H1 es evaluable) y `h1_results` (resultado derivado por `(mint, variant)` con los parámetros usados; recalculable, se reemplaza; sustituye a la antigua `h1_signals`, que se borra al abrir la base).
- `src/h1.rs`: cálculo puro de H1 (sin RPC), con los umbrales [P] en `PARAMS_V1` (criterio vigente) y `PARAMS_H1C` (variante H1c, descartada en calibración).
- `src/h1b.rs`: cálculo puro de H1b (sin RPC), con los umbrales [P] en `PARAMS_1H`, y el test exacto de Fisher bilateral. Tablas de soporte en `store.rs`: `price_points` (`kind ∈ {create, trade}`) y `price_windows`.
- `src/entry.rs`: cálculo puro de T_entry y H4–H7 (sin RPC), con los umbrales [P] en `PARAMS`, y `execution_order`, que reconstruye el orden de ejecución de los trades encadenando reservas.
- `src/entry2.rs`: cálculo puro de T_entry2, los indicadores congelados (`PARAMS`, `PILOT_LAST_SLOT`, `VALIDATION1_LAST_SLOT`), el retorno neto y el test de Mann-Whitney.
  - **Aclaración de implementación (2026-09-30), no es un cambio de criterio**: el tramo de la validación 1 estaba en el código como "slot > 451 340 397", sin límite superior, así que las creaciones posteriores habrían entrado en él. Ahora está acotado a slot ≤ **451 342 089** (`VALIDATION1_LAST_SLOT`, el slot máximo de sus 337 creaciones). El conjunto sigue siendo exactamente los mismos 337 mints.
  - El script de robustez cuenta ahora las fábricas solo sobre las 377 creaciones del piloto y la validación 1.
  - Se verificó antes y después de ingerir la validación 2 que la salida es **idéntica byte a byte**: `entry2 --json` (misma lista de 251 mints y mismos números), `entry2`, `entry2 --check`, `scripts/robustez_posthoc.py` y `scripts/curva_retorno_posthoc.py`.
  - `h1`, `h1b` y `entry` no seleccionan por tramo: trabajan sobre todos los tokens con ventana completa. Cuando se descarguen las ventanas de la validación 2 los incluirán si se vuelven a ejecutar. No se han tocado.
- `marxol index-tramo --from <unix> --count N`: indexa un tramo por tiempo recorriendo solo las listas de firmas de `mint-authority` y pidiendo `getTransaction` solo de las firmas exitosas necesarias, por slots enteros. Se puede reanudar.
- `src/rpc.rs`: presupuesto de RPC por proveedor (`Budget`): ritmo máximo, tope de peticiones por ejecución y contador de peticiones y unidades reales (40 CU por método histórico en Alchemy, 10 créditos en Helius, 0 en el RPC público).
- **Descargas reanudables** (`windows`, `prices`):
  - Saltan tokens ya completos y, dentro de un token, las firmas ya ingeridas (tabla `fetched_sigs`, por propósito `window`/`prices`). No se usa `seen_signatures` porque una firma vista por `index` antes de conocer la creación no tiene sus trades guardados.
  - Al llegar a `--max-requests` paran limpiamente, y el token a medias sigue en la próxima pasada.
  - Un 429 del proveedor se reintenta hasta 4 veces con espera de 1, 2, 4 y 8 s (cada intento cuenta en el presupuesto). Un error al listar las firmas de un token lo deja pendiente sin cortar la ejecución.
  - `rpc_usage.tokens` cuenta los tokens procesados aunque la ejecución termine en error.
  - Cada ejecución deja su consumo en `rpc_usage` y lo imprime junto con el acumulado.
  - `early_trades` guarda ahora `fee_basis_points` y `creator_fee_basis_points` (se añaden a bases antiguas al abrirlas).
  - `windows` guarda también el precio inicial del `CreateEvent`.
- `src/pump.rs`: constantes, PDAs y matemática de graduación.
- `src/replica.rs`: cálculo puro del criterio de réplica (tablas, veredicto, filtros) y el bootstrap. Usa un Mersenne Twister sembrado como `random.Random` de Python, para que los intervalos coincidan exactamente con los de `scripts/robustez_posthoc.py` con la misma semilla.
- `scripts/` (Python, solo biblioteca estándar; análisis **post hoc**, sin veredicto):
  - `robustez_posthoc.py`: robustez de la validación congelada.
  - `curva_retorno_posthoc.py`: curva de retorno y máxima subida desde T_entry2.
  - `verificar_replica.py`: comprueba que `entry2 --tramo validacion --replica` reproduce exactamente la robustez post hoc (primaria n = 210). Es un test, no un análisis.
  - Los dos leen `target/release/marxol entry2 --json` y `marxol.db`. Se ejecutan desde la raíz del repo con `python3 -B`.
  - `cribado_posthoc.py`: cribado de descarte D, D2 y repetición de creadores en V1 y V2 (→ `resultados/cribado_posthoc.txt`). Su carga de datos la reutilizan los scripts siguientes.
  - `cribado_posthoc2.py`: segunda pasada del cribado (variantes de D2 sin H6' y momento del primer 2x frente a T_entry2) (→ `resultados/cribado_posthoc2.txt`).
  - `verificar_d2_v2.py`: comprueba que `entry2 --tramo validacion2 --d2-prueba` da la misma población, los mismos descartados y la misma diferencia de medianas que `cribado_posthoc.py`. Es un test, no un análisis.
  - `etapa0_latencia.py`: Etapa 0, retraso de T_entry2 respecto a la creación y D2 restringido a T_entry2 ≤ X (→ `resultados/etapa0_latencia.txt`).
  - `salidas_posthoc.py`: reglas de salida corta TP/SL/tiempo con latencias 0–10 s (→ `resultados/salidas_posthoc.txt`).
  - `bench_stream.py`: benchmark de `logsSubscribe` en Alchemy (Etapa 1). No es un análisis post hoc: **usa la red** y la clave de `.env`, y escribe en `marxol.bench.db` (→ `resultados/benchmark_streaming.txt`).
  - `madurez_fase0b.py`: fase 0b de tokens maduros. `--dry-run` (sin red), `--check` (etapa A, tope 2 000 CU), `--run` (lista un día por blockTime sobre `withdraw_authority`, decodifica las migraciones y descarga de GeckoTerminal una muestra; reanudable; topes de CU y de llamadas; escribe en `madurez.db` y en `rpc_usage`) y `--report` (solo lee).
  - `madurez_vida.py`: descriptores de vida de los graduados de un día. `--download` (velas horarias USD de GeckoTerminal, reanudable, ≤ 10/min, `--max-gt`), `--chain` (saldos de los vaults con `getMultipleAccounts` y precio del quote) y `--report` (solo lee; → `resultados/madurez_vida.txt`). Escribe en `madurez.db` y `rpc_usage`.
  - `madurez_explora.py`: exploración post hoc de las señales S1–S5 de "segunda vida" sobre las velas de `madurez_vida.py`. Lift con bootstrap por token. Sin red; solo lee `madurez.db` (→ `resultados/madurez_explora.txt`).
  - `madurez_cota.py`: cota superior de rentabilidad (TP/stop/7 d con comisión y sin deslizamiento) sobre el universo de `madurez_explora.py`. Sin red; solo lee `madurez.db` (→ `resultados/madurez_cota.txt`).
- `src/v3.rs`: cálculo puro de la validación 3 (filtro D2, población de un token por creator, criterios 1–5, veredicto y adendas), sin RPC ni base.

Hallazgos operativos de mainnet: docs/referencia_tecnica.md.
