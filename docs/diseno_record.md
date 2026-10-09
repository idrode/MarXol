# Diseño de `marxol record` (Etapa 2, solo documento)

Estado: **diseño, sin código.** Redactado el 2026-10-09, después de la Etapa 0 (`resultados/etapa0_latencia.txt`) y de la prueba corta de streaming (`resultados/benchmark_streaming.txt`). Nada de esto cambia definiciones ni veredictos congelados (V1, V2, V3, D2).

## 0. Objetivo y restricciones

`marxol record` graba en vivo, por cada token nuevo de pump.fun:
- su creación;
- los trades de sus primeros **60 s**, con slot, `timestamp` del evento (blockTime) y hora local de recepción;
- instantáneas de precio posteriores, para poder medir retornos.

El objetivo es que el próximo pre-registro se evalúe con los datos que habría tenido un observador real, en vez de reconstruirlos días después con `getSignaturesForAddress` + `getTransaction`. Así se evita, por ejemplo, el coste de descargar tarde que se vio en V2.

Restricciones heredadas:
- Solo lectura sobre la cadena: ni wallets ni firmas.
- Las claves salen de `.env`: nunca se imprimen ni se registran.
- Base separada (`marxol.record.db`, no versionada). `marxol.db` no se toca.
- Todo el consumo va a `rpc_usage`.
- Una tarea de fondo por trabajo, comunicación solo por canal y un único escritor de la base (principio 5).

Por qué 60 s (Etapa 0, sin mayhem, un token por creator):
- T_entry2 cae a ≤ 60 s de la creación en el 79–88 % de los tokens de V1, V2 y V3.
- Ningún pump-y-caída de V2 ni de V3 tiene T_entry2 > 60 s.
- Los tokens con T_entry2 > 60 s quedan como "sin T_entry2 en 60 s" y se cuentan aparte.
- Si un pre-registro necesita la ventana completa de 5 min de las validaciones, hay que ampliar la ventana (sección 6).

## 1. Fuente de datos y coste (lo que condiciona todo)

### Hechos medidos el 2026-10-09
- `logsSubscribe` funciona en el plan gratuito de Alchemy.
- Con `mentions = programa pump` llegan 117 msg/s, unos 246 kB/s (≈ 21 GB/día). El 65 % son tx fallidas, que no se pueden filtrar en origen.
- Latencia respecto a `blockTime`: mediana 1.2 s, p99 2.1 s.
- 0 firmas perdidas en 51 slots.
- **Los eventos van también en los logs.** Pump emite cada evento por `emit_cpi` **y** como línea `Program data:` en los logs. En una tx de venta comprobada, el payload del log era idéntico byte a byte al de la inner instruction (`TradeEvent`, 383 bytes). Así que el grabador **puede decodificar desde `logsNotification` sin pedir `getTransaction`**. Está comprobado en **1 tx**: hay que verificarlo en volumen antes de depender de ello (ver "Logs truncados" más abajo).
- **La facturación no está resuelta.** Alchemy factura los WebSocket por byte, y la página de costes da dos tarifas: 0.0002 CU/byte (sección Solana) y 0.04 CU/byte (sección genérica).
  - Con la tarifa barata, el stream del programa entero cuesta ≈ 4.2 M CU/día (≈ 127 M/mes, frente a los 30 M del free tier).
  - Con la genérica es inviable.
  - **Esta decisión está bloqueada hasta ver el panel de Alchemy.**

### Opciones de suscripción

| Opción | Qué llega | Volumen estimado | Comentario |
|---|---|---|---|
| A. `mentions = programa pump` | Todo el tráfico de pump (trades de todos los tokens, también los viejos) | ≈ 21 GB/día (medido, 15 s) | Una sola conexión y lo más simple. Se descarta en local lo que no es de los primeros 60 s. No cabe en el free tier con ninguna de las dos tarifas |
| B. `mentions = mint-authority` (`TSLvdd1p…`) + una suscripción `mentions = bonding curve` por token durante 60 s | Solo creaciones, y luego solo el tráfico de cada curva joven | Creaciones ≈ 0.4–0.8/s. Trades: ≈ 31 por token en 60 s (media de los 973 tokens con ventana en `marxol.db`; mediana 9), más las tx fallidas sobre esa curva (desconocidas). Orden de magnitud: **varios GB/día**, no 21 | ~25–50 suscripciones concurrentes. **No verificado** cuántas admite el plan gratuito de Alchemy; Helius free tier admite 2 conexiones, no se sabe cuántas suscripciones por conexión. Una suscripción nueva tarda en estar activa: los trades del primer segundo de cada curva pueden perderse, y se recuperan del bloque de creación con `getTransaction` (sección 3) |
| C. Yellowstone gRPC | Filtro por cuentas, sin tx fallidas (`failed: false`) | Bastante menos que A | Solo PAYG/Enterprise ($75/TB prorrateado). Fuera del plan gratuito y sin contratar |

