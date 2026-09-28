# marXol

Proyecto hermano de [marXi](https://github.com/idrode/MarXi), mismo autor (Roi, GitHub `idrode`), misma filosofía, para **Solana** en vez de Robinhood Chain (EVM). Ruta local: `/home/ander/marXol`.

Este documento es la fuente de verdad del proyecto. Se actualiza en la misma sesión en que se confirma un hallazgo contra la chain real — nunca se deja un hallazgo verificado solo en una conversación de chat.

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

## 3. Lo que NO se traslada de marXi (investigado de cero para Solana)

- **Cliente de chain completo**: nada de `alloy`/EVM. Aquí es `solana-client` / `solana-sdk` (Rust).
- **Modelo de datos**: no hay eventos/logs como en EVM. Solana tiene "program logs" e instrucciones dentro de transacciones. El indexado se basa en decodificar eventos Anchor (ver sección 5), no en un `eth_getLogs` equivalente (no existe).
- **Launchpad de referencia**: pump.fun en vez de Pons V2 (ver sección 4).
- **Definición de "creador"**: en Solana/pump.fun es un campo de negocio explícito en el propio programa, no se infiere del deployer/signer de la transacción (ver sección 5.2 — esto es el hallazgo más importante del proyecto hasta ahora).
- **Proveedor RPC**: Alchemy sigue siendo el proveedor base (ya se usaba en marXi), pero con API/RPC de Solana, no EVM — y con una dinámica de límites completamente distinta por el volumen de Solana (ver sección 6).

---

## 4. Launchpad de referencia: pump.fun

**Decisión: pump.fun**, no LetsBonk.fun ni otros. Cuota de mercado real 2026 es más reñida de lo esperado (fuentes dan entre 45% y 74% de share para pump.fun según el periodo, con LetsBonk.fun como rival serio que en algunos tramos lo superó), pero pump.fun es el único con documentación técnica pública, mantenida activamente y con SDKs oficiales — es el que permite construir con confianza contra specs verificables, que es el criterio que más pesa para este proyecto.

El diseño del indexador debe dejar la puerta abierta a añadir LetsBonk.fun después (mismo patrón estructural: bonding curve → graduación a AMM) sin rehacer el modelo de datos.

### Program ID

```
6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P
```

Desplegado en Mainnet y Devnet. Fuente: `idl/pump.json` del repo oficial `pump-fun/pump-public-docs` (descargado y parseado directamente).

**Verificado en mainnet (2026-09-28, `marxol verify`)**: cuenta ejecutable, owner `BPFLoaderUpgradeab1e11111111111111111111111`. La PDA `Global` derivada (`["global"]`) coincide con `4wTV1Ymi…`. Ver sección 11.

### Ciclo de vida del programa (confirmado contra el IDL)

1. **Initialization**: se crea una única cuenta `Global` (PDA `4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf`) vía `initialize`, configurable por el primer caller.
2. **Coin Creation**: `create` / `create_v2` lanza un SPL token nuevo con mecánica de bonding curve. El parámetro `creator` puede diferir del signer de la transacción (ver sección 5.2).
3. **Trading Phase**: `buy`/`sell` (y variantes `_v2`, `_exact_quote_in`) ejecutan trades sobre la bonding curve, con fees en cada transacción.
4. **Completion Trigger**: cuando `real_token_reserves` llega a cero durante un `buy`, la bonding curve se marca `complete == true` (evento `CompleteEvent`).
5. **Migration**: una vez completada, cualquiera puede llamar `migrate` (permissionless) para migrar la liquidez a PumpSwap (AMM propio del ecosistema pump.fun, no Raydium) y quemar los LP tokens resultantes (evento `CompletePumpAmmMigrationEvent`).

### Parámetros económicos: se leen de la cuenta `Global`, nunca se hardcodean

El umbral de graduación, las comisiones de trading, la comisión de migración, etc. **no son constantes fijas** — se leen de la cuenta `Global` on-chain en tiempo real. Campos relevantes del struct `Global` (del IDL):

```
initial_virtual_token_reserves, initial_virtual_sol_reserves, initial_real_token_reserves,
token_total_supply, fee_basis_points, creator_fee_basis_points, pool_migration_fee,
enable_migrate, create_v2_enabled, mayhem_mode_enabled, creator_fee_configurable,
max_configurable_creator_fee_bps, whitelisted_quote_mints, is_holder_reward_enabled
```

No dar por bueno ningún número de "market cap de graduación" citado en blogs (se ha visto citado ~$69K en varias fuentes de 2025).

**Umbral de graduación verificado (2026-09-28)** — leído del `Global` en vivo y contrastado con eventos reales:
- `initial_virtual_token_reserves = 1_073_000_000_000_000`, `initial_virtual_sol_reserves = 30_000_000_000` (30 SOL), `initial_real_token_reserves = 793_100_000_000_000`, `token_total_supply = 1_000_000_000_000_000` (6 decimales).
- Curva de producto constante: se completa al venderse `initial_real_token_reserves` → **85.0054 SOL reales** en la curva; mcap al completar ≈ **410.9 SOL** (inicial ≈ 27.96 SOL). El umbral es en SOL, no en USD: el "$69K" de blogs solo era la conversión a precio de SOL de entonces.
- Contraste independiente: `CompletePumpAmmMigrationEvent.sol_amount` observado en mainnet = 84.99036 SOL = 85.00536 − `pool_migration_fee` (15_000_001 lamports). Cuadra.
- Resto de parámetros vivos: `fee_basis_points = 95`, `creator_fee_basis_points = 5`, `max_configurable_creator_fee_bps = 300`, `buyback_basis_points = 5000`, `create_v2_enabled`, `mayhem_mode_enabled`, `is_cashback_enabled`, `is_holder_reward_enabled`, `creator_fee_configurable` todos `true`. Estos pueden cambiar (evento `SetParamsEvent`): leerlos siempre con `marxol global`.
- **Curvas con quote USDC**: `initial_virtual_quote_reserves = 4_292_000_000` (4 292 USDC). Graduación USDC observada: `sol_amount = 12_161_433_370` → el campo `sol_amount` de los eventos **va en unidades del `quote_mint`**, no siempre lamports. Nunca interpretarlo como SOL sin mirar `quote_mint`.

### Precio efectivo en PumpSwap (post-graduación)

El `Pool` account de PumpSwap tiene un campo `virtual_quote_reserves` (actualmente siempre `0` según el propio equipo, pero puede dejar de serlo). La reserva efectiva correcta para pricing es:

```
effective_quote_reserves = pool_quote_token_account.amount + Pool::virtual_quote_reserves
```

Usar siempre `effective_quote_reserves`, nunca el balance crudo del vault, para que el indexador no quede desactualizado si ese campo deja de ser 0.

---

## 5. Modelo de datos: identidad de "creador" (hallazgo central del proyecto)

### 5.1 `creator` es un campo explícito, separado del signer

En `create_v2`, el creador se pasa como **argumento de datos** (`creator: Pubkey`, tipo `Pubkey`, "Must not be `Pubkey::default()`"), separado de la cuenta `user` (el signer/payer). Quien paga el gas y firma la transacción no tiene por qué ser la misma wallet que queda registrada como `creator` del token.

Confirmado en el propio `CreateEvent` del IDL, que trae `user` y `creator` como dos campos independientes desde el momento de creación — no hace falta inferir nada, ambos vienen dados.

**Observado en mainnet (2026-09-28)**: la wallet `CVchGjSiutVmw5q5Gv2K5xYyKKnNxG3imqV7jwd5Dza6` creó `SNOW` como `user == creator`, y 51 minutos antes pagó (`user`) la creación de `BINGE` (`34jt5NvH…pump`) con `creator = 4Mv3VAWbhK7GzPH7Csp9q6tYVCvsh9HA7mFYfxWVwtHR`. `user ≠ creator` ocurre en la práctica, no solo en teoría. `marxol operator` lo muestra como "creaciones pagadas para otro creator" (señal candidata para H3).

**Decisión de diseño**: la identidad de "operador" para efectos de `operator_tracker` es el campo `creator`, no `user`/`payer` — es el campo económicamente relevante (recibe fees) y el que el propio protocolo trata como identidad del proyecto.

### 5.2 El `creator` puede cambiar después de la creación — por DOS mecanismos distintos

Este es el hallazgo que más cambia el diseño frente a la intuición desde EVM/Pons. `BondingCurve.creator` **no es estable de por vida**. Hay dos formas legítimas de que cambie, confirmadas ambas directamente en el IDL:

**a) Creator fee sharing** (reparto de fees entre varios shareholders)
- `create_fee_sharing_config`: migra `bonding_curve.creator` (y `pool.coin_creator` si ya graduó) a una PDA `sharing_config`. Lista inicial de shareholders: `[(creator, 10_000 bps)]`. Callable por el creador actual o por `global.admin_set_creator_authority`.
- `update_fee_shares_v2`: fija la lista final de shareholders (hasta 10, `share_bps` suma 10_000). Solo se puede llamar una vez por `sharing_config` (el admin queda revocado después).
- Evento: `MigrateBondingCurveCreatorEvent` — campos: `timestamp, mint, bonding_curve, sharing_config, old_creator, new_creator`.

