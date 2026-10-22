# Recolección continua (`marxol collect`) y libro de CU

Fase 2 (2026-10-22). Base aparte: `marxol_vivo.db` (`--vivo-db` / `MARXOL_VIVO_DB`). `marxol.db` queda congelada con V1–V3 y `collect` nunca la abre.

Los datos de `marxol_vivo.db` son **fuera de muestra**. D2 y las métricas actuales quedaron congeladas antes de recolectar, en docs/congelado_vivo_2026-10-22.md. No se ajusta nada contra estos datos sin avisar al dueño.

## Libro de CU diario compartido

- Tabla `cu_ledger_day` (día UTC × bolsa) y `cu_ledger_runs` (una fila por proceso: comando, bolsa, pid, proveedor, peticiones y unidades). Las dos están en `marxol_vivo.db`.
- Cada petición se carga **antes** de hacerla, con `BEGIN IMMEDIATE`, así que dos procesos a la vez no pueden pasarse del tope.
- Hay dos bolsas por día UTC:
  - `collect`: `--collect-cu` / `MARXOL_COLLECT_CU`, por defecto 700 000.
  - `manual`: el resto hasta `--daily-cu` / `MARXOL_DAILY_CU`, por defecto 1 000 000, es decir 300 000.
- Un comando manual no puede dejar a `collect` sin cuota, y `collect` no puede comerse la de los manuales. La suma nunca pasa del total. El contador se reinicia a las 00:00 UTC.
- Cargan en el libro todos los comandos que usan la red: `verify`, `global`, `curve`, `tx`, `scan`, `index`, `index-tramo`, `windows`, `prices`, `check` (en línea) y `collect`. Los comandos sin red no abren el libro: `h1`, `h1b`, `entry`, `entry2`, `operator`, `stats`, `check --offline` y `collect-status`.
- Costes cargados: los de `rpc::Provider`.
  - Alchemy: `getTransaction` y `getSignaturesForAddress` a 40 CU, `getAccountInfo` a 10 y `getMultipleAccounts` a 20.
  - `getMultipleAccounts` no figura en la tabla revisada, así que se cargan 20 CU por el lado conservador.
  - El CU real solo lo da el panel de Alchemy.
- **`payer-history` no corre en paralelo con `collect`** (regla del dueño, 2026-10-22): se niega a arrancar si el libro tiene un `collect` vivo (fila sin `ended_at` y proceso en `/proc`). Para usarlo, para `collect` (`kill $(cat run/collect.pid)`) y vuelve a lanzarlo después: el punto de reanudación recupera el listado.
- **El libro reparte CU, no ritmo.** Con `collect` corriendo a 12 req/s, un comando manual al mismo ritmo pasa de los 500 CU/s del free tier y provoca 429. Observado el 2026-10-22: `payer-history` a 12 req/s tuvo 429, `collect` ninguno. Mientras `collect` corre, los comandos manuales largos van con `--max-rps 4` (12 + 4 = 16 req/s, 640 CU/s de pico; en la práctica `collect` está casi siempre en espera).
- Al agotarse una bolsa, las descargas paran limpias (`ledger::Exhausted`): el token a medias queda pendiente y no se cuenta como intento fallido.
- La base solo guarda el nombre del proveedor, nunca la URL del RPC ni la clave. Hay un test que lo comprueba en los bytes del archivo y del WAL.

## Ciclo de `collect`

Cada `--poll-secs` (por defecto 300 s):

1. **Listado**:
   - Pide las firmas nuevas de la PDA mint-authority (`TSLvdd1p…`) desde el punto de reanudación, 1 000 por petición.
   - Las guarda todas en `vivo_create_sigs`, con agregados por hora UTC en `vivo_hourly`.
   - El punto de reanudación es la firma y el slot más nuevos, en `vivo_meta` (`cursor_signature`, `cursor_slot`). Se actualiza en la misma transacción que el listado.
   - Si hacen falta más de `--max-backfill-pages` páginas para alcanzarlo (tras un corte largo), el hueco queda en `vivo_gaps`.
2. **Muestreo**:
   - Firma sin error con `fnv1a64(firma) % 1e6 < --sample-ppm`. Las firmas son aleatorias, así que es uniforme, reproducible y no depende de la hora: cubre las 24 h sin franjas.
   - Cada token guarda `captured_at`, `hour_utc` (de blockTime), `sample_ppm`, la regla y la versión del congelado (`vivo_tokens`).
3. **Seguimiento**:
   - Estado de la bonding curve a +1 h, +24 h, +7 d y +60 d (`vivo_outcomes`), en lotes de 100 con `getMultipleAccounts`.
   - Guarda la cuenta decodificada y cuándo se comprobó de verdad, porque un corte retrasa la comprobación.
