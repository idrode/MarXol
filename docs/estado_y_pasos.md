# Estado detallado y siguientes pasos

Movido **literalmente** desde `CLAUDE.md` (antigua sección 10) el 2026-10-21. Las definiciones de la robustez post hoc están en docs/validaciones_congeladas.md; los resúmenes de líneas cerradas, en `CLAUDE.md`.

Las referencias a "sección N" usan la numeración de `CLAUDE.md` anterior al 2026-10-21: secciones 3–7 y 9 → docs/referencia_tecnica.md; 8 → docs/hipotesis_y_resultados.md, docs/validaciones_congeladas.md y docs/exploracion_operadores.md; 10 → docs/estado_y_pasos.md; 11 → `CLAUDE.md`. El detalle de líneas cerradas está en docs/historico_lineas_cerradas.md.

---

## 10. Siguientes pasos

1. ~~Verificar contra RPC real los puntos de la sección 9~~ — hecho 2026-09-28 salvo Yellowstone.
2. ~~Prototipo de decodificación de `create_v2` reales~~ — hecho (sección 11).
3. ~~Esquema de datos para las tres señales de identidad~~ — hecho en `src/store.rs` (sección 11). Pendiente: observar casos reales de `SetCreatorEvent`/`MigrateBondingCurveCreatorEvent`.
4. Benchmark real de consumo (CU/créditos) de un día de captura vía WebSocket antes de comprometerse a un proveedor único.
5. ~~Definir explícitamente el criterio de "éxito" de H1 antes de tocar datos~~ — hecho 2026-09-28 (sección 8, con valores provisionales).
   H1 implementada (2026-09-28): `marxol windows` + `marxol h1`. Pendiente: H1b, que necesita la trayectoria de precio de 1 h y 24 h (bonding curve y, si gradúa, PumpSwap), y ninguna de las dos está descargada.
   **Piloto (2026-09-28/29), registros de ejecución**: 40 tokens de calibración. H1 = true en 21 de 40; H1b en 1 h sin potencia (1 graduado); la ronda H4–H7 degenera (sección 8).
   Coste medido ≈ 117 `getTransaction` por token (≈ 4.8k CU en Alchemy). Detalle literal: docs/historico_lineas_cerradas.md
   **Validación congelada (2026-09-29 15:24 UTC)**: ejecutada el mismo día con Alchemy (`windows` → `prices` → `entry2 --check` → `entry2` una vez). Resultado y consumo (906 320 CU) en la sección 8: H5, H6', H7' y H8 confirmadas; H4' no confirmada (+19 pts). Todas marcan tokens **peores** para entrar (retorno neto mediano negativo en todos los grupos). Solo vale para la ráfaga de 8 min del 2026-09-28.
   **Validación 2 (réplica), ejecutada 2026-10-01**: H4', H6' y H7' se confirman en réplica; H5, H8 y H10 no (sección 8). Sigue siendo una sola franja de 8 minutos de otro día.
   **Validación 3 (filtro D2), ejecutada 2026-10-08**: **D2 SEPARA GRUPOS** (+19.0 pts de retorno mediano a +30 min, p = 0.00016, IC [+11.2, +27.2]). Los tokens que pasan siguen con retorno mediano negativo (sección 8).
   **Siguiente**: repetir la validación en más franjas y días antes de dar ningún indicador por general, y pre-registrar H9/H10 y el resultado medido desde el precio de entrada (sección 8). Pre-registrar también H11 (lanzamiento previo del creator, con N horas y estratificado por mayhem y mismo símbolo), H12 (persistencia de dev-sell entre tokens del mismo creator) y H13 (repetir H5/H6'/H7'/H8 con un token por creator como control de seudorreplicación), surgidas de la exploración de operadores (sección 8).
   **Margen real del aviso (Etapa 0, 2026-10-09) — POST HOC, sin p-valores** (`scripts/etapa0_latencia.py` → `resultados/etapa0_latencia.txt`, sin red):
   - T_entry2 − creación, sin mayhem (1 por creator): mediana **13 s** en V1 (n 193), **15.5 s** en V2 (174) y **16 s** en V3 (75). El p90 está en 67–83 s, y ≤ 30 s lo cumple el 64–78 %. En mayhem la mediana es 10 s.
   - Si solo se atienden los tokens con T_entry2 ≤ 30 s, D2 sigue capturando casi todos los p&c: 38/53 en V1, 30/33 en V2 y 13/14 en V3. Ningún p&c de V2 ni de V3 tiene T_entry2 > 60 s.
   - Las medianas a +30 min se mantienen: descartados −24.7 % a −32.4 % frente a −6.3 % a −8.6 % de los que pasan.
   - **Para el grabador**: unos 60 s de trades por token cubren el 79–88 % de los tokens. A la latencia del stream (Etapa 1) hay que sumarle 10–16 s de margen intrínseco.
   **`marxol check <mint>` (2026-10-21) — herramienta, no análisis; sin pre-registro nuevo**. Aplica D2 a un token: T_entry2, H4', H6' y H7' con las funciones congeladas (`entry2::compute`, `v3::d2`), sin cambiarlas.
   - **Verificado sin red**:
     - Sobre los 933 tokens de V1, V2 y V3, el veredicto coincide con el D2 calculado como en `entry2`: 213 EVITAR, 479 SIN BANDERAS y 241 NO EVALUABLE (todos por no tener T_entry2), con mayhem incluidos.
     - Reconstruyendo sus transacciones como fixtures y pasándolas por el mismo camino que la descarga, el resultado coincide campo a campo.
     - Contra `entry2 --tramo validacion3 --check --json`: 158/158 iguales.
     - Las salidas de `entry2` (V1, V2, V3 y piloto) siguen idénticas byte a byte.
   - **Prueba en vivo (2026-10-21, Alchemy)**: 6 tokens de V3 inactivos (2 EVITAR, 2 SIN BANDERAS, 1 mayhem y 1 sin T_entry2, este con 1 firma fallida en la curva).
     - El veredicto y el informe JSON coinciden **campo a campo** con `--offline`, 20 días después de la creación.
     - La creación se detectó bien en los 6.
     - Coste: 120–760 CU por token, media 333.
     - El graduado más activo de V3 (`BsaissLq…`) llega a la creación en 10 páginas, pero su ventana tiene 1 659 tx. Da NO EVALUABLE por coste sin pedirlas: 440 CU.
     - Con `--max-pages 5` corta en 5 páginas (200 CU).
     - Tras la prueba se añadió `--max-pages` (25 por defecto): antes, el listado podía gastar las 400 peticiones antes de rendirse.
     - Total de la prueba: 3 000 CU, sondeo incluido.
   - **Pendiente**: un token recién creado (ventana abierta, commitment `confirmed`) sigue sin probarse.
   - **Casos que el pre-registro no cubre**:
     - Evaluar con la ventana de 5 min aún abierta: se da veredicto si ya hay T_entry2, porque las banderas solo usan trades anteriores.
     - Tokens antiguos o muy activos: el listado hasta la creación agota el tope y no hay veredicto.
     - Datos a commitment `confirmed` en tiempo real, frente a descargas días después en las validaciones.
   **`marxol check` en vivo con tokens recién creados (2026-10-21, Alchemy) — herramienta, sin pre-registro** (`resultados/check_en_vivo_2026-10-21.txt`):
   - 13 tokens descubiertos con `scan --address <mint-authority>` (~40 CU por creación) y 18 consultas a 22 s, ~1 min, ~5.5 min y ~30 min de edad. Tiempos medidos con una copia instrumentada en el scratchpad; el código del repo no se tocó.
   - **Latencia**: con veredicto, 0.15–2.3 s, lineal en las tx de la ventana (~50 ms por tx, el ritmo de 20 req/s; red ~32 ms, decodificación ≤ 16 ms, evaluación 0 ms). Basta como aviso si se consulta en cuanto hay T_entry2.
   - **Ventana abierta**: E9 (`692WB76e…`) dio EVITAR a los 62 s, a los 5.5 min y a los 29 min, con los mismos 42 trades (1 de 1 token activo legible en las tres edades).
   - **Bloqueante — IDL desactualizado**: 5 de 18 consultas (5 de 8 tokens activos) fallan con un evento de discriminador desconocido `74 2b 4d bd 11 7a 48 2b`, que emiten instrucciones pump.fun que el IDL no conoce (en una tx hay 6 sin ningún `TradeEvent`). Podría ser una variante nueva de trade (sin verificar). `check` se niega a dar veredicto, pero gasta la cuota antes (hasta 4 400 CU). No se ha cambiado ni el IDL ni el código.
   - `--offline`: ninguno de los 13 mints está en `marxol.db`.
   - Consumo: **18 080 CU** (check 17 120, scan 800, tx 160).
   **Señal de creador repetido (2026-10-21) — EXPLORACIÓN POST HOC, SIN PRE-REGISTRO, SIN VEREDICTO** (`scripts/creador_senal_explora.py` → `resultados/creador_senal_explora.txt`, sin red, sobre una copia de `marxol.db`):
   - Señal S = otra creación del mismo `creator` en la franja con slot < slot de T_entry2. Con un token por creator es degenerada (S_t0 = 0 por construcción), así que se usan todos los tokens con bootstrap por clúster.
   - Cobertura: S en 84 de 503 comparables sin mayhem (V1 18, V2 57, V3 9; en V3, los 5 que pasan D2 con S son de un solo creator).
   - Juntas, dentro de los que pasan D2: +1.5 pts (MW unilateral p = 0.11, IC clúster [−1.5, +2.8]). Dentro de los descartados: +8.5 pts (p = 0.08, IC [−6.8, +23.2]). Ningún grupo con mediana positiva; 0 % de los tokens con S tiene retorno > 0 a +30 min.
   - Lectura: no separa grupos dentro de D2 y no cambia qué token se evita. El informe incluye un borrador de H11-D2, no congelado, con la estimación de coste del historial (≈ 7 000–15 000 CU por franja con firmas del payer ∩ mint-authority).
   **IDL de pump.fun actualizado (2026-10-21) — herramienta, sin cambio de definiciones** (`resultados/idl_actualizacion_2026-10-21.txt`):
   - El discriminador `74 2b 4d bd 11 7a 48 2b` es `SweepBondingCurveFeeEvent`, que emiten las instrucciones nuevas `sweep_protocol_fee` y `sweep_creator_fee` al barrer comisiones acumuladas en la curva. **No es un trade**: trae `timestamp, mint, bonding_curve, quote_mint, recipient, amount, bucket`, sin usuario ni reservas.
   - La cuenta IDL on-chain (`AYgC53tU…`) no se actualiza y es igual al IDL viejo. El vigente es el de GitHub (commit 2026-10-13, sha256 `7616681e…`): 56 instrucciones (9 nuevas, entre ellas `buy_v3`, `sell_v3`, `multi_hop_curve_swap` y los dos `sweep_*`), 32 eventos (4 nuevos, entre ellos `PostCompleteBuyEvent`, una compra sobre una curva completada que no emite `TradeEvent`) y campos añadidos al final en `TradeEvent`, `CreateEvent`, `BondingCurve` y `Global`.
   - El último despliegue es del 2026-10-19 17:47 UTC (slot 458 921 440). Hubo despliegues el 30-09 21:13 y el 01-10 07:45 UTC, antes de V3. No se ha localizado la primera tx con el evento.
   - El `TradeEvent` real de hoy lleva 8 bytes más que el IDL de GitHub: un campo sin documentar del despliegue del 18/19-10. Se tolera como bytes sobrantes.
   - **Impacto en V1–V3: ninguno**:
     - Las 973 ventanas tempranas y de precio tienen `complete = 1` y `fetch_errors = 0`, y un evento desconocido las habría marcado incompletas.
     - El encadenado de reservas se completa en 718 de 720 tokens con T_entry2; las 2 roturas son bucles compra/venta del mismo importe, no huecos.
   - Cambios:
     - `idl/pump.json` ← GitHub. `verify`: `Global` con 30 campos y 0 bytes sobrantes; antes, 1 sobrante.
     - `tx::decode_events` separado de `fetch_events`.
     - `check` lee la ventana con `ingest_window`: para en la primera tx no decodificable y el error dice el discriminador.
     - 6 tests nuevos con 22 tx reales en `tests/fixtures/` (66 en verde).
   - Salidas de `entry2`, `entry`, `h1`, `h1b`, `check --offline` (973 tokens) y scripts post hoc: **idénticas byte a byte** antes y después (61 ficheros).
   - En vivo: `4mekvGzn…`, que antes fallaba, da SIN BANDERAS DE D2 con la ventana completa.
   - Consumo: ≈ 3 300 CU (cota superior).
   - Fallo previo y ajeno: `scripts/cribado_posthoc.py` falla con los datos de V3 (pump-y-caída n/e), con los dos binarios.
   **`verify` endurecido, aviso de bytes de más y prueba en vivo 2 (2026-10-22) — herramienta** (`resultados/check_en_vivo_2_2026-10-22.txt`):
   - `verify` sale con error si `Global` tiene bytes sobrantes o le faltan campos (`global_exact`). Hoy pasa: 30 campos y 0 sobrantes.
   - Los `TradeEvent` con más bytes que el IDL se cuentan (`tx::TRADE_EXTRA_BYTES`). Se avisa una sola vez por ejecución con la longitud observada, y `check`, `windows`, `prices` e `index-tramo` imprimen el contador al final. Hay 3 tests nuevos (69 en verde), y las salidas de V1–V3 siguen idénticas byte a byte (61 ficheros).
   - `docs/referencia_tecnica.md` §6 actualizada con el IDL vigente y el procedimiento tras cada despliegue.
   - Prueba en vivo con 20 creaciones a ~60–180 s, `--max-requests 35`: 0 errores.
     - 6 veredictos (4 SIN BANDERAS, 2 EVITAR) en 0.8–4.8 s.
     - 6 sin T_entry2 todavía.
     - 8 NO EVALUABLE por coste (ventanas de 45–360 tx).
   - **Online frente a offline**: los 12 baratos se incorporaron a una copia de `marxol.db` en el scratchpad con `index-tramo` + `windows`. Dan 12 de 12 veredictos, banderas y T_entry2 iguales; solo cambia `window_trades`, por los trades posteriores a la consulta.
   - Latencias anómalas (hasta 37.6 s con 16 peticiones), sin explicar. Una posibilidad es que fueran reintentos 429 no registrados: 20 req/s × 40 CU supera los 500 CU/s del free tier.
   - Consumo: ≈ 12 300 CU.
   **Reintentos 429, ritmo de 12 req/s y consulta temprana (2026-10-22) — herramienta** (`resultados/check_temprano_2026-10-22.txt`):
   - `rpc::with_retry` avisa en stderr de cada reintento por 429: proveedor, método, qué se pedía (mint, firma o curva), nº de reintento y espera. Nunca imprime la URL. Al final sale el total, junto al contador de bytes de más.
   - El ritmo por defecto de Alchemy baja de 20 a **12 req/s** (480 CU/s < 500); se cambia con `--max-rps` / `MARXOL_MAX_RPS`.
     - Una descarga de N tx tarda ahora ≥ N/12 s: las latencias medidas el 2026-10-21, ~50 ms por tx, eran a 20 req/s.
   - `check` con NO EVALUABLE por tope dice cuántas peticiones faltaban y el coste en CU para completar la ventana, y sugiere consultar cerca de T_entry2. También corrige el texto, que citaba 400 aunque el tope fuera otro.
   - 3 tests nuevos (72 en verde); las salidas de V1–V3 siguen idénticas byte a byte.
   - Consulta a ~30 s frente a ~120–145 s (7 creaciones):
     - Veredicto y T_entry2 iguales en 1 de 1 token comparado.
     - En tokens tranquilos ahorra poco: 360 frente a 600 CU.
     - En los muy activos no basta: a 30 s ya hay ventanas de 27–103 tx (hasta ≈ 4 200 CU). El coste depende del volumen, no de la edad.
     - 0 reintentos 429 a 12 req/s.
   - Consumo: 2 680 CU.
   **Estado (2026-10-22): APARCADO por decisión del dueño** (`CLAUDE.md`, "Rumbo").
   - No hay señal de entrada rentable en pump.fun con estos datos. D2 es un filtro de descarte validado y `marxol check` lo implementa: funciona y es fiable, con cobertura limitada en tokens muy activos.
   - La señal de creador repetido no mejoró D2 (post hoc).
   - Operadores (H2, H3, H11–H13): sin explorar y sin pre-registrar.
   - Para retomar:
     1. Revisar el IDL tras cada despliegue en ProgramData (`docs/referencia_tecnica.md` §6).
     2. Si se reabre la línea de operadores, fijar el presupuesto de franjas y el criterio de parada ANTES de recoger datos.
     3. Los límites de uso de `check` están documentados en `resultados/check_en_vivo_2026-10-21.txt`, `check_en_vivo_2_2026-10-22.txt` y `check_temprano_2026-10-22.txt`.
   **Estado (2026-10-22, después): REABIERTO por decisión del dueño** (`CLAUDE.md`, "Rumbo"). Orientación nueva: veto/descarte y reputación de creador, en cinco fases (núcleo/CLI, recolección continua en base aparte, métricas de creador, veto en `check`, validación fuera de muestra). Fase 0 (auditoría, 0 CU): `windows` ≈ 2 710 CU/token, `prices` ≈ 690, `index-tramo` ≈ 82 por creación, `check` 80–1 040 en vivo; `tx`, `scan`, `index`, `verify`, `global` y `curve` no registran su consumo en `rpc_usage`. Indexar todas las creaciones (35–70 k/día) costaría 1.4–2.8 M CU/día: la recolección tiene que muestrear.

   **Fase 2 (2026-10-22): recolección continua lista, sin lanzar en continuo** (docs/recoleccion.md).
   - D2 y las métricas actuales se congelaron antes de recolectar (docs/congelado_vivo_2026-10-22.md, commit `a74a456`).
   - Libro de CU diario compartido en `marxol_vivo.db`: bolsa `collect` de 700 000 y bolsa manual hasta 1 000 000. Lo usan todos los comandos con red.
   - `marxol collect`: muestreo por hash (24 h), fecha y hora de captura, WAL, punto de reanudación, retención con exportación verificada.
   - Medición (resultados/collect_medida_2026-10-22.txt, 5 580 CU):
     - ~51 000 firmas de mint-authority al día;
     - 40 CU por ciclo de listado;
     - ~432 CU por token medidos (13 tokens nocturnos), 3 444 en V1–V3;
     - con 4 000 ppm, ≈ 96 000 CU/día con el coste medido y ≈ 686 000 con la cota pesimista.
   - Falta el OK del dueño para dejarlo en continuo.

   **Recolección en continuo desde 2026-10-22 02:15:51 UTC** (OK del dueño):
   - `collect --sample-ppm 4000 --poll-secs 300`, topes por defecto.
   - Corre con `run/collect.sh`: proceso desligado con reinicio, binario copiado en `run/` y log en `run/collect.log` (fuera de git).
   - **Parada y reinicio (2026-10-22)**:
     - Último ciclo a las 04:38 UTC. El log no muestra ninguna salida del proceso: la máquina se suspendió o se reinició.
     - Relanzado a las 16:24:39 UTC con el mismo binario (`cad4ce6`): `setsid nohup run/collect.sh >> run/collect.out 2>> run/collect.log < /dev/null &`.
     - `run/collect.pid` guarda el pid del bucle, que es líder de su grupo. Para pararlo: `kill -- -$(cat run/collect.pid)`.
     - El hueco (~12 h, rodaje) cabe en `--max-backfill-pages 100` y se recupera entero. No afecta al bloque A.
   - Pre-registro de la fase 5, escrito antes de leer ningún resultado: docs/preregistro_fase5_2026-10-22.md (bloques A, B y C por fecha).
   - Propuesta de medición de creadores en serie: docs/propuesta_creadores_serie.md. Hallazgo: el `creator` es un argumento de `create`/`create_v2`, no una cuenta; solo el pagador aparece en la tx.

   **Fase 3 (2026-10-22, post hoc sobre marxol.db)**:
   - Exploración sin red en resultados/fase3_explora.txt (comisión de creador, coste de lanzar, compras en el slot de creación).
   - C' etapa 1 (resultados/fase3_cprima_etapa1.txt, 20 440 CU), sobre 100 tokens de V3: 80 tienen un pagador con ≥ 1 lanzamiento previo en 7 d (mínimo), y 16 de 62 pagadores tienen ≥ 50.
   - Dentro de la muestra solo se veía en 56. El rendimiento es alto; la etapa 2 queda pendiente del OK del dueño.

   - C' etapa 2 (resultados/fase3_cprima.txt), 933 tokens de V1–V3 y 513 pagadores, 108 280 CU en total con la etapa 1:
     - 75,7 % [IC 72,8–78,3] de 904 tokens clasificables tienen un pagador con ≥ 1 lanzamiento previo en 7 d (mínimo), y 41,7 % [38,5–44,9] uno con ≥ 50.
     - Pagadores con ≥ 50 (103), según una muestra de 10 tx de creación cada uno:
       - 45 "operador único" (un solo creator; 288 tokens);
       - 58 "posible servicio" (≥ 2 creators; 101 tokens).
   - Coste de lanzar medido en 187 tx de creación: mediana 0,0097 SOL, p90 0,021.
   - Todo es exploratorio: ninguna razón de veto sale de aquí sin su adenda y el bloque B.

   **Adenda 3 (2026-10-27, aprobada por el dueño; sin red, sin resultados de marxol_vivo.db)**: docs/adenda_3_2026-10-27.md (sha256 en el commit); plan de commits y escaneos en docs/plan_commit_adenda_3.md. Solo tablas de operación de marxol_vivo.db (resultados/cobertura_adenda3.txt, `scripts/cobertura_adenda3.py`).
     - Mínimos en tokens evaluables (con T_entry2, ventanas completas y en SOL): A ≥ 900 y B ≥ 1 800. Ampliación por días UTC; sustituye la regla «no se amplía el bloque» (pre-registro y adenda 1). Ritmo esperado ≈ 47 evaluables/día (prudente 39): A ≈ hasta 2026-11-12 (tope 2026-11-16), B ≈ 39 días (tope inicio + 47 días, como muy tarde 2027-01-02).
     - Bloque A hasta el 27-10 19:27 UTC: 880 `creado`, 544 con un token por creator y sin mayhem (113/día). Tope de 400 tx: 39/880 = 4,4 %. Incompletos 59 (39 tope, 20 max_pages; 0 `pendiente_agotado`). Franjas > 10 %: 3 de 19. Retraso de captura: mediana 3,2 h, p90 14,0 h; incompletos < 1 h / 1–6 h / 6–24 h: 6,4 / 8,6 / 6,2 %.
     - Sin campo nuevo de creación (BOOST u otro); creator_fee_bps > 0 pasa del 9,6 % (V1–V3) al 21,6 %; quote ≠ SOL 29 %. `depth` no se guarda (no medido).
     - Potencia (resultados/potencia_adenda3.txt, retornos de V1–V3 agregados): por el suelo de 10 pts el MDE80 queda en 11–12 pts y nunca baja de 11; con 10 pts verdaderos la potencia es 0,43–0,61 con cualquier n. "No concluyente" es un resultado esperable y válido para R1 y R2.

   **Compras ajenas tras el slot de creación (2026-10-27, post hoc, descriptivo, sin red, sin retornos)**: resultados/compras_ajenas_posthoc.txt (`scripts/compras_ajenas_posthoc.py`). Solo marxol.db + marxol_fase3.db (R1); no abre marxol_vivo.db.
   - Población: 538 tokens con ventana completa, sin mayhem, un token por creator. Compra ajena = compra de una cuenta ≠ creator y ≠ pagador en slot > slot de creación.
   - Todos: ≥ 1 compra ajena en 60 s 67,1 % [63,0–70,9]; en 5 min 69,5 % [65,5–73,3]; ≥ 3 cuentas ajenas en 5 min 53,3 % [49,1–57,5]. 1.ª compra ajena: mediana 3 s, p90 > 300 s (censura); entre los 374 que la tienen, mediana 1 s, p90 14 s. Casi todo lo que llega, llega en segundos.
   - Por compra del creador en el slot (% supply): 0 % → 47,1 % con compra en 5 min (n = 34); (0, 2] → 56,8 % (185); (2, 5] → 74,6 % (122); (5, 10] → 80,0 % (105); > 10 % → 84,8 % (92).
   - Otras cuentas en el mismo slot de creación: sí 86,0 % [81,7–89,4] (n = 314), no 46,4 % [40,0–53,0] (n = 224).
   - R1 (adenda 1): sí 65,5 % [59,0–71,4] (n = 223; 5–49: 54,5 %, n = 123; ≥ 50: 79,0 %, n = 100), no 73,4 % [67,9–78,3] (n = 271); 9 n/e y 35 del piloto sin dato.
   - Solo descriptivo. Nada decide; si alguna variable se usa luego, va con adenda y se valida en el bloque B.
   - Segunda pasada (2026-10-27, misma población y definiciones): resultados/compras_ajenas2_posthoc.txt (`scripts/compras_ajenas2_posthoc.py --copia <copia de marxol.db>`). D2 sale de `entry2 --check --json` (sin retornos); control frente al `d2` de V3: 158 tokens, 0 distintos.
     - Δslots hasta la 1.ª compra ajena (374 con ella): 1 slot 37,2 % [32,4–42,2], 2 slots 4,5 %; 1–2 slots 41,7 % [36,8–46,8], más tarde 58,3 %. Mediana 4 slots, p90 52. Sobre los 538: 1–2 slots 29,0 %, más tarde 40,5 %, ninguna 30,5 %.
     - Compras ajenas en 60 s: 9 260 de 4 990 wallets. Top 10 hace 4,6 % [4,2–5,0], top 50 10,9 % [10,3–11,5]. Wallets en ≥ 2 tokens: 25,6 % [24,4–26,8] de las wallets y 51,9 % de las compras; están en 61,9 % [57,7–65,9] de los tokens. Concentración baja en el top; repetición alta en la cola media.
     - Sin compra ajena en 60 s (n = 177): 42,4 % [35,3–49,7] sin T_entry2 (D2 no aplica). De los 102 evaluables, D2 descarta 2,0 % [0,5–6,9] (H4' 9,8 %, H7' 10,8 %, H6' 2,0 %; alguno 19,6 %). Con compra ajena en 60 s (351 evaluables): D2 descarta 55,3 % [50,0–60,4]. El grupo sin compradores tempranos casi no lo toca D2: no se solapan.
     - Pagadores con R1 ≥ 50 en la población (79; muestra de 10 tx de creación de marxol_fase3.db): "posible servicio" (≥ 2 creators) 73,4 % [62,8–81,9] con 79 de 100 tokens; "operador único" 26,6 % con 21 tokens, todos con creator = pagador. No distingue servicio de operador que rota creators.
   - Tercera pasada (2026-10-27): resultados/compras_ajenas3_posthoc.txt (`scripts/compras_ajenas3_posthoc.py --copia …`). Retornos solo de V1–V3 en marxol.db (ya vistos: no decide nada). Controles: trade T_entry2 reconstruido = entry2 en los 453 tokens con T_entry2; retorno (a) +30 min recalculado = `entry2 --json` de V1 (188 tokens, 0 distintos).
     - De los 351 tokens con compra ajena en 60 s y T_entry2, la 1.ª compra ajena va antes de T_entry2 en el 88,9 % [85,2–91,8], es el propio trade de T_entry2 en el 7,7 % y va después en el 3,4 %. En T_entry2 o después: 11,1 % [8,2–14,8].
     - Medibles antes de T_entry2: la compra del creador en el slot de creación y otras cuentas en ese slot (T_entry2 siempre cae en un slot posterior) y R1 (historial previo, aunque pide RPC). "Compra ajena en 60 s / 5 min" y "≥ 3 cuentas en 5 min" no lo son tal como están definidas; sí lo sería "compra ajena antes de T_entry2". La repetición de wallets entre tokens usa tokens posteriores: así es fuga, y solo valdría con historial previo.
     - Wallets en ≥ 2 tokens (1 278): en ≥ 2 días UTC distintos 24,8 % [22,5–27,2] (248 en 2 días, 69 en 3); en un solo día 75,2 %.
     - Retorno (a) +30 min, V1–V3, evaluables por D2 (mediana [IC 95 %], % ≤ −50 %):
       - sin compra ajena en 60 s (100): −5,6 % [−7,4, −4,1], 0/100 ≤ −50 %; 98 pasan D2;
       - 1.ª compra ajena a 1–2 slots (143): −13,3 % [−19,5, −8,5], 11,2 % [7,0–17,4]; pasan D2 −6,5 %, descartados −32,0 %;
       - a > 2 slots (183): −14,4 % [−19,6, −11,5], 9,3 % [5,9–14,4]; pasan D2 −8,7 %, descartados −25,2 %.
       - Sin T_entry2 (65 + 5 + 4) el retorno (a) no existe. Sustituto bruto desde t0 (no comparable): mediana 0,0 %.
     - Control ampliado (2026-10-27): el retorno (a) recalculado coincide con `entry2 --json` también en V2 (167 tokens) y V3 (74, incluido 1 n/e), con 0 diferencias. La tabla queda validada en los tres tramos.
     - Inventario de los 41 cortes exploratorios sobre retornos de V1–V3 (1 961 con las reglas de salida): docs/borrador_adenda_3.md. "Compra ajena" no entra como métrica.
     - `marxol check` sin T_entry2 (antes del commit 14f0fa3): imprimía "VEREDICTO: NO EVALUABLE — sin T_entry2: …", sin banderas, con los avisos fijos (que citan el −5,6 % de "los que pasan D2", que no aplica), y sale con código 3. No dice "aprobado", pero tampoco da ninguna cifra histórica de este grupo, y un flujo que solo filtre "EVITAR" lo dejaría pasar. Desde 14f0fa3 (solo presentación): «SIN DATOS SUFICIENTES … NO es una aprobación», sin el aviso del −5,6 %, con el recuento de V1–V3 (241 de 933, 39 mayhem, ninguno con retorno (a)) como contexto. Mantiene el código de salida 3 y `verdict` = NO EVALUABLE en el JSON. Referencia: resultados/check_sin_tentry2_2026-10-27.txt.

   **Auditoría fase A (2026-10-22, post hoc, sin red, sin retornos)**: resultados/auditoria_fase_a.txt (`scripts/auditoria_fase_a.py`). 530 tokens sin mayhem con T_entry2 de piloto y V1–V3. El script reproduce T_entry2, H4', H5, H7' y H8 de `entry2 --json` sin ninguna discrepancia.
   - El creador compra en el slot de creación en el 91,3 % [88,6–93,4] de los tokens (88,9 % en la propia tx). Casi no discrimina, así que no entra como variable. Ningún token tiene compras de otra cuenta dentro de la tx de creación.
   - Tx con compras de ≥ 2 cuentas antes de T_entry2: 13,0 % [10,4–16,2]. D2 ya descarta el 89,9 % de esos tokens (frente al 33,8 % del resto).
   - Tenencia del top 10 de cuentas ajenas en T_entry2: Spearman 0,82 con el llenado de H6'. Es casi la misma variable.
   - Tenencia del creador > 5 % del supply: 15,8 % [13,0–19,2]. D2 descarta casi lo mismo con ella que sin ella (41,7 % frente a 41,0 %).
   - No hay pagador de comisión ni lista de cuentas por tx guardados. El `user` del `TradeEvent` es el firmante del trade.
   - Mayhem: 0 tx con varias cuentas y 1,1 % con compradores ajenos en el slot de creación.

   **Banco de evaluación, fase B (2026-10-22)**: `src/banco.rs` y `marxol banco-d2` (resultados/banco_d2_v3.txt). Es descriptivo y post hoc: no cambia el veredicto de V3.
   - Reproduce exactamente los criterios congelados de V3 (46/28, +19,0 pts, p = 0,000159, IC [+11,2, +27,2]).
   - Las 21 salidas de referencia y los 1 015 `check --offline` son idénticos byte a byte. sha256 de marxol.db intacto.
   - Con clase positiva "retorno (a) a +30 min ≤ 0", la prevalencia es del 94,6 % [86,9–97,9]. El modelo trivial (vetar todo) tiene una exactitud del 94,6 %, frente al 35,1 % de D2. El MCC de D2 es −0,18 [−0,37, +0,09].
   - De los 4 tokens con retorno > 0, D2 descarta 3.
   - D2 separa medianas, pero no clasifica "positivo/negativo" mejor que el modelo trivial: casi todo pierde.
   - Fragilidad: quitando los 5 casos que más sostienen la diferencia, queda en +16,2 pts (p = 0,0031).
   - Bloques de 4 min: +12,3 pts (p = 0,057) y +24,0 pts (p = 0,00011).
   - Censura: a +24 h, 0/75. marxol.db solo tiene 65 min de precio.

   **Fase C (2026-10-22, exploratorio y post hoc, sin veredicto)**: `marxol banco-fase-c` (resultados/fase_c_banco.txt).
   - Declaración previa escrita antes de cruzar nada con retornos: docs/fase_c_declaracion_2026-10-22.md (sha256 `e8fbe56c…`, 16:27:50 UTC). El cálculo se ejecutó a las 16:31:49 UTC.
   - Población: 438 tokens de V1–V3, sin mayhem, un token por creator y con retorno (a) a +30 min.
   - **C1 (tenencia del creador > 5 %), 15,3 %**:
     - Medianas: −24,7 % con C1 frente a −8,2 % sin C1. Diferencia +16,6 pts [+11,8, +23,9].
     - Misma dirección en V1 (+11,9), V2 (+25,1) y V3 (+18,4).
     - Sobrevive al control por estratos de actividad: ≤ 2 trades +9,6, 3–9 trades +17,6, ≥ 10 trades +13,3. Diferencia ponderada +13,5 [+8,7, +19,3].
     - Dentro de D2 también: +11,2 pts en los que pasan y +10,2 en los descartados. Aporta algo que D2 no tiene.
     - Como clasificador, MCC ≈ 0: el 97 % de los tokens tiene retorno ≤ 0.
     - La variable es un % del supply, no de lo que circula, así que no sube con pocos trades: el 10,9 % de los tokens con ≤ 2 trades tienen C1, frente al 22,6 % de los de 3–9 trades.
   - **C2 (tx con ≥ 2 cuentas), 11,9 %**:
     - Diferencia bruta de +20,1 pts, pero **no sobrevive** al control: ponderada +11,7 [−1,9, +17,1], van Elteren p = 0,051.
     - Ningún token con ≤ 2 trades tiene C2.
   - Con la etiqueta "≤ −50 %" (prevalencia 7,5 %), C1 tiene MCC 0,047 [−0,05, +0,15] y C2 0,163 [+0,04, +0,29].
   - **Control de C1 por progreso de la curva en T_entry2 (2026-10-22, post hoc, declarado antes: docs/fase_c_control_progreso_2026-10-22.md): C1 NO sobrevive.**
     - Terciles de progreso: 0/146 con C1 en el bajo; +6,0 pts [+3,7, +7,6] en el medio; −2,5 [−8,9, +3,9] en el alto.
     - Diferencia ponderada +1,8 [−1,5, +5,0].
     - Detalle: resultados/fase_c_control_progreso.txt y docs/informe_noche_2026-10-22.md.

   **Amenaza a la validez: cambio del `TradeEvent` el 2026-10-19 (verificado contra la chain el 2026-10-22, 880 CU).**
   - Desde el despliegue del **2026-10-19 15:46:16 UTC (slot 458 888 140)**, el `TradeEvent` lleva al final un campo nuevo, `quote_transfer_fee: u64`: 8 bytes, que dan 382 en vez de 374 en un `buy` típico.
     - Acotación: el último evento de 374 se vio en el slot 458 888 128 y el primero de 382 en el 458 921 419. Entre ambos solo hay ese despliegue.
     - El campo está en el IDL del SDK `@pump-fun/pump-sdk` 4.2.0 (2026-10-19 14:28 UTC), pero todavía no en el IDL de GitHub ni en `idl/pump.json`.
   - Significado, según el SDK: es la comisión de transferencia Token-2022 del mint de quote, que se cobra encima de la entrada (compras) o se descuenta de la salida (ventas). Vale 0 cuando el quote es SOL.
   - **El parser es seguro**: el campo va al final y el resto de campos decodifica igual. En 11 trades reales (6 de antes del cambio y 5 de después) cuadran la constante de la curva, las reservas reales, las comisiones y el paso creación → trade.
   - **Efecto**: el retorno congelado no descuenta esta comisión. Solo puede afectar a los tokens con un quote Token-2022 que la cobre. V1–V3 son anteriores al cambio. El rodaje y los bloques A y B quedan después.
   - Detalle: docs/informe_noche_2_2026-10-23.md.

   **Cambio de protocolo de recolección (2026-10-26 18:40:53 UTC, reinicio de `marxol-collect`)** — autorizado por el dueño, solo lo de la fase 2 de docs/informe_2026-10-26.md:
   - **Causa, el bucle**: `pending_windows` / `pending_price_windows` devolvían para siempre los tokens con `complete = 0`, los más antiguos primero. Los 40 con fallo determinista (17 + 9 del bloque A, 14 del rodaje) más 5 de precio ocupaban cada ciclo y se volvían a listar en cada pasada: 10 635 reintentos, ≈ 1,36 M CU estimados (44 % del gasto de collect) y 6 151 reintentos por 429. La bolsa se agotó el 25-10 a las 08:37 UTC y el 26-10 a las 06:03 UTC. Medición: resultados/fase1_bucle_2026-10-26.txt (`scripts/fase1_bucle_2026-10-26.py`, solo lectura, sin retornos).
   - **Fallo determinista** (tope de 400 tx por token, max_pages sin llegar a la creación o al rango) → fila en `vivo_download_final` con estado `incompleto_permanente` y su motivo. No se reintenta nunca.
   - **Fallo transitorio** (red, 429 tras los reintentos de `with_retry`, errores de `getTransaction` o de decodificación) → como mucho 3 intentos por token (`download_attempts`). Después pasa a `pendiente_agotado`, con su motivo.
   - En los dos casos la fila de `trade_windows` / `price_windows` sigue con `complete = 0` y su nota, así que el token sigue contando en censura y exclusiones. La agregación (`finished_without_result`) no cambia.
   - Al arrancar, `vivo::sweep_download_backlog` clasifica por su nota lo incompleto de antes del cambio, sin red: 45 `incompleto_permanente` (40 ventanas y 5 de precio) y 0 `pendiente_agotado`.
   - **Ritmo horario**: el libro reparte la bolsa `collect` por hora UTC (`ledger::hourly_cap`). Al final de la hora h el tope acumulado es 700 000 · (h + 1) / 24 (≈ 29 167 CU/h) y lo no gastado pasa a las horas siguientes. Al tope horario, `collect` espera a la hora siguiente; al diario, al día siguiente. Los comandos manuales no llevan ritmo horario.
   - Sin cambios: selección por hash de firma, 4 000 ppm, sondeo de 300 s, tope de 400 tx, `max_pages` 5 y ningún cálculo congelado (D2, T_entry2, retornos). `windows` y `prices` sobre marxol.db usan las consultas de siempre (`skip_final = false`).
   - Tests: 99 en verde (3 nuevos). Las salidas de referencia sobre una copia de marxol.db (17 comandos y 1 015 `check --offline`) son idénticas byte a byte, y el sha256 de marxol.db no cambia (`d513e1f3…`).
   - Binario anterior guardado en `run/marxol.antes-2026-10-26`.
   - **Primeros 3 ciclos (2026-10-27, 00:00–02:00 UTC)**:
     - Cada ciclo gasta ~29 170 CU (el tope horario) en vaciar la cola atrasada. Los 54 tokens descargados tienen 1 solo intento y los 3 topes nuevos quedaron permanentes al primero.
     - La clave nueva funciona y hubo 13 reintentos por 429.
     - Hallazgo: el 26-10, entre las 16 y las 17 UTC, hubo 61 867 firmas de mint-authority, 59 007 de ellas fallidas. La recuperación usó 100 páginas, justo `--max-backfill-pages`, sin hueco.

6. Registrar cada hallazgo nuevo en este archivo en la misma sesión en que se confirme.