**b) `SetCreatorEvent` — reasignación directa, sin pasar por fee-sharing**
- Existe un evento `SetCreatorEvent` independiente (`timestamp, mint, bonding_curve, creator`) que **no aparece documentado en la doc Markdown oficial**, solo se detectó al parsear el IDL directamente. El struct `BondingCurve` tiene un campo `can_edit_creator_fee: bool` asociado a esta capacidad.
- **Quién puede llamarlo (del IDL, 2026-09-28)**: la instrucción `set_creator` exige como signer `set_creator_authority`, que debe ser `Global.set_creator_authority` (hoy `39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg`, la misma wallet que `withdraw_authority`). Doc del IDL: "Allows Global::set_creator_authority to set the bonding curve creator from Metaplex metadata or input argument". **Es una reasignación administrativa del protocolo, no algo que el creador haga por sí mismo** — interpretarla así en el análisis.
- `migrate_bonding_curve_creator` (la que emite `MigrateBondingCurveCreatorEvent`) **no tiene signer**: es permissionless y sincroniza el creator con una `sharing_config` que vive en otro programa, el de fees: `pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ` (seeds `["sharing-config", mint]`). `create_fee_sharing_config`/`update_fee_shares_v2` son instrucciones de ese programa, no de pump.
- **Frecuencia observada**: 0 `SetCreatorEvent` y 0 `MigrateBondingCurveCreatorEvent` en muestras de 400 tx de `set_creator_authority` y 300 tx del programa de fees (2026-09-28). Son eventos raros; hace falta indexado prolongado para tener casos reales.

