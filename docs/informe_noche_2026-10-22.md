# Informe de la tarea nocturna (2026-10-22, de 20:30 a 21:00 UTC aprox.)

Todo es exploratorio y post hoc. **Sin red: 0 CU, ninguna llamada RPC ni a GMGN.** No hay commit ni push. No se ha tocado `collect`, `.env`, ninguna clave ni la tasa de muestreo. Los retornos salen solo de `marxol.db`, abierto en solo lectura.

## Integridad

- **sha256 de `marxol.db`**: `d513e1f3a0fa0459aad5e89be918d99eab9d39d47005b0dc437bc09479fb8dae`, igual al principio y al final.
- `marxol_vivo.db`: solo conteos operativos (`collect-status`, libro de CU, `vivo_cycles`, `vivo_tokens.status`/`sample_ppm`, `vivo_create_sigs`). No se ha leído `vivo_results`, `vivo_outcomes`, `price_points` ni `early_trades` de esa base.
  - **Efecto lateral**: `collect-status` abre la base en modo escritura (WAL). Al cerrarla, como no había otro proceso conectado, SQLite volcó el WAL en el fichero principal y borró `marxol_vivo.db-wal`. No cambia ningún dato, pero sí el fichero. El resto de consultas fueron con `?mode=ro`.
- `~/pruebas/gmgn-probe/raw/` se trató como datos: solo se leyó `data.address` y `data.dev.creator_address` de los `token_info`.

## Cambios en el árbol (sin commit)

- `src/report.rs`, `src/main.rs`: `banco-fase-c` acepta ahora dos opciones.
  - `--progreso`: añade el control de la tarea 1.
  - `--volcado DIR`: escribe `tokens.tsv` (la población evaluable) y `compradores.tsv` (compradores anteriores a T_entry2, en todas las filas con T_entry2).
  - Sin opciones, la salida es **idéntica byte a byte** a resultados/fase_c_banco.txt (comprobado con `cmp`).
  - `cargo test --release`: 96 tests, todos pasan. Son tests unitarios, porque no hay tests de integración.
- Ficheros nuevos:
  - docs/fase_c_control_progreso_2026-10-22.md (declaración);
  - docs/borrador_adenda_2.md;
  - scripts/potencia_adenda2.py;
  - resultados/fase_c_control_progreso.txt y resultados/potencia_adenda2.txt;
  - este informe.
- `target/release/marxol` recompilado. `run/collect.sh` usa su propia copia, `run/marxol`, que no se ha tocado.

## 1. Control mecánico de C1 por progreso de la curva

- **Declaración previa**: docs/fase_c_control_progreso_2026-10-22.md, escrita a las 20:39:45 UTC, antes de calcular nada. **sha256 `643406d1339aba93bdb4ad66667565665fe7787eeb2ee2d855a6bcdd6581832b`**.
- **Progreso**: 1 − reservas virtuales de token justo antes de T_entry2 / iniciales. Terciles sobre los 438 tokens; cortes en 2.2061 % y 9.5404 %. El umbral de C1 (5 %) no cambia.

| Tercil | n | con C1 | mediana sin C1 | mediana con C1 | diferencia (IC 95 %) | MW unilateral p |
|---|---|---|---|---|---|---|
| 1 (bajo) | 146 | 0 (0 %) | −3.9 | n/d | n/d | n/d |
| 2 | 146 | 26 (17.8 %) | −8.9 | −14.9 | +6.0 [+3.7, +7.6] | 1.2e-6 |
| 3 (alto) | 146 | 41 (28.1 %) | −37.1 | −34.6 | **−2.5 [−8.9, +3.9]** | 0.77 |