**Recomendación de diseño**: la opción **B**, con A como modo de diagnóstico de corta duración. Antes de construir, una prueba corta de B (10 min) que mida el volumen real por curva, la latencia de alta de la suscripción y el límite de suscripciones concurrentes. El consumo de esa prueba se registra igual que en la Etapa 1.

### Logs truncados
Solana trunca los logs de una tx a 10 KB ("Log truncated"). Una tx de pump con muchas instrucciones (bundles, routers) puede perder el `Program data:`. El grabador:
1. detecta el truncado (la línea literal "Log truncated", o un `invoke` de pump sin su `Program data:`);
2. encola esa firma para `getTransaction` (40 CU), que decodifica desde la inner instruction con el código actual (`src/tx.rs`);
3. cuenta los truncados en `rec_health`, porque su frecuencia decide el coste extra.

### Precios después de los 60 s
Los retornos a +5, +30 y +60 min (los horizontes congelados) **no salen de la grabación de 60 s**. En vez de mantener la suscripción abierta una hora, se leen las cuentas `BondingCurve` con `getMultipleAccounts` (20 CU por llamada de hasta 100 cuentas) en los instantes t0 + {5 min, 30 min, 60 min, 65 min}. Son reservas exactas en ese instante, y precio = `virtual_quote / virtual_token`, igual que en H1b.

Coste: con 50 000 tokens al día y 4 instantes, son 2 000 llamadas por instante y día → **≈ 160 k CU/día**.

Diferencia con la definición congelada: el retorno congelado usa el **último trade ≤ salida**. Una instantánea da el estado de la curva en ese momento, que es lo mismo si no ha habido trades posteriores y si la curva no se ha completado. Si `complete = true`, el precio de la curva deja de valer (sigue en PumpSwap): se marca n/e, como ahora. Si un pre-registro quiere el retorno congelado exacto, hay que bajar los trades de la hora con el método actual (`marxol prices`) solo para los tokens de su población.

## 2. Esquema de la base (`marxol.record.db`)

Las tres señales de identidad de creador van por separado, nunca colapsadas en un campo mutable (CLAUDE.md 5.2). Los nombres siguen `src/store.rs` donde existe equivalente.