**Consecuencia de diseño obligatoria**: para reconstruir la identidad de "operador" de un token a lo largo del tiempo hay que escuchar y loguear **tres eventos por separado**, nunca colapsarlos en un solo campo mutable:

1. `CreateEvent.creator` — creador original, inmutable, momento de creación.
2. `SetCreatorEvent` — reasignación directa detectada.
3. `MigrateBondingCurveCreatorEvent` — migración a reparto de fees (`old_creator → new_creator` vía `sharing_config`).

Tratar estas tres señales como eventos distintos en el esquema de datos, no como un único campo `creator` que se sobreescribe — de lo contrario se pierden "cambios de operador" reales o se generan falsos positivos cuando en realidad es solo una migración a reparto de fees.

### 5.3 No hay eventos/logs como en EVM — mecanismo Anchor

pump.fun usa el patrón Anchor `#[event_cpi]` / `emit_cpi!`: el programa se llama a sí mismo con una instrucción no-op y el payload del evento va como datos de esa inner instruction (cuentas `event_authority` y `program` presentes en cada instrucción). Alternativamente, el patrón más antiguo `emit!()` escribe logs base64 prefijados con `Program data:`.

**Flujo correcto de indexado**: `solana-client` para leer transacciones (`getTransaction`) → extraer las inner instructions dirigidas al `event_authority` PDA → decodificar con Anchor/Borsh contra el IDL. No parsear logs de texto a mano — se truncan si son largos y no tienen garantía de formato estable.

