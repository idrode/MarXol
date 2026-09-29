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
- **Primeros casos reales de `MigrateBondingCurveCreatorEvent` (2026-09-28)**: 4 de 377 creaciones consecutivas (~1 %), **todos dentro de la propia tx de creación** (`create_v2` + compra + `MigrateBondingCurveCreatorEvent` + `DistributeCreatorFeesEvent`), con `new_creator == sharing_config`. Es decir, el reparto de fees se configura al crear, no después: desde el primer trade el `creator` embebido es la PDA `sharing_config`, no la wallet original. Ejemplo: tx `2QScQ4JqMia8tfR4FqB4h1PqpqSRJTS8MmjoV86KYpSFzuH9D6a6J3nE5CLR53iSqMuhD4zbD7XxncsG56i5sF7`, mint `4m9CcfsLKH…`. Implicación para H2: "migra a sharing_config" puede ser una decisión de lanzamiento, no un paso posterior de profesionalización. `SetCreatorEvent` sigue sin observarse.

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

**Corrección (2026-09-28, 377 creaciones consecutivas de mainnet)**: hay **muchos más quote mints que SOL y USDC**. 12 distintos en la muestra: SOL 255 (68 %), `N7Q5fYX7YRnDQksfdBKnoUb3awm92n7QNAD35X3Rq1X` 95 (25 %), USDC 11, y otros 9 con 1–4 tokens cada uno (entre ellos `pumpCmXq…`, `SPCXxcqX…`, `Xsc9qvGR…`). La lista `whitelisted_quote_mints` de `Global` no es la lista completa de quotes posibles; probablemente entran por el mecanismo `QuoteControl` (`AddQuoteControlMintEvent`), **no verificado**. Qué es `N7Q5fYX7…` está sin identificar. Consecuencia: nunca comparar importes o mcap en valor absoluto entre tokens de quotes distintos sin convertirlos; las métricas relativas (CV, fracción del volumen, ratios de mcap) sí son comparables.

**Campos de importe del `TradeEvent`** (observado): con quote SOL, `sol_amount == quote_amount` y `virtual_sol_reserves == virtual_quote_reserves`; con otro quote, `sol_amount = 0` y `virtual_sol_reserves = 0`, y el importe va solo en `quote_amount`/`virtual_quote_reserves`. Usar siempre los campos `quote_*`.

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

1. **Ventana** **[P]**: `TradeEvent` con `CreateEvent.timestamp ≤ t < CreateEvent.timestamp + 300 s` (5 min). Los `timestamp` de los eventos tienen resolución de 1 s. Los intervalos del paso 3 se calculan sobre los `timestamp` ordenados, así que el orden entre trades del mismo segundo no los cambia; no hace falta el índice de tx dentro del bloque (que `getTransaction` no da). Orden de almacenamiento: `(slot, timestamp, firma, índice del evento)`.
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

Fijado 2026-09-28, antes de mirar datos de precio.

- **Ventanas**: **N = 1 h** desde `CreateEvent` **[P]** es la ventana **primaria**, la que valida H1, coherente con el objetivo de entrada temprana. **24 h** se mantiene como ventana **secundaria**, no descartada: sirve para `operator_tracker` a más largo plazo, pero no se usa para decidir si H1 sirve para entrar rápido.
- **Métricas**, calculadas en cada ventana:
  - **Graduación**: existe `CompletePumpAmmMigrationEvent` del mint dentro de la ventana.
  - **Pump sostenido**: sea `pico` el mcap máximo alcanzado dentro de la ventana y `cierre` el mcap al final de la ventana (último precio observado antes del cierre). Es "sostenido" si `cierre ≥ (1 − X) · pico` con **X = 30 %** **[P]** de caída máxima desde el pico. Así se distingue "hubo pump y aguantó" de "pumpeó y se desplomó justo antes de la marca", cosa que no hace comparar dos puntos fijos (creación vs. N).
  - **Condición de pump** **[P]**, propuesta al fijar la métrica y confirmada por Roi (2026-09-28): un token que nunca subió no cae desde su pico y saldría "sostenido" sin haber tenido pump. Por eso además se exige `pico ≥ 2 × mcap inicial` (mcap en el `CreateEvent`). Sin esa condición la métrica mide "no se desplomó", no "hubo pump real".
  - **mcap** = precio × `token_total_supply`, con precio = `virtual_quote_reserves / virtual_token_reserves` del último `TradeEvent` (en unidades del `quote_mint`: no mezclar tokens SOL y USDC en el mismo umbral absoluto; el criterio es relativo, así que funciona para ambos). Tras la graduación el precio sale del pool de PumpSwap (`effective_quote_reserves`, sección 4), no de la bonding curve.