```
rec_runs        (run_id PK, started_at, stopped_at, provider, mode 'A'|'B', commitment, git_rev, params_json)

rec_tokens      (mint PK, run_id, signature, slot, block_time,          -- CreateEvent.timestamp
                 recv_ns,                                                -- hora local de recepción (ns, reloj NTP)
                 creator, user, quote_mint, is_mayhem_mode, is_cashback_enabled,
                 virtual_token_reserves, virtual_quote_reserves, real_token_reserves,
                 token_total_supply, creator_fee_bps, name, symbol, uri,
                 window_secs,                                            -- 60
                 window_status 'open'|'complete'|'gap'|'backfilled'|'lost')

rec_trades      (signature, event_idx, mint, slot, block_time, recv_ns,
                 source 'stream'|'backfill',
                 user, is_buy, token_amount, quote_amount, quote_mint,
                 virtual_token_reserves, virtual_quote_reserves,         -- tras el trade (encadenado, sección 11 de CLAUDE.md)
                 real_token_reserves, real_quote_reserves,
                 fee_basis_points, creator_fee_basis_points, creator,    -- creator embebido
                 ix_name, mayhem_mode,
                 PRIMARY KEY (signature, event_idx))
                 -- solo trades con block_time < rec_tokens.block_time + window_secs

rec_failed      (signature PK, mint NULL, slot, recv_ns)                -- tx fallidas sobre curvas jóvenes (opción B): solo recuento y slot, sin payload

rec_creator_changes (signature, event_idx, mint, kind 'set_creator'|'migrate_to_sharing',
                 old_creator, new_creator, sharing_config, slot, block_time, recv_ns,
                 PRIMARY KEY (signature, event_idx))
rec_completions (mint PK, signature, slot, block_time, recv_ns, kind 'complete'|'migrated', pool)

rec_snapshots   (mint, offset_secs, requested_ns, slot,                  -- slot del contexto de getMultipleAccounts
                 virtual_token_reserves, virtual_quote_reserves, real_quote_reserves, complete,
                 PRIMARY KEY (mint, offset_secs))

rec_gmgn        (mint, offset_secs, requested_ns, received_ns, http_status, error,
                 raw_json BLOB,                                          -- respuesta íntegra (comprimida), para re-extraer
                 -- campos extraídos (ver sección 4); NULL si no vienen
                 creator_funding_source, creator_funding_amount, creator_funding_age_secs,
                 creator_status, creator_token_count, creator_open_tokens,
                 bundler_count, bundler_pct, sniper_count, sniper_pct,
                 rug_ratio, top10_holder_pct, dev_holding_pct,
                 PRIMARY KEY (mint, offset_secs))

rec_sessions    (run_id, session, opened_at, closed_at, close_reason,
                 first_slot, last_slot, bytes, msgs, subscriptions_peak)
rec_gaps        (run_id, gap_id, from_slot, to_slot, from_time, to_time, reason,
                 status 'pending'|'backfilled'|'unrecoverable', backfill_sigs, backfill_cu)
rec_health      (minute PK, msgs, bytes, failed, truncated_logs, decode_errors, unknown_events,
                 lat_p50, lat_p99, open_windows, queue_depth)
rpc_usage       (mismo esquema que marxol.db, más una columna ws_bytes)
```

Notas:
- `recv_ns` se toma al leer el frame, antes de decodificar. La latencia del aviso es `recv_ns − block_time`. `block_time` sale del `timestamp` del propio evento (el `Clock` del bloque), así que no hace falta `getBlockTime`. La Etapa 1 midió la latencia con `getBlockTime`; hay que comprobar en la primera hora que ambos coinciden.
- El orden de ejecución dentro de un slot se reconstruye encadenando reservas, igual que ahora, empezando por el `CreateEvent`. `rec_tokens.window_status = 'complete'` exige que el encadenado no tenga huecos hasta el último trade de la ventana.
- Para que el análisis reutilice `entry2`/`h1b` sin cambios, un comando aparte (`marxol record-export`) podría copiar a una base con el esquema de `marxol.db` (`creations`, `early_trades`, `trade_windows`, `price_points`). Queda fuera de este documento.

## 3. Caídas del stream y huecos

Hay tres tipos de pérdida, que se tratan por separado:

1. **Caída de conexión.**
   - Se reconecta con espera creciente de 1, 2, 4… hasta 60 s.
   - Se guarda `rec_sessions` con el último slot recibido antes de la caída y el primero después.
   - El intervalo `(último, primero)` se escribe en `rec_gaps` como `pending`.
   - Con la opción B, al reconectar se vuelven a dar de alta las suscripciones de las curvas con ventana abierta.
2. **Suscripción tardía de una curva** (solo B). Los trades entre la creación y el alta de su suscripción se recuperan leyendo el bloque de creación y los siguientes. El método es el de `windows`: `getSignaturesForAddress` sobre la curva acotado a `[slot de creación, slot de alta]`, más `getTransaction` de las firmas exitosas que no se hayan visto. Es pocas firmas por token, pero se mide.
3. **Huecos detectados por el encadenado de reservas.** Si el `virtual_token_reserves` antes de un trade no coincide con el después del anterior, falta al menos un trade. El token pasa a `gap` y se encola un backfill de su curva.

**Backfill**:
- Es un trabajador aparte y con presupuesto propio (`--max-backfill-cu`). Usa el método actual (`getSignaturesForAddress` + `getTransaction`, 40 CU cada uno).
- Solo cubre los tokens afectados, no el programa entero.
- Al terminar, `window_status = 'backfilled'` y `source = 'backfill'` en los trades recuperados.
- Si el presupuesto se agota o los datos ya no se pueden pedir, el token pasa a `unrecoverable`/`lost`. Nunca se rellena por interpolación.