### 5.4 Token program: Token-2022, no SPL clásico

El mint se crea con **Token-2022** (`TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb`), no el SPL Token Program clásico. Afecta a cómo se consultan balances y metadata — las librerías `spl-token` clásicas no sirven tal cual, hace falta `spl-token-2022`.

El mint authority real durante la fase bonding curve es una PDA del programa (`mint-authority`), no una wallet de usuario — coherente con que "creador" es un concepto de negocio (campo en la bonding curve), no la autoridad criptográfica del mint.

### 5.5 Quote mints múltiples (SOL y USDC)

Desde `create_v2`, un token puede crearse con quote mint SOL (por defecto, `quote_mint = Pubkey::default()` o wSOL `So11111111111111111111111111111111111111112`) o USDC. **Observado en mainnet**: los tokens SOL traen `quote_mint = 11111111111111111111111111111111` (`Pubkey::default()`), no wSOL. El único quote whitelisteado en `Global.whitelisted_quote_mints` es USDC (`EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v`). El campo `quote_mint` en `BondingCurve`, `CreateEvent` y `TradeEvent` indica cuál. El indexador debe soportar ambos desde el esquema base, no asumir SOL fijo.

---

## 6. Eventos del IDL — referencia de campos (confirmado directamente contra `idl/pump.json`)

Fuente: IDL Anchor oficial descargado de `pump-fun/pump-public-docs`, parseado programáticamente (no prosa de terceros). Copia versionada en `idl/pump.json` (sha256 `ffe966c4…c8b56064b`, descargada 2026-09-28): **47 instrucciones, 7 cuentas** (`BondingCurve, FeeConfig, Global, GlobalVolumeAccumulator, QuoteControl, SharingConfig, UserVolumeAccumulator`), 28 eventos, 42 tipos. (Una versión anterior de este documento decía 25 instrucciones y 5 cuentas: el IDL ha crecido.)

**Verificación contra chain**: la cuenta `Global` real (mainnet, 2026-09-28) decodifica con este IDL con los 29 campos y **0 bytes sobrantes**; un `CreateEvent` real de `create_v2` decodifica con todos sus campos. El IDL local está al día con el programa desplegado a esa fecha.

### `CreateEvent`
```
name, symbol, uri, mint, bonding_curve, user, creator, timestamp,
virtual_token_reserves, virtual_sol_reserves, real_token_reserves,
token_total_supply, token_program, is_mayhem_mode, is_cashback_enabled,
quote_mint, virtual_quote_reserves, creator_fee_bps, is_holder_reward
```

### `TradeEvent` (32 campos — el más rico; ya trae `creator` embebido, no hace falta cruzar con `CreateEvent` por `mint`)
```
mint, sol_amount, token_amount, is_buy, user, timestamp,
virtual_sol_reserves, virtual_token_reserves, real_sol_reserves, real_token_reserves,
fee_recipient, fee_basis_points, fee,
creator, creator_fee_basis_points, creator_fee,
track_volume, total_unclaimed_tokens, total_claimed_tokens,
current_sol_volume, last_update_timestamp, ix_name,
mayhem_mode, cashback_fee_basis_points, cashback,
buyback_fee_basis_points, buyback_fee,
shareholders (Vec<Shareholder>),
quote_mint, quote_amount, virtual_quote_reserves, real_quote_reserves,
holder_rewards_bps, holder_rewards
```
`ix_name` indica qué instrucción concreta generó el trade (`buy`, `buy_v2`, `buy_exact_quote_in_v2`, etc.) — útil para distinguir variantes.

