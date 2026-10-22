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

6. Registrar cada hallazgo nuevo en este archivo en la misma sesión en que se confirme.