- **Prueba**: sobre los tokens del tramo de validación, una tabla 2×2 por métrica (H1 × graduó en 1 h; H1 × pump sostenido en 1 h). Test exacto de Fisher, α = 0.05. Se reporta la tasa de éxito condicionada a H1 = true y a H1 = false, y la odds ratio con su intervalo de confianza. Las mismas tablas en 24 h se reportan como secundarias. Si no hay diferencia significativa, H1 se documenta como descartada con el mismo detalle que si se confirmara. Recordar que una asociación aquí es correlación, no causalidad (principio 6).
- **Datos que exige**: el pump sostenido necesita la trayectoria de precio de toda la ventana (1 h y 24 h), no solo los trades de los 5 min de H1. Implementado para 1 h (`marxol prices` + `marxol h1b`); 24 h pendiente.
- **Limitación de la implementación actual (2026-09-28)**: solo se descarga el precio de la bonding curve. Si la curva se completa dentro de la ventana, el precio posterior está en PumpSwap, que todavía no se lee. En ese caso "pump sostenido" queda **no evaluable** (se excluye de su tabla 2×2), no se da por cumplido. La graduación sí se mide siempre. Precio inicial = reservas virtuales del `CreateEvent`; cada punto posterior = reservas virtuales tras cada `TradeEvent`.

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

### Validación de H4', H5, H6', H7', H8 — CONGELADO antes de descargar las 337 (2026-09-29 15:24 UTC)