### `CompleteEvent` (bonding curve se llena)
```
user, mint, bonding_curve, timestamp, quote_mint
```

### `CompletePumpAmmMigrationEvent` (graduación completada, liquidez ya en PumpSwap)
```
user, mint, mint_amount, sol_amount, pool_migration_fee, bonding_curve, timestamp, pool, quote_mint
```
Dos eventos separados (`CompleteEvent` y este) con timestamps propios — permite medir "tiempo hasta graduación" como métrica.

### `SetCreatorEvent`
```
timestamp, mint, bonding_curve, creator
```

### `MigrateBondingCurveCreatorEvent`
```
timestamp, mint, bonding_curve, sharing_config, old_creator, new_creator
```

### `CollectCreatorFeeEvent`
```
timestamp, creator, creator_fee, quote_mint
```

### `DistributeCreatorFeesEvent`
```
timestamp, mint, bonding_curve, sharing_config, admin, shareholders (Vec<Shareholder>), distributed, quote_mint
```

### `SetParamsEvent` (cambios de configuración global del programa)
```
initial_virtual_token_reserves, initial_virtual_sol_reserves, initial_real_token_reserves,
final_real_sol_reserves, token_total_supply, fee_basis_points, withdraw_authority,
enable_migrate, pool_migration_fee, creator_fee_basis_points, fee_recipients (array[8]),
timestamp, set_creator_authority, admin_set_creator_authority
```

### Otros eventos del IDL (28 en total, no todos relevantes para el MVP)
`AddQuoteControlMintEvent, AdminCtoEvent, AdminSetIdlAuthorityEvent, AdminUpdateTokenIncentivesEvent, ClaimCashbackEvent, ClaimTokenIncentivesEvent, CloseUserVolumeAccumulatorEvent, ExtendAccountEvent, InitUserVolumeAccumulatorEvent, MinimumDistributableFeeEvent, RemoveQuoteControlMintEvent, ReservedFeeRecipientsEvent, SetMetaplexCreatorEvent, SetQuoteControlAdminEvent, SyncUserVolumeAccumulatorEvent, UpdateCreatorFeeConfigEvent, UpdateGlobalAuthorityEvent, UpdateMayhemVirtualParamsEvent`

### Structs de cuenta relevantes

**`BondingCurve`**:
```
virtual_token_reserves, virtual_quote_reserves, real_token_reserves, real_quote_reserves,
token_total_supply, complete, creator, is_mayhem_mode, is_cashback_coin,
quote_mint, creator_fee_bps, can_edit_creator_fee, is_holder_reward
```

**`Global`**: ver sección 4 ("Parámetros económicos").

**`Shareholder`** (usado en `TradeEvent.shareholders` y `DistributeCreatorFeesEvent.shareholders`):
```
address: pubkey, share_bps: u16
```

---

## 7. Proveedor RPC/indexador

**Base: Alchemy** (mismo proveedor que ya se usaba en marXi), pero la dinámica de límites es estructuralmente distinta por el volumen de Solana frente a Robinhood Chain.

### Cifras de referencia (free tier, confirmadas sep 2026)

| Proveedor | Cuota gratuita | Rate limit | Coste de métodos históricos | Streaming |
|---|---|---|---|---|
| Alchemy | 30M CU/mes | 25 req/s (500 CU/s) | `getTransaction`=40 CU, `getSignaturesForAddress`=40 CU, `getBlock`=40 CU, `getProgramAccounts`=20 CU | Yellowstone gRPC facturado aparte por bandwidth ($75/TB, prorrateado); disponibilidad en free tier no confirmada explícitamente — **pendiente de verificar en el dashboard real antes de diseñar sobre ello** |
| Helius | 1M créditos/mes | 10 req/s, 2 req/s DAS, 2 conexiones WebSocket | Métodos de archivo (`getTransaction`, `getBlock`, `getSignaturesForAddress`) = 10 créditos (10x el resto) | `logsSubscribe`/WebSocket estándar sí disponible en free tier; LaserStream gRPC solo en plan Professional (pago) |
| QuickNode | 10M créditos/mes | — | — | Streams como add-on de pago |
| dRPC | 210M CU/mes | Solo contra nodos públicos compartidos | — | — |