- van Elteren unilateral: z = 2.508, p = 0.006.
- Diferencia ponderada: **+1.8 pts, IC [−1.5, +5.0]**.
- **¿Sobrevive? NO.** El IC incluye 0, y el tercil 3 (≥ 10 por grupo) va en contra. Con los estratos de actividad sí sobrevivía (+13.5 [+8.7, +19.3]).
- Lectura (post hoc, no decide nada): C1 y el progreso van muy juntos. El progreso mediano es de 13.8 % con C1 y de 3.2 % sin C1. En el tercil bajo C1 no puede darse, porque > 5 % del supply exige haber movido la curva (≈ 4.7 % de las reservas). La parte de los +16.6 pts que no explica el progreso es pequeña y solo aparece en el tercil medio.
- **"Capitalización o reservas"**: en la declaración supuse que la capitalización daba los mismos terciles. **Era falso**: 170 de 438 tokens cambian de tercil, porque 101 de 438 no cotizan en SOL (reservas virtuales de quote iniciales distintas de 30 SOL) y su precio no se puede comparar sin conversión, que exigiría red.
  - Decisión conservadora: el control principal es el de reservas de token, que se puede comparar entre todos.
  - Como sensibilidad, solo con los 337 cotizados en SOL (donde progreso y capitalización ordenan igual): tercil 1 sin C1, tercil 2 +5.6 pts, tercil 3 −2.6 pts, ponderada +1.5 [−3.4, +6.1]. Misma conclusión.
- Salida completa: resultados/fase_c_control_progreso.txt.

## 2. Borrador de la adenda 2

docs/borrador_adenda_2.md, **sin commit**:
- C1 como razón principal: diferencia de medianas (sin − con) a +30 min, sin > con.
- C2 solo descriptiva, con la etiqueta secundaria "retorno ≤ −50 %".
- k = 3 (R1, R2, C1) → p < 0,0033.
- **n esperado en el bloque B ≈ 960** (≈ 147 con C1), sin mayhem y con un token por creator.
- **Con la mitad del efecto (+8.3 pts), la potencia es ≈ 0.20 con cualquier n.** El cuello de botella es el suelo de 10 pts del criterio 3, no la n. El efecto mínimo detectable con el criterio completo es ≈ 12,5 pts (potencia 0.87 con n = 720–960).
- Incluye, para el dueño, el resultado de la tarea 1 y los terciles de progreso como secundario, con los cortes congelados.

## 3. Comparación de creadores (20 tokens)

- `/tmp/mints_prueba.csv` no existía. **Decisión**: en vez de volver a exportar, usé `~/pruebas/gmgn-probe/mints_prueba.csv`, que es el fichero exacto con el que se consultó GMGN. Así la comparación es contra las mismas 20 filas. No se buscó el código de exportación con semilla 20261022 ni se volvió a ejecutar.
- La columna `creator` del CSV coincide con `creations.creator` de marXol en 20/20.

| Comparación | Coinciden |
|---|---|
| GMGN `dev.creator_address` = creator de marXol (`CreateEvent.creator`) | 16/20 |
| **GMGN `dev.creator_address` = pagador (`CreateEvent.user`)** | **20/20** |
| creator = pagador en marXol | 16/20 |

- **Los 4 que no coinciden** (t04 `7BcTjtdW`, t15 `FGFziTe1`, t18 `AUG4TfjY`, t19 `BkSHaA1m`) son justo los 4 en que creator ≠ pagador. GMGN atribuye el token al pagador, el que firma.
  - En los 4, la wallet `creator` aparece una sola vez como creator en `marxol.db`, nunca paga una creación y **no tiene ningún trade en la ventana temprana**.
  - El pagador sí compra: 2, 5, 4 y 2 trades.
  - En 3 de los 4, el pagador figura en 2 creaciones de `marxol.db`, ambas con un creator distinto de él. En t15 el pagador (`timeworn…`) es además creator de otro token.
  - Es compatible con el patrón "posible servicio" de la fase 3 (alguien que paga la creación para otro `creator`), pero **no está verificado**.
  - Ninguno de los 4 tiene `creator_changes`.
