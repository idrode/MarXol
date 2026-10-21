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
6. Registrar cada hallazgo nuevo en este archivo en la misma sesión en que se confirme.