### Por qué el polling puro no escala aquí (a diferencia de Robinhood Chain)

Volumen citado: 9,000–24,000 lanzamientos diarios solo en pump.fun en picos, sin contar trades. Con Alchemy (25 req/s, 40 CU por `getTransaction`), indexar vía `getSignaturesForAddress` + `getTransaction` por firma agota la cuota mensual antes de fin de mes y el rate limit deja el indexador sistemáticamente por detrás del chain en momentos de actividad alta (bloques cada ~400ms).

### Decisión de arquitectura

1. **Captura en tiempo real de creaciones**: WebSocket `logsSubscribe` filtrado por el program ID de pump.fun (gratis en Alchemy y Helius), reforzado con `getTransaction` puntual cuando el log se trunca o hace falta el payload completo decodificado. Nota: `blockSubscribe` está marcado "inestable" oficialmente por el equipo de Solana (desconexiones reportadas en carga alta) — no usar como vía principal.
2. **Medir consumo real antes de descartar Alchemy**: el caso de uso (solo eventos de creación + cambios de creador, no todo el volumen de swaps) es mucho más ligero que indexar trades completos. No asumir que el free tier no alcanza sin medirlo con tráfico real de un día.
3. **Benchmarking Helius como alternativa**, especialmente por el coste distinto por método (10 créditos por llamada de archivo vs. 40 CU-equivalente de Alchemy) — puede rendir distinto según el patrón real de consumo.
4. **Devnet para probar el pipeline de decodificación** sin gastar cuota de mainnet (el programa pump.fun también está desplegado en devnet).
5. Preferencia declarada: no depender de plataformas de pago — free tier combinado de RPC público + Alchemy free, igual que en marXi.

---

## 8. Hipótesis a pre-registrar (antes de mirar datos)

Pre-registradas aquí antes de empezar el análisis, para evitar sesgo de confirmación (principio heredado de marXi, sección 2).

### H1 — señal temprana de actividad orgánica (redefinida 2026-09-28, antes de generar datos de trades)

Sustituye a la versión anterior de H1 ("la tasa de éxito de un mismo `creator` difiere entre operadores repetidores y de un solo lanzamiento", con éxito = graduación o mcap sostenido). Esa medida retrospectiva pasa a ser **H1b** (validación, ver abajo), no el criterio de entrada. La comparación repetidores vs. un solo lanzamiento queda para cuando H1 esté validada, usando H1 como variable por token.

**Hipótesis**: la presencia, en los primeros minutos de vida de un token, de actividad de compra y venta de varias wallets independientes (no el creador, no bots, no ballenas) es una señal temprana y accionable que anticipa el éxito retrospectivo del token (H1b).

> ⚠️ **Valores provisionales**: los tres umbrales numéricos marcados con **[P]** (ventana de **5 min**, tope por trade de **15 %**, mínimo de **3 wallets**) y los umbrales de bot marcados igual son valores iniciales, **no un criterio cerrado**. Se recalibrarán con datos reales. Para no sobreajustar contra H1b, la recalibración se hace solo sobre un tramo de calibración (los tokens más antiguos) y la validación contra H1b sobre tokens posteriores que no se usaron para recalibrar (partición temporal). Cada recalibración se anota aquí con fecha, valores viejos → nuevos y motivo.

**Criterio operacional**, por token (mint), en este orden:

1. **Ventana** **[P]**: `TradeEvent` con `CreateEvent.timestamp ≤ t < CreateEvent.timestamp + 300 s` (5 min). Los `timestamp` de los eventos tienen resolución de 1 s; para ordenar trades del mismo segundo se usa `(slot, índice de tx en el bloque, índice del evento)`.
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