4. **Creación**:
   - `getTransaction` de la firma muestreada, guardada con `Store::ingest` como en `index`, y precio inicial.
   - Se da por descargada para la ventana y el precio, así no se pide dos veces.
   - Sin `CreateEvent`: `sin_create`. Errores de decodificación (posible IDL desactualizado): no se da por buena y se reintenta hasta 3 veces.
5. **Ventana temprana y precio**:
   - Las mismas pasadas que `windows` y `prices` (`ingest::windows_pass` / `prices_pass`).
   - Llevan un tope de `--max-token-sigs` tx por token y ventana, 400 por defecto. Un token con más queda incompleto, con la nota en `trade_windows` o `price_windows`, y se cuenta.
6. **Agregación**:
   - Cuando un token termina, se calcula con el código congelado (`report::v3_rows`, las mismas filas que la validación 3) y se guarda en `vivo_results`.
   - Un test comprueba que coincide con V3 en sus 196 tokens.
   - Los resultados **no se resumen ni se leen** hasta el pre-registro de la fase 5.

Cada ciclo deja una fila en `vivo_cycles`, con las CU por etapa y el trabajo hecho. `marxol collect-status` las resume, proyecta el consumo diario y muestra la cobertura por hora.

Un error de red no tumba el proceso: se registra en el ciclo y se sigue en el siguiente. Todo es idempotente, así que tras un corte el arranque sigue desde el cursor y lo pendiente.

## SQLite para 24/7

- **WAL**, `synchronous = NORMAL`, `auto_vacuum = INCREMENTAL` (se fija al crear la base) y `busy_timeout` de 30 s.
- `ledger::open_vivo_conn` se niega a abrir como base de recolección una base que no lo sea (sin `vivo_meta`). Así `marxol.db` no puede pasar a WAL por error.
- Inserciones por lotes en transacciones: el listado completo de un ciclo, los seguimientos de cada lote de 100 y la ingesta de cada tx.
- **Retención** (`--retention-days`, por defecto 30; 0 la desactiva; mínimo 2), cada `--maintenance-every` ciclos:
  1. Exporta a `--export-dir` (por defecto `marxol_export/`, fuera de git) las filas de crudos anteriores al corte que aún no se hayan exportado.
     - Crudos: `early_trades`, `price_points` y `vivo_create_sigs`.
     - Formato: CSV con gzip, con cabecera. Se escribe a `.tmp`, se hace fsync y se renombra.
  2. **Verifica la copia** leyéndola del disco y descomprimiéndola entera: el número de filas tiene que cuadrar. Solo entonces la registra en `vivo_exports` (ruta, filas, bytes, sha256) y avanza la marca `exportado_hasta:<tabla>`.
  3. Borra en lotes de `--delete-batch` filas, cada lote en su transacción, solo lo cubierto por una exportación verificada **y** ya agregado:
     - el token tiene fila en `vivo_results`;
     - las firmas ya están contadas en `vivo_hourly`.

     Borra también las filas de `fetched_sigs` y `seen_signatures` de esas tx.
  4. `PRAGMA incremental_vacuum` y `PRAGMA wal_checkpoint(TRUNCATE)`.
- **No se borran nunca**: `vivo_meta` (punto de reanudación), `vivo_tokens`, `vivo_results`, `vivo_outcomes`, `vivo_creators`, `wallet_links` (clústeres y fábricas de la fase 3), `vivo_hourly`, `vivo_cycles`, `vivo_gaps`, `vivo_exports`, el libro, ni ninguna tabla de `marxol.db`.
- **Parquet**: no está. Haría falta la pila de Arrow, pesada para compilar en el teléfono. CSV con gzip sirve para cualquier herramienta y se convierte fuera si hace falta.
- "La copia llegó" se verifica en la ruta de `--export-dir`. Si esa carpeta se sincroniza a otra máquina, el sha256 de `vivo_exports` permite comprobar allí la copia.

## Termux (RedMagic u otro Android)

1. `pkg install rust git sqlite` y `cargo build --release` en el repo. SQLite va incluido (`rusqlite` bundled) y TLS es rustls, así que no hace falta OpenSSL.
2. Pon la URL del RPC en un archivo de entorno fuera del repo, con permisos 600 (por ejemplo `~/.marxol.env` con `MARXOL_RPC_URL=...`). Nunca va a la base.
3. `termux-wake-lock` y excluir Termux de la optimización de batería de Android. Si no, el sistema mata el proceso.
4. Lanzarlo bajo un supervisor (`termux-services`) o en un bucle que lo reinicie:
   ```
   set -a; . ~/.marxol.env; set +a
   while true; do ./target/release/marxol collect --sample-ppm N >> collect.log 2>&1; sleep 30; done
   ```
5. `marxol collect-status` en cualquier momento (sin red) para ver el consumo, la proyección y la cobertura.
6. Los comandos manuales (`check`, `tx`, …) se pueden lanzar a la vez: comparten el libro con `--vivo-db` apuntando a la misma base.
