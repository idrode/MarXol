# Plan de commits — adenda 3 (2026-10-27)

**Nada de esto está hecho.** No hay commit ni push; el OK final es del dueño. El orden importa: la adenda 3 (commit 6) tiene que quedar comprometida **antes del 2026-10-30 00:00 UTC**, el fin nominal del bloque A, y antes de leer cualquier resultado de `marxol_vivo.db`.

Último commit en `main`: `14f0fa3` (check: presentación del caso sin T_entry2), sin push.

## 1. Qué hay sin commitear y de dónde viene

`git diff --stat`: 14 ficheros, +995 / −33. Además hay 25 ficheros sin seguimiento. Son cuatro trabajos distintos mezclados, algunos en el mismo fichero.

**Lo que queda en `src/check.rs` fuera de `14f0fa3`**: un solo hunk de test, `trade_sizes: vec![]` en el constructor `txe` del módulo de tests. Viene del campo nuevo `TxEvents::trade_sizes` de `src/tx.rs` (diagnóstico de bytes sobrantes del `TradeEvent`, 2026-10-22). Sin él, los tests de `check.rs` no compilan contra el `tx.rs` del árbol. El mismo hunk está en el test de `src/store.rs`.

**Lo que queda en `CLAUDE.md` fuera de `14f0fa3`** (6 líneas):
- Estado actual: «Amenaza a la validez (2026-10-19 15:46:16 UTC…)» → `quote_transfer_fee`.
- Estado actual: «Recolección (2026-10-26 18:40:53 UTC)…» → arreglo del bucle de `collect`.
- Tabla de comandos: filas de `marxol banco-d2` y `marxol banco-fase-c`.
- Módulos: líneas de `src/banco.rs` y `src/concentracion.rs`.

Ninguno de estos cambios es de la sesión de hoy salvo el commit `14f0fa3` y los ficheros de la adenda 3.

## 2. Commits propuestos

Varios ficheros (`src/main.rs`, `src/store.rs`, `CLAUDE.md`, `docs/estado_y_pasos.md`) tienen hunks de más de un commit. Hay que repartirlos por hunk (`git add -p`, o `git apply --cached` con el parche recortado, como en `14f0fa3`). Cada commit se compila y se prueba sobre el árbol indexado (`git checkout-index` a una carpeta temporal + `cargo test`) antes de crearlo.

### Commit 1 — diagnóstico del `TradeEvent` (`quote_transfer_fee`)

- `src/tx.rs`: campo `trade_sizes`.
- `src/check.rs` y `src/store.rs`: solo el hunk de test `trade_sizes: vec![]`.
- `src/main.rs`: solo los hunks de `scan` (`--before`, `--solo-firmas`).
- `docs/referencia_tecnica.md`: `quote_transfer_fee`.
- `docs/informe_noche_2_2026-10-23.md`.
- `docs/estado_y_pasos.md`: párrafo «Amenaza a la validez: cambio del `TradeEvent` el 2026-10-19».
- `CLAUDE.md`: línea «Amenaza a la validez».

Mensaje:
```
scan: diagnóstico de bytes sobrantes del TradeEvent (quote_transfer_fee, 2026-10-19)

Solo diagnóstico: TxEvents::trade_sizes, scan --before y --solo-firmas. No cambia
la decodificación ni ningún cálculo. Registra la amenaza a la validez del retorno
congelado (no descuenta quote_transfer_fee) en referencia_tecnica, estado_y_pasos
y CLAUDE.md.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
```

### Commit 2 — protocolo de recolección del 2026-10-26 18:40:53 UTC

- `src/ledger.rs`, `src/vivo.rs`, `src/ingest.rs`, `src/collect.rs`.
- `src/store.rs`: solo `pending_windows` / `pending_price_windows` con `skip_final`.
- `src/main.rs`: solo el hunk del ritmo horario de `collect`.
- `docs/recoleccion.md`.
- `docs/informe_2026-10-26.md`.
- `scripts/fase1_bucle_2026-10-26.py` y `resultados/fase1_bucle_2026-10-26.txt`.
- `docs/estado_y_pasos.md`: párrafo «Cambio de protocolo de recolección».
- `CLAUDE.md`: línea «Recolección (2026-10-26 18:40:53 UTC)».