- **Primaria**: el token gradúa (`CompletePumpAmmMigrationEvent` del mint) en las **24 h** siguientes a `CreateEvent`.
- **Secundaria**: ratio de mcap sostenido a N horas post-creación (candidato N = 24 h; fijar N y la definición exacta de "sostenido" antes de mirar datos de precio).
- **Prueba**: tabla 2×2 (H1 × graduó) sobre los tokens del tramo de validación; test exacto de Fisher, α = 0.05; se reporta la tasa de graduación condicionada a H1 = true y a H1 = false, y la odds ratio con su intervalo de confianza. Si no hay diferencia significativa, H1 se documenta como descartada con el mismo detalle que si se confirmara. Recordar que una asociación aquí es correlación, no causalidad (principio 6).
- **H2**: los operadores que migran a `sharing_config` (`MigrateBondingCurveCreatorEvent`) tienen un perfil de comportamiento distinto (más profesionalizados, más colaborativos, más propensos a repetir lanzamientos) que los que no lo hacen. Señal nueva, sin equivalente en el modelo de Pons/Robinhood Chain.
- **H3**: el criterio estadístico de financiación externa vs. ingreso propio del negocio usado en marXi para Robinhood Chain probablemente necesita rehacerse desde cero para Solana — el patrón de wallets financiadoras aquí puede no parecerse al de Pons (muchos operadores compran SOL directamente en CEX o usan bridges distintos). No asumir que el mismo modelo estadístico traslada sin validarlo con datos de Solana.

Al descartar o confirmar cada hipótesis con datos reales, documentarlo aquí con el mismo detalle independientemente del resultado (principio 6 de la sección 2).

---

## 9. No verificado — pendiente de confirmar contra la chain real antes de construir encima

Esta sección existe porque el principio 1 de la filosofía marXi lo exige: nada de esta lista se ha confirmado contra un nodo RPC en vivo, solo contra documentación pública oficial (repo `pump-fun/pump-public-docs` en GitHub) y el IDL descargado de ahí. Antes de escribir código que dependa de estos valores, verificarlos directamente contra `getAccountInfo` / lectura de la cuenta `Global` real en mainnet:

- ~~Que el program ID es ejecutable y corresponde al programa pump.fun activo~~ → **verificado 2026-09-28** (sección 4).
- ~~El umbral de graduación real actual~~ → **verificado 2026-09-28**: 85.0054 SOL reales (sección 4). Nota: `final_real_sol_reserves` no está en `Global`, solo en `SetParamsEvent`; el umbral se deriva de `initial_*`.
- Si Yellowstone gRPC de Alchemy está disponible en el plan free o requiere pago — no documentado explícitamente por Alchemy, confirmar en el dashboard real.
- Cualquier cambio reciente en el IDL no reflejado en la copia descargada (el repo está activo, con actualizaciones frecuentes — releer antes de asumir que esta sección sigue vigente si ha pasado tiempo desde la última actualización de este documento). Señales de desfase: `marxol verify` muestra campos ausentes o bytes sobrantes en `Global`, o aparecen "evento con discriminador desconocido" / "campos del IDL ausentes" al decodificar tx recientes.
- Casos reales de `SetCreatorEvent` y `MigrateBondingCurveCreatorEvent` (no observados aún, ver 5.2).

---

## 10. Siguientes pasos

1. ~~Verificar contra RPC real los puntos de la sección 9~~ — hecho 2026-09-28 salvo Yellowstone.
2. ~~Prototipo de decodificación de `create_v2` reales~~ — hecho (sección 11).
3. ~~Esquema de datos para las tres señales de identidad~~ — hecho en `src/store.rs` (sección 11). Pendiente: observar casos reales de `SetCreatorEvent`/`MigrateBondingCurveCreatorEvent`.
4. Benchmark real de consumo (CU/créditos) de un día de captura vía WebSocket antes de comprometerse a un proveedor único.
5. ~~Definir explícitamente el criterio de "éxito" de H1 antes de tocar datos~~ — hecho 2026-09-28 (sección 8, con valores provisionales).
   Pendiente para implementarlo: el indexador aún no guarda `TradeEvent`. Hace falta almacenar los trades de la ventana temprana de cada token (con `slot` e índices para ordenar) y calcular H1 y H1b por token.