**Regla para el análisis** (a fijar en cada pre-registro, no aquí): qué estados entran en la población. Propuesta para discutir: `complete` y `backfilled` entran; `gap` y `lost` se cuentan aparte, como los "sin datos" de la adenda 3 de V3. Hay que reportar siempre cuántos tokens quedan en cada estado y si los perdidos se concentran en horas de más carga (sesgo de selección).

**Huecos de cobertura que no se pueden recuperar**: si el grabador estuvo caído cuando se creó un token, `rec_tokens` no lo tiene. Esos tokens se detectan comparando con `getSignaturesForAddress` de `mint-authority` sobre el rango del hueco (barato: solo listas de firmas). Se registran en `rec_gaps` aunque no se graben.

## 4. Proveedor GMGN (opcional, solo lectura)

- **Configuración**: `GMGN_API_KEY` en `.env` (no existe todavía: no se pide). Opcionalmente `GMGN_BASE_URL` y `GMGN_MAX_RPS`. Sin clave, el proveedor está desactivado y `record` funciona igual.
- **Qué hace**: para cada token grabado, una petición a +10 s, +30 s y +60 s desde `block_time` de la creación. Guarda la respuesta íntegra (`raw_json`) y los campos extraídos.
- **Endpoints**: **no verificados**. Se fijarán contra la documentación oficial de GMGN cuando haya clave. No se usa scraping de la web, solo la API documentada, respetando sus términos.
- **Control de velocidad**:
  - Un token bucket con `GMGN_MAX_RPS` (por defecto conservador, p. ej. 1 req/s) y una cola con prioridad por instante objetivo.
  - Si la cola acumula más de N s de retraso respecto al instante objetivo, la instantánea se marca `late` (con su `requested_ns` real) o se descarta. Una instantánea a +30 s pedida a +45 s no es una instantánea a +30 s.
  - Con 0.4–0.8 tokens/s y 3 instantáneas por token hacen falta 1.2–2.4 req/s. Si el límite de GMGN es menor, se **muestrea** una fracción aleatoria fija de tokens (semilla registrada en `rec_runs.params_json`), nunca "los que parecen interesantes".
  - Los 429 se reintentan con espera, cuentan en el presupuesto y van a `rpc_usage` con `provider = 'gmgn'`.
- **Campos a guardar** (nombres internos; el mapeo a los de GMGN se fija al tener la doc):
  - financiación del creador: origen de los fondos (CEX, wallet, bridge), importe y antigüedad de la financiación;
  - estado del creador: tokens creados, tokens abiertos o vendidos, % que conserva;
  - bundlers: número y % del supply;
  - snipers: número y % del supply;
  - `rug_ratio`;
  - top 10 holders, %;
  - holding del dev, %.
- **Advertencia para el análisis**: los campos de GMGN son **derivados por un tercero con criterios no públicos**. Pueden cambiar sin aviso y no se pueden verificar on-chain con el principio 1. Sirven como covariables o para comparar con H3 (financiación), no como verdad de referencia. La instantánea a +60 s es posterior a la mayoría de T_entry2 (mediana 13–16 s). Usarla en una señal "antes de entrar" sería fuga de información, salvo que se use solo la instantánea anterior a T_entry2.

## 5. Almacenamiento y rotación

Estimación (orden de magnitud, a confirmar en la prueba de B):
- Tokens: 35 000–70 000 al día (0.4–0.8/s; V3 tenía 0.4/s y V2 0.8/s).
- Trades en 60 s: ≈ 31 por token de media → **1–2 M filas al día** en `rec_trades`. Con ~250 bytes por fila más índices, **≈ 0.3–0.6 GB/día**.
- `rec_snapshots`: 4 filas por token, despreciable. `rec_failed`: del orden de 2× los trades, unos 60 bytes por fila.
- `rec_gmgn.raw_json` comprimido: depende de la respuesta. Se estima en 1–3 KB por instantánea → 0.1–0.6 GB/día si se graban todos los tokens.
- **No se guardan los logs crudos.** Solo los eventos decodificados, más la firma para poder volver a pedir la tx.