Mensaje:
```
collect: sin reintentos de fallos deterministas y ritmo horario de la bolsa (2026-10-26 18:40:53 UTC)

incompleto_permanente (tope de 400 tx, max_pages) y pendiente_agotado (3 intentos
ante fallos transitorios) en vivo_download_final; tope acumulado de la bolsa
collect = 700 000·(h+1)/24. Corrige el bucle que gastó ≈ 1,36 M CU (44 %) del
22-10 al 26-10. Sin cambios en muestreo, topes, D2, T_entry2 ni retornos; salidas
de referencia idénticas byte a byte. Es el binario que corre desde el reinicio.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
```

**Nota**: el binario en servicio (`run/marxol`, 2026-10-26 19:40) se compiló desde este árbol. Conviene que este commit sea el que lo reproduce. Para comprobarlo: compilar el commit y comparar con `sha256sum run/marxol` (puede no coincidir byte a byte si cambian las rutas o la versión de rustc).

### Commit 3 — banco de evaluación y fase C (post hoc)

- `src/banco.rs`, `src/concentracion.rs`, `src/lib.rs`, `src/report.rs`.
- `src/main.rs`: solo `BancoD2` y `BancoFaseC`.
- `src/store.rs`: solo `entry_trades_tx` y `creator_identities_base_before`.
- `docs/fase_c_declaracion_2026-10-22.md`, `docs/fase_c_control_progreso_2026-10-22.md`, `docs/informe_noche_2026-10-22.md`.
- `scripts/auditoria_fase_a.py`, `scripts/c1_vs_h6_progreso.py`.
- `resultados/auditoria_fase_a.txt`, `banco_d2_v3.txt`, `fase_c_banco.txt`, `fase_c_control_progreso.txt` y `c1_vs_h6_progreso.txt`.
- `docs/estado_y_pasos.md`: párrafos «Auditoría fase A», «Banco de evaluación, fase B» y «Fase C».
- `CLAUDE.md`: filas de `banco-d2` / `banco-fase-c` y líneas de los dos módulos.

Mensaje:
```
banco-d2 y banco-fase-c: banco de evaluación de reglas de veto y fase C (post hoc, sin veredicto)

Sin red; --db en solo lectura. banco-d2 reproduce exactamente los criterios
congelados de V3. Fase C (C1, C2) es exploratoria: C1 no sobrevive al control
por progreso de la curva y queda como nota descriptiva.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
```

### Commit 4 — borrador de la adenda 2 (sustituido) y su potencia

- `docs/borrador_adenda_2.md`, con una línea de cabecera añadida: «Sustituido por la adenda 3 (docs/adenda_3_2026-10-27.md), que incorpora sus cambios. No se aplica por separado».
- `scripts/potencia_adenda2.py` y `scripts/potencia_adenda2_r.py`.
- `resultados/potencia_adenda2.txt` y `resultados/potencia_adenda2_r.txt`.

Se versiona para que conste lo que se consideró, no porque esté vigente.

Mensaje:
```
Borrador de la adenda 2 (sustituido por la adenda 3) y su cálculo de potencia

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
```

### Commit 5 — compras ajenas (post hoc, descriptivo)

- `scripts/compras_ajenas_posthoc.py`, `compras_ajenas2_posthoc.py` y `compras_ajenas3_posthoc.py`.
- `resultados/compras_ajenas_posthoc.txt`, `compras_ajenas2_posthoc.txt` y `compras_ajenas3_posthoc.txt`.
- `docs/estado_y_pasos.md`: párrafo «Compras ajenas tras el slot de creación (2026-10-27)».

Mensaje:
```
Compras ajenas tras la creación: tres pasadas post hoc y descriptivas (sin veredicto)

Solo marxol.db y marxol_fase3.db, sin red. No es métrica ni razón de veto; queda
como corte exploratorio n.º 13 del inventario de la adenda 3.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
```

### Commit 6 — adenda 3 (antes del 2026-10-30 00:00 UTC)