6. Registrar cada hallazgo nuevo en este archivo en la misma sesión en que se confirme.

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
| `marxol operator <creator>` | Informe de un operador sobre lo indexado |
| `marxol stats` | Recuento de filas |

`--json` sirve para todos los comandos que imprimen eventos o cuentas.

**Módulos**:
- `src/idl.rs`: decodificador Borsh genérico dirigido por `idl/pump.json` (embebido en el binario). No hay structs escritos a mano por evento. Si los bytes son más cortos que el IDL actual (eventos o cuentas históricos, de antes de que se añadieran campos al final), los campos que faltan se reportan en `missing`, nunca se rellenan con valores por defecto.
- `src/tx.rs`: `getTransaction` → inner instructions al programa pump cuyo primer account es el `event_authority` (PDA `["__event_authority"]`) → prefijo `e445a52e51cb9a1d` (sha256("anchor:event")[..8]) + discriminador de evento → Borsh. Incluye las cuentas cargadas desde lookup tables.
- `src/store.rs`: tablas `creations` (creator original, inmutable), `creator_changes` (una fila por evento, `kind ∈ {set_creator, migrate_to_sharing}`, nunca sobreescribe), `completions`, `migrations`, `seen_signatures`.
- `src/pump.rs`: constantes, PDAs y matemática de graduación.

**Hallazgos operativos (mainnet, 2026-09-28)**:
- **Transacciones versión 1 en mainnet**: `getTransaction` con `maxSupportedTransactionVersion: 0` falla (error -32015) con parte del tráfico de pump. Hay que pedir `1`. `solana-transaction-status-client-types 4.3` las decodifica bien en encoding `json`.
- **~68 % de las firmas del programa pump son tx fallidas** (203 de 300 en una muestra). `getSignaturesForAddress` ya trae `err`, así que el indexador las descarta sin gastar un `getTransaction` (40 CU en Alchemy) en cada una. Es un ahorro grande de cuota que hay que tener en cuenta en el benchmark del paso 4.
- En una muestra de 97 tx exitosas del programa: 57 `TradeEvent`, 22 `DistributeFeeToHoldersEvent`, 3 `CollectCreatorFeeEvent`, 1 `CreateEvent`. Las creaciones son una fracción pequeña del tráfico: recorrer las firmas del programa entero para encontrarlas es caro. Para seguir a un operador concreto es mucho más barato `index --address <creator>`.
- Algunos mints no acaban en `pump` (p. ej. `zVbJ3eRj…phi`). No usar el sufijo como filtro.
- Observado un `CreateEvent` con `creator_fee_bps = 0` mientras `Global.creator_fee_basis_points = 5`. Sin interpretar todavía: no está verificado cómo se combinan ambos campos.

---

## Fuentes

- [pump-fun/pump-public-docs (GitHub)](https://github.com/pump-fun/pump-public-docs) — repo oficial, incluye `idl/pump.json`
- [COIN_CREATION.md](https://github.com/pump-fun/pump-public-docs/blob/main/docs/instructions/COIN_CREATION.md)
- [CREATOR_FEE_SHARING.md](https://github.com/pump-fun/pump-public-docs/blob/main/docs/instructions/CREATOR_FEE_SHARING.md)
- [PUMP_PROGRAM_README.md](https://github.com/pump-fun/pump-public-docs/blob/main/docs/PUMP_PROGRAM_README.md)
- IDL crudo: `https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/idl/pump.json`
- [pump-rust-client (crates.io)](https://crates.io/crates/pump-rust-client)
- [Alchemy Compute Unit Costs](https://www.alchemy.com/docs/reference/compute-unit-costs)
- [Alchemy Yellowstone gRPC Overview](https://www.alchemy.com/docs/reference/yellowstone-grpc-overview)
- [Helius Pricing](https://www.helius.dev/pricing)