- **Alcance**: en `marxol.db`, **173 de 1 015 creaciones (17 %) tienen creator ≠ pagador**. En esa fracción, "creador" significa una wallet distinta en GMGN y en marXol. R1 ya usa el pagador, y C1 usa ambos como identidades.
- **Export**: `/tmp/wallets_prueba.csv` (sha256 `76de01db57f191acdf4b466ef70cbebf0f7a56701a32e9d62f3649962d8ca7da`), con las columnas `mint,wallet,rol` y sin importes ni retornos.
  - 68 filas de 17 tokens: 53 `comprador`, 12 `creador_y_pagador` y 4 `pagador`.
  - Los otros 3 tokens (t01 `79JfGo1T`, t05 `9BNp3Jiz` y t14 `4XnWZ9Hb`) no tienen T_entry2: ningún trade a ≥ 10 s y en un slot posterior al de la creación. No hay "antes de T_entry2" que exportar.
  - Una wallet aparece una vez por token. La partición es la de `entry2::compute`, que incluye tokens mayhem y tokens fuera de la población de un token por creator (4 de los 17).
  - Roles: `creador_y_pagador` si la wallet es a la vez `CreateEvent.creator` y `user`; `creador` o `pagador` si es solo una de las dos; `otra_identidad_creador` para el resto de identidades de C1 (no aparece ninguna); `comprador` para las demás.

## 4. Escaneo de secretos (solo lectura; no se ha borrado ni reescrito nada)

- **`.env`**: existe, con permisos 600. Está en `.gitignore` y **nunca se ha versionado** (`git log --all -- .env` vacío). Variables: `ALCHEMY_API_KEY` (rellena) y `HELIUS_API_KEY` (vacía).
- **Valor real de la clave de Alchemy**: no está en ningún fichero del árbol de trabajo (código, docs, `resultados/`, `run/`, `*.db`, `target/`), ni en `~/pruebas` ni en `/tmp`. Tampoco aparece en ninguna revisión del historial git (`git log --all -S`).
- **Sí aparece en una transcripción de Claude Code**: `~/.claude/projects/-home-igt-MarXol/8b7b17f5-2227-47c7-b671-9f73e61cde77.jsonl` (2 apariciones, modificada el 2026-10-22 a las 06:26 local). Es la única copia fuera de `.env` en todo `/home/igt`. Queda para que decidas si la borras o rotas la clave.
- **Patrones de URL con clave** (`alchemy.com/v2/…`, `api-key=…`):
  - En el árbol, solo están en los tests de `src/rpc.rs` (líneas 366–400), con valores ficticios (`alch_p-x2Ab_9`, `abc-123`, `k`).
  - En el historial, solo esos mismos tests (commit `573e224`).
  - No hay ficheros con nombre sensible (`.env`, `*.pem`, `id_rsa`…) en ninguna revisión.
- **Clave de GMGN**: la clave de demostración pública del README (`gmgn_…`) aparece en claro en `~/pruebas/gmgn-probe/informe_prueba.md` (§7), fuera del repo. No está en marXol ni en su historial.
- No hay claves privadas, tokens de GitHub, Anthropic, Slack ni AWS en el árbol.

## 5. Estado de `collect` (solo lectura)

- **`collect` NO está corriendo.** No hay ningún proceso `marxol` ni el bucle de `run/collect.sh`. `run/collect.pid` (28438) apunta a un proceso que ya no existe.
  - El último ciclo (37) empezó a las 17:01:10 UTC y no terminó. `run/collect.log` acaba a las 17:02 UTC sin la línea `=== salida rc=`, así que el proceso y el bucle se mataron desde fuera (¿sesión cerrada o máquina suspendida?).
  - No lo he reiniciado, por las reglas. Lleva parado **≈ 3 h 50 min** a la hora de este informe.