Definiciones, umbrales y criterio fijados **antes de descargar ninguno de los 337 tokens pendientes**. Se evalúan **una sola vez** con `marxol entry2` sobre el tramo de validación. Cualquier cambio posterior es una hipótesis nueva que exige tokens nuevos. Sustituye a las propuestas post hoc anteriores (T_entry2, H4'/H6'/H7', H8), que quedan fijadas aquí.

**Tramos**:
- Calibración: los 40 del piloto, creaciones con slot ≤ **451 340 397**.
- Validación: creaciones con slot > 451 340 397. Son los 337 indexados el 2026-09-28 entre las 14:03 y las 14:11 UTC; el primero, en el slot 451 340 405.
- La frontera es limpia (no hay slots compartidos) y está en el código: `entry2::PILOT_LAST_SLOT`.
- `entry2` se niega a cruzar el piloto con el resultado (solo admite `--check`).

**T_entry2** (definición primaria única):
- Es el primer `TradeEvent` en **orden de ejecución** (encadenado de reservas, sección 11) que está en un **slot posterior al de creación** y tiene `timestamp ≥ t0 + 10 s`, buscado dentro de la ventana temprana `[t0, t0 + 300 s)`.
- **Anteriores a T_entry2** = los trades anteriores en orden de ejecución, sin incluirlo. Todos los indicadores se calculan solo con ellos.
- Estado en T_entry2 = reservas tras el último trade anterior (las del `CreateEvent` si no hay ninguno).
- Token sin ningún trade que cumpla la condición dentro de la ventana: **sin entrada**, excluido de las tablas y contado aparte. También se cuentan aparte los tokens con entrada pero sin ventana de precio completa.

**Indicadores** (dirección esperada en los cinco: señal = true ⇒ más pump-y-caída):

| | Definición | Umbral |
|---|---|---|
| H4' | Alguna identidad de creador (la unión del paso 2 de H1) vende antes de T_entry2. Se guarda también el % del supply vendido | ≥ 1 venta |
| H5 | Sin cambios respecto a la ronda anterior: ≥ 1 wallet ajena al creador compra en el slot de creación | ≥ 1 wallet |
| H6' | % de llenado en T_entry2 = `(V0 − V) / 793.1e12`, con `V` = `virtual_token_reserves` en T_entry2 y 793.1e12 las reservas reales iniciales estándar. Mide tokens vendidos, así que no depende del quote ni de los cambios de reservas virtuales de quote de los tokens mayhem. **n/e** si `V0 ≠ 1.073e15` (las reservas reales iniciales no se guardan por token) | **≥ 13.65 %** = mediana en los 28 tokens del piloto con T_entry2 (media de 11.84 y 15.46), calculada sin cruzar con el resultado |
| H7' | Wallets ajenas al creador con exactamente 1 compra + 1 venta antes de T_entry2 | **≥ 1** = mediana del piloto (n = 28), que **no es 0**: 12 tokens con 0 y la 14.ª y 15.ª observación valen 1 |
| H8 | % del supply (10^15) comprado por identidades de creador en el slot de creación. **Origen post hoc**: se formuló mirando el piloto (7 tokens con entrada a 1.5× en la sola compra del creador), así que solo se evalúa en tokens nuevos | **≥ 15 %** |

Mediana convencional: con n par, la media de los dos valores centrales.

**Resultado primario**: pump-y-caída con la definición vigente (H1c/H1b): pico ≥ 2× el precio inicial y caída > 30 % del pico al cierre, en `[t0, t0 + 1 h)`. Exige ventana de precio completa de ≥ 1 h.

**Criterio de éxito**, por hipótesis, sobre los tokens con T_entry2 y resultado evaluable. Es **CONFIRMADA** si se cumplen las cuatro condiciones:
1. ≥ 3 tokens en cada grupo.
2. p₁ − p₀ ≥ 20 puntos en la fracción de pump-y-caída.
3. **Fisher bilateral p < 0.01** (≈ 0.05 / 5, corrección por 5 hipótesis).
4. p₁ − p₀ > 0 también sin el **token de mayor peso**, definido como el token con más trades en su ventana de 5 min entre los que entran en las tablas (definición ciega al resultado).

Si falla alguna es **NO CONFIRMADA**, y se indica si la dirección es la opuesta. Si algún grupo tiene < 3 tokens es **NO EVALUABLE**. La odds ratio y su IC se reportan, pero no deciden.

**Resultado secundario** (descriptivo, sin veredicto): retorno neto desde T_entry2 a +10, +30 y +60 min.
- Fórmula: `(P_salida / P_entrada) · (1 − f)² − 1`.
  - `f = (fee_basis_points + creator_fee_basis_points) / 10⁴` del `TradeEvent` de T_entry2 (entrada y salida con la misma comisión).
  - `P_entrada` = precio en el estado de T_entry2.
  - `P_salida` = último punto de precio de la curva con `timestamp ≤ t_T_entry2 + Δ`.
- Posición marginal: sin impacto de precio. No se cuentan otras componentes (cashback, buyback, holder rewards).
- **n/e** si la curva se completa antes de la salida (el precio pasa a PumpSwap, que no se descarga), si la salida cae fuera del rango descargado o si no se conoce la comisión.
- Se reportan la mediana por grupo de cada indicador y Mann-Whitney bilateral (aproximación normal con corrección por empates y continuidad).
- Por eso `prices` descarga ahora 65 min (`[t0, t0 + 3 900 s)`): T_entry2 ≤ t0 + 5 min, así que +60 min queda cubierto. El cálculo de H1b sigue recortando a 1 h.
- Las comisiones solo se guardan desde esta versión, así que en el piloto el retorno es n/e (0 de 28 con comisión).

**Chequeo de no-degeneración en el piloto** (`marxol entry2 --check --tramo piloto`, sin calcular el resultado):
- 28 de 40 con T_entry2 y 12 sin entrada; orden de ejecución completo en 28/28.
- Grupos true/false: H4' 17/11, H5 16/12, H6' 14/14, H7' 16/12, H8 9/19. Ninguno degenerado.
- T_entry2: mediana a 12 s, 15.5 trades antes de entrar, múltiplo de precio mediano 1.31×.
- En 2 tokens mayhem el múltiplo es < 1: sus reservas virtuales de quote cambian fuera de los trades. No es un error.

### H2 y H3

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
- Casos reales de `SetCreatorEvent` (no observados aún, ver 5.2). `MigrateBondingCurveCreatorEvent` ya observado (2026-09-28).

---

## 10. Siguientes pasos

1. ~~Verificar contra RPC real los puntos de la sección 9~~ — hecho 2026-09-28 salvo Yellowstone.
2. ~~Prototipo de decodificación de `create_v2` reales~~ — hecho (sección 11).
3. ~~Esquema de datos para las tres señales de identidad~~ — hecho en `src/store.rs` (sección 11). Pendiente: observar casos reales de `SetCreatorEvent`/`MigrateBondingCurveCreatorEvent`.
4. Benchmark real de consumo (CU/créditos) de un día de captura vía WebSocket antes de comprometerse a un proveedor único.
5. ~~Definir explícitamente el criterio de "éxito" de H1 antes de tocar datos~~ — hecho 2026-09-28 (sección 8, con valores provisionales).
   H1 implementada (2026-09-28): `marxol windows` + `marxol h1`. Pendiente: H1b, que necesita la trayectoria de precio de 1 h y 24 h (bonding curve y, si gradúa, PumpSwap), y ninguna de las dos está descargada.
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
   **Validación congelada (2026-09-29 15:24 UTC)**: sección 8. Secuencia pendiente de ejecutar (no lanzada; necesita red y decidir proveedor):
   1. `marxol windows --limit 337 --provider alchemy --max-requests K`
   2. `marxol prices --limit 337 …`, que se puede repetir hasta completar.
   3. `marxol entry2 --check` (no-degeneración en validación, sin resultado).
   4. `marxol entry2`, **una sola vez**.
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
| `marxol windows [--limit N] [--max-pages P] [--provider auto\|public\|alchemy\|helius] [--max-rps R] [--max-requests K]` | Para tokens indexados con la ventana temprana ya cerrada, recorre las firmas de su bonding curve hasta la creación y guarda los `TradeEvent` de la ventana; registra en `trade_windows` si la descarga fue completa |
| `marxol h1 [--variant v1\|h1c]` | Calcula la señal H1 (sección 8) sobre los tokens con ventana completa, la guarda en `h1_results` por variante e imprime el resumen y la distribución de intervalos 1 + 1 |
| `marxol prices [--limit N] [mismas opciones de ritmo]` | Para tokens con ventana temprana completa y la ventana de precio ya cumplida, guarda los puntos de precio de la bonding curve en `[t0, t0+65 min)` (1 h de H1b + 5 min para el retorno a +60 min desde T_entry2) (reutiliza los trades de la ventana temprana; pide el resto y la tx de creación) |
| `marxol h1b [--variant v1\|h1c]` | Calcula H1b en 1 h (graduación, pump sostenido) y la cruza con la variante de H1: tablas 2×2, test exacto de Fisher, odds ratio con IC 95 %, pump-y-caída por grupo y criterio (a) de H1c |
| `marxol entry` | T_entry (primer trade con precio ≥ 1.5× el inicial en 5 min, en orden de ejecución) e indicadores previos H4–H7, cruzados con pump-y-caída en 1 h: tablas 2×2 con y sin `CRHnzej9…`, veredicto del criterio pre-registrado y distribuciones. Sin RPC |
| `marxol entry2 [--check] [--tramo validacion\|piloto]` | Validación congelada (sección 8): T_entry2, H4', H5, H6', H7', H8 contra pump-y-caída en 1 h (Fisher, criterio congelado, sin el token de mayor peso) y retorno neto desde T_entry2 a +10/+30/+60 min (Mann-Whitney). `--check` solo cuenta grupos, sin resultado. Sin `--check`, se niega a usar el piloto. Sin RPC |
| `marxol operator <creator>` | Informe de un operador sobre lo indexado |
| `marxol stats` | Recuento de filas |

`--json` sirve para todos los comandos que imprimen eventos o cuentas.

**Módulos**:
- `src/idl.rs`: decodificador Borsh genérico dirigido por `idl/pump.json` (embebido en el binario). No hay structs escritos a mano por evento. Si los bytes son más cortos que el IDL actual (eventos o cuentas históricos, de antes de que se añadieran campos al final), los campos que faltan se reportan en `missing`, nunca se rellenan con valores por defecto.
- `src/tx.rs`: `getTransaction` → inner instructions al programa pump cuyo primer account es el `event_authority` (PDA `["__event_authority"]`) → prefijo `e445a52e51cb9a1d` (sha256("anchor:event")[..8]) + discriminador de evento → Borsh. Incluye las cuentas cargadas desde lookup tables.
- `src/store.rs`: tablas `creations` (creator original, inmutable), `creator_changes` (una fila por evento, `kind ∈ {set_creator, migrate_to_sharing}`, nunca sobreescribe), `completions`, `migrations`, `seen_signatures`; para H1: `early_trades` (los `TradeEvent` de la ventana temprana de mints con creación conocida, con el `creator` embebido del trade = creator vigente en ese momento), `trade_windows` (cobertura por mint: solo con `complete = 1` H1 es evaluable) y `h1_results` (resultado derivado por `(mint, variant)` con los parámetros usados; recalculable, se reemplaza; sustituye a la antigua `h1_signals`, que se borra al abrir la base).
- `src/h1.rs`: cálculo puro de H1 (sin RPC), con los umbrales [P] en `PARAMS_V1` (criterio vigente) y `PARAMS_H1C` (variante H1c, descartada en calibración).
- `src/h1b.rs`: cálculo puro de H1b (sin RPC), con los umbrales [P] en `PARAMS_1H`, y el test exacto de Fisher bilateral. Tablas de soporte en `store.rs`: `price_points` (`kind ∈ {create, trade}`) y `price_windows`.
- `src/entry.rs`: cálculo puro de T_entry y H4–H7 (sin RPC), con los umbrales [P] en `PARAMS`, y `execution_order`, que reconstruye el orden de ejecución de los trades encadenando reservas.
- `src/entry2.rs`: cálculo puro de T_entry2, los indicadores congelados (`PARAMS`, `PILOT_LAST_SLOT`), el retorno neto y el test de Mann-Whitney.
- `src/rpc.rs`: presupuesto de RPC por proveedor (`Budget`): ritmo máximo, tope de peticiones por ejecución y contador de peticiones y unidades reales (40 CU por método histórico en Alchemy, 10 créditos en Helius, 0 en el RPC público).
- **Descargas reanudables** (`windows`, `prices`):
  - Saltan tokens ya completos y, dentro de un token, las firmas ya ingeridas (tabla `fetched_sigs`, por propósito `window`/`prices`). No se usa `seen_signatures` porque una firma vista por `index` antes de conocer la creación no tiene sus trades guardados.
  - Al llegar a `--max-requests` paran limpiamente, y el token a medias sigue en la próxima pasada.
  - Cada ejecución deja su consumo en `rpc_usage` y lo imprime junto con el acumulado.
  - `early_trades` guarda ahora `fee_basis_points` y `creator_fee_basis_points` (se añaden a bases antiguas al abrirlas).
  - `windows` guarda también el precio inicial del `CreateEvent`.
- `src/pump.rs`: constantes, PDAs y matemática de graduación.

**Hallazgos operativos (mainnet, 2026-09-28)**:
- **Transacciones versión 1 en mainnet**: `getTransaction` con `maxSupportedTransactionVersion: 0` falla (error -32015) con parte del tráfico de pump. Hay que pedir `1`. `solana-transaction-status-client-types 4.3` las decodifica bien en encoding `json`.
- **~68 % de las firmas del programa pump son tx fallidas** (203 de 300 en una muestra). `getSignaturesForAddress` ya trae `err`, así que el indexador las descarta sin gastar un `getTransaction` (40 CU en Alchemy) en cada una. Es un ahorro grande de cuota que hay que tener en cuenta en el benchmark del paso 4.
- En una muestra de 97 tx exitosas del programa: 57 `TradeEvent`, 22 `DistributeFeeToHoldersEvent`, 3 `CollectCreatorFeeEvent`, 1 `CreateEvent`. Las creaciones son una fracción pequeña del tráfico: recorrer las firmas del programa entero para encontrarlas es caro. Para seguir a un operador concreto es mucho más barato `index --address <creator>`.
- **Vía barata para listar creaciones**: la PDA `mint-authority` (`TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM`, seeds `["mint-authority"]`, la imprime `marxol verify`) solo la tocan `create`/`create_v2`. En 400 firmas suyas: 22 tx fallidas y 378 exitosas, de las que 377 traen `CreateEvent` (la restante no está examinada). `index --address TSLvdd1p…` indexa creaciones sin recorrer el tráfico de trades del programa (~1 creación por cada 100 tx).
- **Ritmo de creación observado**: 377 creaciones en 492 s (2026-09-28, 14:03–14:11 UTC) ≈ 0.77/s. Es una muestra de 8 minutos, no una media diaria.
- **Orden de ejecución dentro de un slot (2026-09-29)**: ni `getTransaction` ni el orden `(slot, timestamp, firma)` lo dan (la firma es un hash). Se reconstruye exacto encadenando reservas: `virtual_token_reserves` antes de un trade = después + `token_amount` (compra) o − `token_amount` (venta), y coincide con el después del trade anterior, empezando por el `CreateEvent`. Encadena 2 831 de 2 831 trades de la ventana temprana del piloto. Hace falta en todo lo que dependa del orden dentro del slot (primer trade que cruza un umbral, precio de cierre). H1 y H1c no dependen de él (ver sección 8). Ignorarlo produjo un falso positivo en H5 (p = 0.026 → 0.42 tras corregir).
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