Rotación:
- **Una base por día UTC** (`marxol.record.YYYYMMDD.db`), cerrada al cambiar de día. El grabador abre la nueva sin cortar el stream. Los tokens con ventana abierta en el cambio de día se escriben en la base de su `block_time` de creación.
- Las bases cerradas se comprimen (zstd) a los 7 días. Se conservan **mientras haya un pre-registro que las use**. El tope por defecto es de 60 días o un tamaño máximo configurable (`--max-disk-gb`); al pasarlo, se avisa y **se para**, no se borra en silencio.
- `rec_health` y `rpc_usage` también se copian a un resumen diario pequeño, que no se rota.
- Escritura en lotes (cada 1 s o 500 filas) en modo WAL, con un único escritor. El hilo del stream no escribe en la base: pasa los eventos por un canal (principio 5).

## 6. Qué pre-registros permitirían estos datos y cuáles no

### Posibles

- **Validación 4 de D2 "en tiempo real"**. Es la misma hipótesis congelada con datos grabados en vivo, sin cambiar definiciones. Además permite medir:
  - la diferencia entre el retorno desde T_entry2 y el retorno desde el **momento real del aviso** (`recv_ns` de T_entry2 más el tiempo de cálculo);
  - el **margen**: tokens cuyo aviso llega después de X s.

  Debe pre-registrarse aparte como resultado secundario. El primario sigue siendo la definición congelada. Exige las instantáneas de +30 min y la regla de equivalencia con el retorno congelado (sección 1).
- **H11** (lanzamiento previo del creator en las N horas anteriores), **H12** (persistencia de dev-sell) y **H13** (un token por creator). Con días seguidos de creaciones grabadas sí hay masa de repetidores. En 8 min había 24; la exploración estimaba que hacen falta horas o días.
- **H9 / H10 y "resultado desde el precio de entrada"**, con las instantáneas de +5, +30 y +60 min.
- **Perfil horario y por día de la semana** de D2 y de la tasa de pump-y-caída, que hasta ahora son tres franjas de 8 min.
- **Covariables de GMGN** (si hay clave) como hipótesis nuevas: por ejemplo, que `bundler_pct` a +10 s se asocie con pump-y-caída. Solo con la instantánea anterior a T_entry2 para cualquier señal de entrada.

### No posibles con este diseño

- **Cualquier indicador que use trades posteriores a 60 s**: la ventana de 5 min de H1/H1c/T_entry, o T_entry2 > 60 s. Esos tokens quedan "sin T_entry2 en 60 s". Ampliar la ventana a 300 s es un cambio de parámetro, pero multiplica el volumen.
- **Pump-y-caída con la definición congelada** (pico y cierre con todos los trades de la hora). Con instantáneas a 5, 30 y 60 min se ve el cierre, pero no el pico entre instantáneas. Se necesita o bien la descarga de la hora con `marxol prices` para la población del pre-registro, o bien una definición nueva basada en instantáneas, pre-registrada como tal.
- **Retorno congelado exacto** (último trade ≤ salida) sin descarga adicional. Las instantáneas lo aproximan, pero no son la misma definición.
- **Post-graduación**: precio en PumpSwap. No se lee en ningún diseño actual.
- **Financiación del creador on-chain (H3) verificable**: GMGN la da como dato de terceros. Para H3 hay que seguir la financiación en la cadena, un trabajo aparte.
- **Mayhem**: se graban, pero su precio cambia fuera de los trades. Siguen siendo una población aparte, sin retornos fiables.

## 7. Pendiente de decidir (antes de escribir código)

1. **Tarifa real de los WebSocket de Solana en Alchemy** (0.0002 o 0.04 CU/byte): mirar el panel con la prueba del 2026-10-09 a las 08:15 UTC (3.56 MB). Decide si hay streaming viable en el free tier.
2. **Opción A o B** (o un proveedor distinto: Helius free tier, o Yellowstone de pago), según el punto 1 y una prueba corta de B.
3. **Ventana de 60 s o 300 s**: 300 s permite reproducir T_entry2 en el 100 % de los casos y H1, pero cuesta unas 3–5 veces más en volumen.
4. **Regla de inclusión** de los tokens `gap`/`backfilled`/`lost` en las poblaciones de los pre-registros.
5. **GMGN**: si se pide clave, qué plan, y si se muestrea.
6. **Disco disponible** y periodo de retención.