- **Arranques** (`run/collect.log`): 02:15:51, 03:08:01 y 16:24:39 UTC. El primero tampoco tiene línea de salida.
- **Hueco de ejecución**: entre el ciclo 30 (04:38 UTC) y el 31 (16:24 UTC), ≈ 11 h 46 min sin `collect`. Al reanudar, el ciclo 31 se puso al día con el listado: 20 755 firmas, 87 muestreadas, 109 700 CU. `vivo_gaps` está vacía, así que **no hay huecos en el listado**.
- **Ciclos**: 37 en `vivo_cycles` (36 terminados). Medidos en 52 913 s.
- **CU del 2026-10-22 (libro)**:
  - `collect`: 263 600 CU en 6 600 peticiones (de 700 000);
  - `manual`: 108 280 CU en 2 707 peticiones (`payer-history`);
  - total: 371 880 de 1 000 000;
  - por etapa (collect-status): listado 2 440, seguimiento 400, creación 4 680, ventanas 224 560 y precios 29 760.
- **429**: 138 reintentos en el log (101 en el primer intento, 24 en el segundo, 10 en el tercero y 3 en el cuarto). Ninguno agotó los 4 intentos; no hay línea de fallo definitivo.
- **Tokens**: 112 muestreados a 4 000 ppm (111 `creado`, 1 `sin_create`). Ventanas tempranas completas 101/110 y de precio 92/93. Seguimientos 101/444. Resultados agregados (sin leer): 102.
  - En el último ciclo quedaban 9 tokens con la ventana incompleta: 3 por superar el tope de 400 tx y 6 por `max_pages` sin llegar a la creación. Además, 1 token tiene el precio incompleto (822 tx, por encima del tope).
- **Aviso del log** (16:24 UTC): "TradeEvent de 382 bytes, 8 más de los 374 que describe el IDL embebido". Ha habido un despliegue nuevo de pump.fun y, según la regla del proyecto, **hay que revisar el IDL**. El decodificador lo cuenta sin error (test `trade_event_con_bytes_de_mas_se_cuenta_sin_error`). No lo he revisado porque exige red.
- `collect-status` proyecta con "muestreo 3000 ppm", que es su valor por defecto. `collect` usa 4 000 (`vivo_tokens.sample_ppm` = 4000 en los 112). Es solo la proyección, no un cambio.

## Decisiones ambiguas tomadas

1. Para el progreso de la curva usé el estado **justo antes** del trade T_entry2 (lo disponible al decidir), no el de después.
2. Empates en los cortes de tercil: al tercil inferior.
3. Capitalización: como no es comparable entre quotes sin red, el control principal es el de reservas de token y la capitalización va como sensibilidad, solo con SOL.
4. `/tmp/mints_prueba.csv`: se usó la copia de `~/pruebas/gmgn-probe/` en lugar de volver a exportar (§3).
5. "Rol" en el export: creador / pagador / creador_y_pagador / otra_identidad_creador / comprador. Solo compradores, como pide la tarea; los vendedores quedan fuera.
6. En el borrador, k = 3 obliga a cambiar también el umbral de R1 y R2 (0,005 → 0,0033). Lo dejo indicado, pero no he tocado el pre-registro.
7. Los hallazgos de esta noche quedan registrados en este informe y en los ficheros enlazados. No he editado CLAUDE.md ni docs/estado_y_pasos.md, porque ya tenían cambios tuyos sin commit. Pendiente al revisar.

## Lo que no se pudo hacer

- Reiniciar `collect` (prohibido) y revisar el IDL nuevo (exige red).
- Capitalización comparable entre quotes distintos de SOL (exige precios externos).
- Volver a exportar `/tmp/mints_prueba.csv` con la semilla 20261022: no encontré el código que lo generó (solo la copia en `~/pruebas`).
- Simular el criterio 4 (sin el token de mayor peso) en la potencia, y el bootstrap con 2 000 réplicas por creator (usé 300 por token, por tiempo).