- `git mv docs/borrador_adenda_3.md docs/adenda_3_2026-10-27.md`. El contenido no cambia, así que el sha256 sigue siendo el mismo.
- `docs/preregistro_fase5_2026-10-22.md`: una línea en «Registro de cambios» con la fecha, el nombre del fichero y su sha256. No se toca nada más del pre-registro.
- `scripts/cobertura_adenda3.py` y `resultados/cobertura_adenda3.txt`.
- `scripts/potencia_adenda3.py` y `resultados/potencia_adenda3.txt`.
- `docs/plan_commit_adenda_3.md` (este fichero).
- `docs/estado_y_pasos.md`: párrafo «Adenda 3, cerrada y sin commit».

Mensaje (el sha256 se recalcula justo antes del commit; si no coincide con el de abajo, se para):
```
Adenda 3 al pre-registro de la fase 5: protocolo, cobertura, ampliación y réplica de D2

sha256(docs/adenda_3_2026-10-27.md) = <SHA256_ADENDA_3>

Comprometida antes del fin del bloque A (2026-10-30 00:00 UTC) y sin leer
retornos ni resultados de marxol_vivo.db (solo tablas de operación). Incorpora
la adenda 2 (k = 2, p < 0,005; C1 solo descriptiva). No cambia D2, T_entry2, el
retorno (a), R1 ni R2. Fija reglas de cobertura, mínimos en tokens evaluables A ≥ 900 y B ≥ 1 800
(con T_entry2, ventanas completas y en SOL), ampliación por días UTC con fecha
tope (sustituye «no se amplía el bloque» del pre-registro y de la adenda 1),
tratamiento de recuperación y huecos, sensibilidad por retraso de captura (2 h)
y el inventario de 41 cortes exploratorios (1 961 con las reglas de salida).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
```

sha256 de `docs/adenda_3_2026-10-27.md` en el commit 6: `e6364813b582d4d384bcbbfe2e2bdff6a7323a856a9663d3618be81c08787a8b` (segunda versión, tras la aprobación del dueño; la primera, sin aprobar, era `663c9077…0ac8`).

### Fuera de los commits

- `src/check.rs` ya está en `14f0fa3` salvo el hunk de test (commit 1).
- `resultados/check_sin_tentry2_2026-10-27.txt` ya está en `14f0fa3`.

## 3. Escaneo de secretos (solo informa; 2026-10-27)

| Qué | Árbol versionado | Sin seguimiento (no ignorado) | Historial git (todas las ramas) |
|---|---|---|---|
| Clave real de Alchemy (`ALCHEMY_API_KEY` de `.env`, 26 caracteres; se buscó el valor literal sin imprimirlo) | 0 | 0 | 0 |
| URLs con clave (`alchemy.com/v2/<…>`, `api-key=<…>`) | 0 reales. Solo claves de prueba inventadas en los tests de `src/rpc.rs` (`alch_p-x2Ab_9`, `k`, `abc-123`) | 0 | 0 reales (las mismas de prueba) |
| `.env` | ignorado, no versionado | — | nunca |
| `run/` (binario, `collect.log`, `collect.out`, scripts de servicio) | ignorado, no versionado; 0 apariciones de la clave en `run/` | — | nunca |
| `*.db` (`marxol.db`, `marxol_vivo.db`, `marxol_fase3.db`, `madurez.db`) | ignorados, no versionados; 0 apariciones de la clave | — | nunca |
| CSV / CSV.gz (`marxol_export/`) | ignorado, no versionado | — | nunca |

- `.gitignore` cubre `.env`, `run/`, `*.db`, `*.db-wal`, `*.db-shm`, `*.db-journal`, `madurez_cache/` y `marxol_export/`.
- Ninguno de los ficheros que se proponen arriba contiene la clave: se comprobó todo el árbol sin seguimiento.
- No hay stashes. El remoto es `git@github.com:idrode/MarXol.git` (SSH, sin credenciales en la URL).
- Las URLs RPC que aparecen en el árbol no llevan clave: el RPC público, la base de Helius y la de Alchemy en documentación y tests.
- `run/collect-service.sh` lee `.env` en tiempo de ejecución. Está ignorado.

**Escaneo de datos personales** (nombres de máquina, usuarios, rutas del home, IPs, Tailscale, correos) de los ficheros de los commits: hecho el 2026-10-27; el resultado se dio en la conversación y no se copia aquí para no versionarlo.

**Antes del push**: repetir el escaneo sobre `git log origin/main..main -p`.
