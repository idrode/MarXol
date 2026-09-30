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

### Resultado de la validación congelada (2026-09-29, `marxol entry2`, ejecutado una sola vez)

**Alcance, antes que nada**: los 337 tokens de validación salen de **una única ráfaga de 8 minutos de un solo día** (creaciones del 2026-09-28 entre las 14:03 y las 14:11 UTC). Validan **dentro de esa ráfaga**. No generalizan a otros momentos, días ni regímenes de mercado: para eso hacen falta muestras de otras fechas y horas. Una asociación aquí es correlación, no causalidad (principio 6).

**Datos**:
- 337 ventanas tempranas y 337 ventanas de precio de 65 min completas, con 0 incompletas en ambas.
- Los 6 fallos puntuales de descarga (429) se reintentaron y completaron. La exclusión por incompleto quedó vacía.
- 86 tokens sin T_entry2, excluidos. **251 en tablas, 63 pump-y-caída (25 %)**.
- 0 tokens con T_entry2 sin ventana de precio. Orden de ejecución completo y comisión conocida en 251/251.
- Token de mayor peso: `9trcArtUpUePNEH6CvnFNDQAd8U9KqcjvPpN4z7LZFDm`.
- Los 40 de calibración no entran en nada de esto.

**Veredicto primario** (criterio congelado: dif. ≥ 20 pts, Fisher p < 0.01, ≥ 3 por grupo, misma dirección sin el token de mayor peso):

| Hipótesis | n señal / sin señal | p&c con señal | p&c sin señal | Dif. | Fisher p | OR (IC95) | Sin mayor peso | Veredicto |
|---|---|---|---|---|---|---|---|---|
| H4' dev vendió antes de T_entry2 | 73 / 178 | 28/73 (38 %) | 35/178 (20 %) | +19 | 0.0036 | 2.54 (1.40–4.63) | +18 | **NO CONFIRMADA** (dif. < 20 pts) |
| H5 wallet ajena en el slot de creación | 125 / 126 | 50/125 (40 %) | 13/126 (10 %) | +30 | < 0.0001 | 5.79 (2.95–11.40) | +29 | **CONFIRMADA** |
| H6' llenado ≥ 13.65 % | 65 / 186 | 38/65 (58 %) | 25/186 (13 %) | +45 | < 0.0001 | 9.06 (4.74–17.34) | +44 | **CONFIRMADA** |
| H7' ≥ 1 wallet 1+1 antes de entrar | 122 / 129 | 51/122 (42 %) | 12/129 (9 %) | +33 | < 0.0001 | 7.00 (3.50–14.03) | +32 | **CONFIRMADA** |
| H8 creador ≥ 15 % en el slot de creación | 18 / 233 | 14/18 (78 %) | 49/233 (21 %) | +57 | < 0.0001 | 13.14 (4.14–41.72) | +55 | **CONFIRMADA** |

H4' queda NO CONFIRMADA por 1 punto por debajo del umbral de 20, aunque p = 0.0036. **No se reinterpreta**: el criterio exige las cuatro condiciones.

**Resultado secundario** (retorno neto desde T_entry2, mediana con señal frente a sin señal; Mann-Whitney):

| | +10 min | +30 min | +60 min |
|---|---|---|---|
| H4' | −12.2 % vs −9.0 % (p 0.10) | −13.9 % vs −10.0 % (p 0.08) | −15.6 % vs −10.2 % (p 0.06) |
| H5 | −12.2 % vs −8.6 % (p 0.024) | −14.3 % vs −8.7 % (p 0.004) | −15.2 % vs −8.9 % (p 0.002) |
| H6' | −36.1 % vs −6.4 % (p < 0.001) | −37.1 % vs −7.6 % (p < 0.001) | −37.5 % vs −7.7 % (p < 0.001) |
| H7' | −13.9 % vs −8.6 % (p 0.015) | −16.8 % vs −8.6 % (p 0.001) | −16.8 % vs −8.6 % (p < 0.001) |
| H8 | −25.5 % vs −9.1 % (p 0.017) | −29.1 % vs −10.2 % (p 0.001) | −29.2 % vs −10.3 % (p < 0.001) |

**Lectura (obligatoria junto al veredicto)**:
- **Parte de la asociación primaria es mecánica.** Pump-y-caída se mide desde el precio **inicial**, no desde el de entrada. H6' (llenado alto) y H8 (compra grande del creador) miden precisamente cuánto ha subido ya el precio antes de T_entry2, así que el token está más cerca del "pico ≥ 2× el inicial" de la definición. Una parte del pump-y-caída ya ha ocurrido cuando se entra. H5 y H7' son menos directamente mecánicas, pero también se asocian a actividad temprana que mueve el precio.
- **La medida accionable es el retorno desde T_entry2.** Ahí la mediana es **negativa en todos los grupos**, con señal y sin ella, en los tres horizontes. Las señales confirmadas marcan tokens **peores para entrar** (retorno mediano más negativo), no mejores. Sirven como filtro de "evitar", no como señal de entrada. Ni siquiera el grupo sin señal tiene retorno mediano positivo (−6 % a −10 %).
- **La mayoría de los tokens muere enseguida.** En 166 de los 250 tokens con retorno evaluable, el retorno a +10 min es idéntico al de +60 min: no hay trades después de los 10 min. Las medianas reflejan sobre todo tokens que dejan de operar.

**POST HOC, NO PRE-REGISTRADO, SIN VEREDICTO** (añadido tras congelar; no decide nada):
- **(a) Con y sin mayhem**: la dirección se mantiene en los 203 no-mayhem en las cinco hipótesis. H4' sin mayhem sale +22 pts, p = 0.0012 (por encima de 20, pero es post hoc y no cambia el veredicto). En los 48 mayhem los grupos son minúsculos o vacíos: H6' y H8 tienen 0 tokens con señal, porque el llenado y la compra del creador se comportan distinto con reservas virtuales dinámicas.
- **(b) Retorno estratificado**:
  - **Los 36 tokens con comisión 0 bps en T_entry2 son todos mayhem** (los otros 12 mayhem tienen 125 bps). El estrato "0 bps" no mide un efecto de comisión: es un subconjunto de mayhem.
  - Retorno mediano: mayhem −16.8 % (0 bps: −56.7 %) frente a −9.9 % a −11.4 % en no-mayhem.
  - En los mayhem, el precio `virtual_quote / virtual_token` cambia fuera de los trades (sección 5.5 / piloto), así que **sus retornos no son fiables** como medida de la curva. En H5 aparecen valores extremos (+1162 %, +2306 %) con n = 1–2.
  - Dentro de no-mayhem y de 125 bps, el patrón de los cinco indicadores se repite (con señal, peor retorno).

**Hipótesis nuevas surgidas de aquí** (sin evaluar; necesitan otra ráfaga u otro día):
- **H9** (filtro de exclusión): los tokens sin ninguna de las señales H5/H6'/H7'/H8 tienen un retorno neto desde T_entry2 mejor que el resto. El pre-registro debe fijar si el objetivo es "mediana > 0" o solo "mejor que el resto".
- **H10** (supervivencia): la fracción de tokens sin trades después de +10 min desde T_entry2 difiere según las señales. Medir la muerte temprana directamente, no a través de la mediana.
- **Resultado desde el precio de entrada**: redefinir pump-y-caída con el pico y el cierre medidos respecto al precio en T_entry2, para quitar el componente mecánico. Es una definición nueva, que habría que pre-registrar antes de aplicarla a otra muestra.
- Tratar los tokens mayhem como población aparte (precio no comparable) en cualquier análisis futuro de retornos.

**Consumo real de RPC (Alchemy)**:
- Total **906 320 CU**: `windows` 740 720 (18 151 `getTransaction` + 367 `getSignaturesForAddress`) y `prices` 165 600 (3 784 + 356), en 22 658 peticiones.
- Es el **57 % de la estimación de 1.6M CU**, el 3 % del free tier mensual de 30M y el 36 % del tope de 2.5M fijado para la ronda.
- Por qué menos de lo estimado: la ventana temprana costó 53.9 `getTransaction` por token frente a los 106.5 del piloto. Los 40 del piloto eran más activos, y las tx fallidas se descartan sin pedirlas. El precio de la hora costó 11.2 por token y los listados de firmas 2.1 por token.
- Tiempo: ~40 min `windows` y ~25 min `prices`. Iba limitado por la latencia de cada petición (secuencial, ~5 req/s efectivas), no por las 25 req/s.
- Incidencias:
  - Alchemy devolvió **429** 8 veces, aun con ~5 req/s efectivas. Uno de ellos, en `getSignaturesForAddress`, cortó la ejecución. Se añadió un reintento con espera creciente para los 429 (`rpc::with_retry`), y un error al listar firmas deja ahora el token pendiente en vez de cortar la ejecución.
  - `rpc_usage.tokens` registraba 0 en una ejecución terminada en error (la fila de 14 677 `getTransaction` procesó 269 tokens). Se corrigió el código y esa fila.

### EXPLORACIÓN (2026-09-29): identidad del operador y persistencia de patrones — sin veredicto

**Exploración, no validación.** Los resultados de los 337 ya se habían visto, así que nada de esta sección decide ni confirma nada. Solo `marxol.db`, sin red.
- **Muestra**: los 377 tokens (40 del piloto + 337 de validación), todos con ventana completa. Operador = campo `creator` del `CreateEvent`, no el payer (98 tokens tienen `user ≠ creator`).
- **Banderas**:
  - H5 y H8 recalculadas para los 377, porque solo dependen del slot de creación.
  - H4' solo existe en los 279 tokens con T_entry2.
  - Pump-y-caída en 1 h, con la definición vigente, para los 377.

**1. Descriptivo**:
- 377 tokens de **272 creators distintos**. 248 creators lanzan 1 solo token; **24 repetidores** (≥ 2 tokens) suman **129 tokens (34 %)** en solo 8 min.
- Distribución de tokens por creator: 1 (248), 2 (8), 3 (4), 4 (3), 5 (2), 6, 7, 8 (2), 10, 16, 24.
- Los 5 mayores suman 66 tokens (18 % de la muestra):
  - `HSF95wu9aG…`: 24 tokens con **el mismo símbolo en 40 s**.
  - `DsT2JZ2mKq…`: 16 tokens, mismo símbolo.
  - `A9emkNzaqK…`: 10.
  - `EjcwXG6ywz…`: 8, todos mayhem.
  - `Dz1JFxDUEW…`: 8 en 16 s, mismo símbolo.
- Todos los repetidores pagan sus propias creaciones (`user = creator`, un solo payer por creator). Los grandes son **fábricas de lanzamientos en serie**, no operadores con proyectos distintos.
- **Mayhem**: 42 de los 129 tokens de repetidores son mayhem, frente a 10 de 248 en creators de un solo token. 10 de los 24 repetidores tienen algún mayhem y 9 lanzan solo mayhem. Ser repetidor y ser mayhem están muy confundidos.

**2. Consistencia dentro de cada operador**:

| Tasa por token | Todos (n) | Creator de 1 token | Tokens de repetidores |
|---|---|---|---|
| H5 | 0.41 (377) | 0.58 (248) | 0.09 (129) |
| H8 | 0.09 (377) | 0.14 (248) | **0.00** (129) |
| H4' | 0.32 (279) | 0.39 (211) | 0.10 (68) |
| Pump-y-caída | 0.20 (377) | 0.25 (248) | 0.10 (129) |

- Los tokens de repetidores llegan a T_entry2 con menos frecuencia (53 % frente a 85 %). Casi todo el efecto viene de las fábricas grandes: `HSF95wu9aG…` llega en 1 de 24. Sin las 5 mayores, llegan el 89 %.
- **Primer token → siguientes** (tokens siguientes agregados):

| Bandera | Primer token = sí → siguientes | Primer token = no → siguientes |
|---|---|---|
| H5 | 2/4 (2 creators) | 7/101 (22 creators) |
| H4' | 2/3 (2 creators) | 2/41 (18 creators) |
| Pump-y-caída | 4/9 (3 creators) | 6/96 (21 creators) |
| H8 | 0 creators | 0/105 (24 creators) |

- **Unanimidad** (todos los tokens de un creator con la misma bandera), frente a lo esperado al barajar las banderas entre los tokens de repetidores (1 000 permutaciones):

| Bandera | Unánimes / creators | Esperado (p5–p95) |
|---|---|---|
| H5 | 16/24 | 15.9 (14–18) |
| H8 | 24/24 | 24.0 (trivial: nadie la tiene) |
| H4' | 17/19 | 13.8 (13–15) |
| Pump-y-caída | 16/24 | 14.9 (13–17) |

- Solo H4' queda por encima del percentil 95 del azar, con pocos casos y muchos n/e.
- Sin las 5 mayores quedan 19 repetidores con 63 tokens: H5 0.14, H8 0.00, H4' 0.07, pump-y-caída 0.17.
- **Lanzamiento previo del mismo creator en la muestra** (dato que se conoce antes de entrar): 105 tokens con un lanzamiento previo, pump-y-caída 10 % y H5 9 %. 272 tokens sin lanzamiento previo, pump-y-caída 24 % y H5 53 %.

**3. ¿Es medible "persisten los patrones por operador"?**
- **Con estos datos, no.** Los repetidores tienen las banderas casi siempre en 0, así que la unanimidad es sobre todo trivial (todo ceros) y cuadra con el azar. Los casos con "primer token = sí" son 2–3 creators: no hay variación que medir.
- Lo único que se ve es un efecto **entre** operadores (repetidor frente a un solo token), no una persistencia **dentro** de cada uno. Y está confundido con mayhem y con las fábricas de un mismo símbolo.
- **n necesario, orden de magnitud**:
  - Comparar la tasa en los tokens siguientes según el primero (del orden del 50 % frente al 7 %, como aquí) con α = 0.05 y potencia 0.8 pide del orden de **15–20 creators con el primer token marcado** por grupo.
  - Con una tasa de H5 del ~10 % en el primer token de un repetidor, eso son **~150–200 repetidores**.
  - En 8 minutos aparecieron 24, pero muchos serían los mismos si se alarga la ventana. Hace falta indexar **horas o días** de creaciones: al ritmo observado (~0.77/s), del orden de 5 000–10 000 creaciones.
  - Seguir después a cada repetidor con `index --address <creator>` sería más barato que recorrer todo el programa.
- **Aviso para la validación ya hecha**: 64 de los 251 tokens de sus tablas son de repetidores (210 creators distintos). Los tokens de un mismo creator no son independientes (seudorreplicación moderada), y Fisher los trata como si lo fueran. No cambia el veredicto congelado, pero hay que controlarlo en la siguiente ráfaga (H13).

**Hipótesis nuevas (sin evaluar; para la segunda ráfaga, con pre-registro previo de umbrales y criterio):**
- **H11** "lanzamiento previo del creator": un token cuyo `creator` ya había creado ≥ 1 token en las N horas **[P]** anteriores a su creación tiene **menor** tasa de pump-y-caída que uno sin lanzamientos previos. Se conoce antes de entrar. Control obligatorio: estratificar por mayhem y por "mismo símbolo que el lanzamiento previo", porque aquí está confundido con fábricas de lanzamientos en serie.
- **H12** "persistencia de dev-sell": si en el token anterior del mismo creator el dev vendió antes de T_entry2, en el siguiente también. Es la única bandera que en la exploración queda por encima del azar en unanimidad (17/19 frente a 13.8 esperado), con n muy pequeño.
- **H13** (control de seudorreplicación, no es hipótesis de señal): repetir la prueba de H5, H6', H7' y H8 en la segunda ráfaga también con **un token por creator** (el primero). Solo se da por buena una señal si mantiene la dirección y el criterio en ese análisis.

### ROBUSTEZ POST HOC de la validación congelada (2026-09-30) — SIN VEREDICTO

**Post hoc, no pre-registrada.** Se hace después de ver los resultados de la validación. No confirma ni descarta nada y no toca los veredictos congelados. Las definiciones se fijaron antes de calcular y están en la sección 10. Solo `marxol.db`, sin red.
- **Cómo se reproduce**, desde la raíz del repo: `cargo build --release && python3 -B scripts/robustez_posthoc.py`. El script llama a `target/release/marxol entry2 --json` y cruza con `creations`, `completions` y `price_points`. Opciones `--marxol` y `--db`. La semilla (20260930) y las definiciones van en la cabecera. Se ejecutó dos veces con la salida idéntica byte a byte (2026-09-30).
- **Comprobaciones**: la población (b) reproduce exactamente las tablas congeladas. H6' es evaluable en los 251. La supervivencia es n/e en 1 token (`HpXRRsBW…`, completa la curva a los 252 s de T_entry2).

**n efectivo**:

| Subconjunto | Tokens | Creators | Bootstrap |
|---|---|---|---|
| (a) un token por creator | 210 | 210 | simple |
| (b) los 251 | 251 | 210 | clúster |
| (c) sin fábricas | 220 | 202 | clúster |
| (d) sin mayhem | 203 | 193 | clúster |

**1. Diferencia en pump-y-caída (puntos, señal − sin señal)**, con el IC bootstrap al 95 % entre corchetes. En ninguna réplica quedó un grupo vacío.

| | (a) 1 por creator | (b) 251 clúster | (c) sin fábricas | (d) sin mayhem |
|---|---|---|---|---|
| H4' | +21.4 [8.5, 35.1], p 0.0014 | +18.7 [5.5, 31.6], p 0.0036 | +20.0 [6.7, 33.7], p 0.0026 | +21.9 [8.9, 35.8], p 0.0012 |
| H5 | +35.0 [24.7, 45.1] | +29.7 [18.0, 40.7] | +33.4 [23.4, 43.8] | +34.8 [24.4, 44.8] |
| H6' | +46.7 [33.2, 59.7] | +45.0 [31.9, 58.5] | +46.2 [32.2, 59.6] | +47.6 [34.2, 60.7] |
| H7' | +34.7 [24.2, 45.6] | +32.5 [22.6, 42.7] | +35.0 [24.6, 45.3] | +35.2 [24.3, 46.3] |
| H8 | +56.4 [35.2, 75.0] | +56.7 [34.2, 76.2] | +56.5 [34.0, 74.3] | +56.7 [33.6, 77.0] |

- H5, H6', H7' y H8 tienen Fisher p < 0.0001 en los cuatro subconjuntos.
- n señal / sin señal: en (a) H4' 69/141, H5 118/92, H6' 65/145, H7' 106/104, H8 18/192. En (d) H4' 69/134, H5 123/80, H6' 65/138, H7' 104/99, H8 18/185.
- La OR sube al quitar la seudorreplicación o los mayhem. En (b) H5 es 5.79 (2.95–11.4). En (a) es 10.18 (4.12–25.2) y en (d) 12.58 (4.32–36.6).
- **Lectura**: la asociación de las cuatro confirmadas no depende de la seudorreplicación por creator, de las fábricas ni de los mayhem. Los IC por clúster excluyen 0 con holgura.
- H4' queda en torno al umbral de 20 puntos: +18.7 a +21.9 según el subconjunto, con el IC excluyendo 0 en todos. **No cambia su veredicto congelado (NO CONFIRMADA).**

**2. Supervivencia y retorno por grupo**:
- **Supervivencia** (≥ 1 trade a más de 10 min de T_entry2), en (b): 38–61 % con señal frente a 24–34 % sin señal. **Las señales se asocian a MÁS supervivencia**, no a menos, en los cuatro subconjuntos. Ejemplos: H5 43 % frente a 24 %; H8 61 % frente a 32 %. Los tokens sin señal suelen morir sin actividad; los que tienen señal siguen operando, pero caen.
- **Retorno neto sin mayhem** (mediana a +30 min en (b), con IC por clúster):

| Señal | Con señal | Sin señal |
|---|---|---|
| H4' | −13.9 % [−23.7, −10.5] | −9.5 % [−11.4, −8.2] |
| H5 | −14.6 % [−24.9, −11.4] | −8.3 % [−9.9, −6.2] |
| H6' | −37.1 % [−41.6, −33.0] | −7.3 % [−8.6, −6.3] |
| H7' | −16.8 % [−25.0, −12.0] | −8.6 % [−10.1, −6.5] |
| H8 | −29.1 % [−48.1, −18.3] | −10.0 % [−11.5, −8.5] |

  - +10 y +60 min van casi iguales a +30.
  - En (a), (c) y (d) las cifras son prácticamente las mismas.
  - **Todos los IC están por debajo de 0**, también en el grupo sin señal.

**3. Métricas de filtro "evitar"** (sobre los 251; "malo" = pump-y-caída). Precisión, cobertura y sacrificio se dan con todos / sin mayhem. El retorno del resto es la mediana a +30 de los no evitados, sin mayhem, con IC por clúster.

| Filtro | Evitados / no evitados | Precisión | Cobertura | Sacrificio | Retorno del resto (n) |
|---|---|---|---|---|---|
| A (cualquiera de H5/H6'/H7'/H8) | 154 / 97 | 38 / 38 % | 92 / 98 % | 51 / 56 % | −6.9 % [−8.6, −4.8] (67) |
| B (≥ 2 de las cuatro) | 112 / 139 | 45 / 45 % | 79 / 93 % | 33 / 41 % | −7.8 % [−9.0, −6.3] (93) |
| C (H5 o H7') | 150 / 101 | 39 / 39 % | 92 / 98 % | 49 / 53 % | −7.6 % [−8.9, −5.6] (71) |
| H5 | 125 / 126 | 40 / 40 % | 79 / 93 % | 40 / 49 % | −8.3 % [−9.9, −6.2] (80) |
| H6' | 65 / 186 | 58 / 58 % | 60 / 72 % | 14 / 18 % | −7.3 % [−8.6, −6.3] (137) |
| H7' | 122 / 129 | 42 / 43 % | 81 / 85 % | 38 / 39 % | −8.6 % [−10.1, −6.5] (99) |
| H8 | 18 / 233 | 78 / 78 % | 22 / 26 % | 2 / 3 % | −10.0 % [−11.5, −8.5] (184) |
| H4' | 73 / 178 | 38 / 41 % | 44 / 53 % | 24 / 27 % | −9.5 % [−11.4, −8.2] (133) |
| Sin filtrar | — | — | — | — | −10.7 % (202) |

- **"Malo" secundario (retorno a +30 < 0, sin mayhem) casi degenera**: 196 de los 202 evaluables son "malos" y solo **6 son "buenos"** (+0.1 %, +2.6 %, +7.8 %, +23 %, +77 %, +81 %). La precisión sale 95–100 % para cualquier filtro, y el sacrificio (5 de 6 buenos en A, B, C, H5 y H7'; 0 en H6' y H8) no se puede interpretar con n = 6.

**Lectura (post hoc, sin veredicto)**:
- Las cuatro señales confirmadas **sobreviven** a un token por creator, a quitar fábricas y a quitar mayhem, con IC por clúster lejos de 0. La seudorreplicación que avisó la exploración de operadores no explica el resultado.
- **Como filtro de entrada no bastan.** Ningún filtro deja un resto con retorno mediano positivo. El mejor (A) sube la mediana de −10.7 % a −6.9 %, con el IC entero por debajo de 0, a costa de descartar el 56 % de los tokens "buenos" (pump-y-caída = no, sin mayhem). La mediana de los no evitados sigue siendo negativa, porque la comisión y la inactividad ya la hunden. Las señales mejoran el "evitar", no generan un "entrar".
- **H6' y H8 son las más selectivas** (sacrificio 14–18 % y 2–3 %), pero siguen siendo en parte mecánicas (sección 8, lectura de la validación).
- La supervivencia va **al revés de lo que suponía H10**: los tokens con señal siguen operando más, pero pierden más. Si se pre-registra H10, hay que fijar la dirección con esto en cuenta. Esa dirección es post hoc, así que solo se puede comprobar con otra ráfaga.

### EXPLORACIÓN POST HOC (2026-09-30): curva de retorno desde T_entry2 y máxima subida — SIN VEREDICTO

**Post hoc, solo descripción.** No propone horizontes ni umbrales. Aplica las reglas de la robustez post hoc: los 251 tokens de validación, métricas de retorno **solo sin mayhem** (203 tokens) y retorno neto calculado como en la validación congelada.
- **Reproducir**: `cargo build --release && python3 -B scripts/curva_retorno_posthoc.py` (sin red).
- **Control**: los retornos a +10, +30 y +60 min coinciden exactamente con los de `marxol entry2 --json` en los 203 tokens (0 discrepancias).
- **Limitación**: el precio de salida es el último punto con `timestamp ≤ salida`, ordenado por `(timestamp, slot)` como en `entry2::returns`. Si en ese segundo y slot hay varios trades, el orden interno no está determinado. Pasa en 52 tokens a +5 s, 44 a +15 s, 42 a +30 s, 30 a +1 min, 22 a +2 min, 32 a +5 min, 23 a +10 min, 16 a +30 min y 15 a +60 min. **Los horizontes cortos son los más afectados.**

**1. Retorno neto mediano y fracción con retorno > 0 según el horizonte** (sin mayhem):

| Horizonte | Todos (n = 203) | H5 sí / no | H6' sí / no | H7' sí / no | H8 sí / no |
|---|---|---|---|---|---|
| +5 s | −3.0 % · 15 % | −2.7 · 21 % / −3.1 · 5 % | −3.8 · 26 % / −2.7 · 9 % | −2.6 · 23 % / −3.3 · 6 % | −2.8 · 28 % / −3.0 · 14 % |
| +15 s | −4.3 % · 17 % | −4.3 · 23 % / −3.9 · 8 % | −13.7 · 25 % / −3.1 · 13 % | −3.9 · 26 % / −4.3 · 7 % | −4.7 · 28 % / −4.1 · 16 % |
| +30 s | −5.3 % · 16 % | −5.7 · 22 % / −4.4 · 6 % | −22.3 · 22 % / −3.6 · 13 % | −5.5 · 23 % / −4.6 · 8 % | −5.1 · 22 % / −5.3 · 15 % |
| +1 min | −6.2 % · 14 % | −7.2 · 20 % / −4.5 · 5 % | −26.5 · 17 % / −4.4 · 13 % | −7.3 · 21 % / −5.3 · 7 % | −6.5 · 17 % / −6.2 · 14 % |
| +2 min | −7.6 % · 11 % | −9.5 · 15 % / −5.7 · 5 % | −30.0 · 12 % / −5.7 · 10 % | −9.2 · 16 % / −6.3 · 5 % | −9.1 · 17 % / −7.6 · 10 % |
| +5 min | −8.9 % · 7 % | −11.4 · 10 % / −7.3 · 4 % | −33.3 · 2 % / −5.8 · 10 % | −11.6 · 13 % / −7.8 · 2 % | −13.1 · 0 % / −8.6 · 8 % |
| +10 min | −9.9 % · 4 % | −12.2 · 7 % / −8.0 · 1 % | −36.1 · 0 % / −6.2 · 7 % | −13.3 · 8 % / −8.5 · 1 % | −25.5 · 0 % / −9.0 · 5 % |
| +30 min | −10.7 % · 3 % | −14.6 · 4 % / −8.3 · 1 % | −37.1 · 0 % / −7.3 · 4 % | −16.8 · 5 % / −8.6 · 1 % | −29.1 · 0 % / −10.0 · 3 % |
| +60 min | −11.4 % · 2 % | −16.4 · 3 % / −8.6 · 0 % | −37.5 · 0 % / −7.6 · 3 % | −18.3 · 4 % / −8.6 · 0 % | −29.2 · 0 % / −10.2 · 2 % |

- Cada celda da la mediana y la fracción de tokens con retorno > 0.
- n por grupo (sí / no): H5 123/80, H6' 65/138, H7' 104/99, H8 18/185. A +30 y +60 min se pierde 1 token (curva completada).
- La mediana es negativa en todos los horizontes y grupos, y se hace más negativa con el tiempo. A +5 s ya es −3 %, del orden de la comisión de ida y vuelta (125 bps × 2 ≈ 2.5 %).
- **En horizontes cortos la fracción con retorno > 0 es MAYOR con señal que sin ella** en H5, H6', H7' y H8, al revés que la mediana. Ejemplo: H5 a +5 s, 21 % frente a 5 %. Los tokens con señal tienen más dispersión, con subidas y caídas, mientras que los que no tienen señal se quedan quietos y pierden la comisión. Con señal, esa fracción cae a ≤ 5 % en +30 min.

**2. Máxima subida alcanzable tras T_entry2** (hasta +60 min, sin mayhem; 202 evaluables, n/e `5wXHg8Ed…` por completar la curva). Es el precio máximo de la curva en el slot de T_entry2 o después, frente al precio de entrada. Es una **cota a posteriori**: supone vender justo en el máximo, sin impacto de precio.

| | n | Múltiplo bruto p25 / p50 / p75 / p90 (máx) | Neto en el máximo p50 / p75 / p90 | Neto > 0 | s hasta el máximo p50 / p75 / p90 | Máx. en el slot de T_entry2 |
|---|---|---|---|---|---|---|
| Todos | 202 | 0.974 / 1.001 / 1.118 / 1.629 (3.63) | −2.4 / +8.7 / +58.8 % | 69 (34 %) | 1 / 31 / 159 | 94 (47 %) |
| H5 sí / no | 122 / 80 | 1.013 / 0.997 (p50) | −1.2 / −3.0 % (p50) | 47 % / 15 % | 3 / 0 (p50) | 48 / 46 |
| H6' sí / no | 65 / 137 | 1.054 / 1.000 | +2.7 / −2.5 % | 57 % / 23 % | 5 / 0 | 19 / 75 |
| H7' sí / no | 103 / 99 | 1.027 / 0.999 | +0.2 / −2.8 % | 50 % / 17 % | 5 / 0 | 36 / 58 |
| H8 sí / no | 18 / 184 | 1.001 / 1.000 | −2.4 / −2.5 % | 50 % / 33 % | 2 / 1 | 8 / 86 |

- **Cuándo ocurre el máximo** (todos): 0 s 99, 1–5 s 17, 6–15 s 20, 16–30 s 12, 31–60 s 12, 1–2 min 14, 2–5 min 13, 5–10 min 7, 10–30 min 8, 30–60 min 0.
- **La mitad de los máximos está en el mismo segundo que T_entry2** (94 en su propio slot). Muchas veces es el precio justo después del propio trade de entrada. Un observador que reacciona a ese trade difícilmente lo captura, así que la cota es optimista.
- En al menos el 25 % de los tokens el máximo posterior queda **por debajo** del precio de entrada: el precio no vuelve a subir nunca.
- **Los tokens con señal tienen más recorrido al alza** (p90 del múltiplo bruto de 1.87–1.97× con señal, frente a 1.16–1.25× en H5, H6' y H7' sin señal), además de peor mediana final. Es coherente con más volatilidad, no con mejor entrada. Es descriptivo: la cota se calcula a posteriori y no dice cómo capturarla.

### Validación 2 (réplica) — CONGELADO antes de descargar (2026-09-30 15:17 UTC)

Fijado **antes de indexar ni descargar ningún token del tramo**. Se evalúa **una sola vez**. Cualquier cambio posterior es una hipótesis nueva que exige tokens nuevos. Los puntos marcados "(confirmado por Roi)" se resolvieron al pre-registrar, porque el texto original era ambiguo.

**Muestra**:
- **S2** = el primer slot con `blockTime ≥ 1790730000` (2026-09-30 01:00:00 UTC), según `getSignaturesForAddress` de la PDA `mint-authority` (`TSLvdd1p…`). Se resuelve al descargar y se anota aquí con su número (confirmado por Roi).
- **Tramo "validación 2"**: las primeras **400 creaciones** exitosas (con `CreateEvent`) con slot ≥ S2, ordenadas por `(slot, mint lexicográfico)` (confirmado por Roi). Es otro día y otra franja horaria que la validación 1 (2026-09-28, 14:03–14:11 UTC).
- Quedan fuera el piloto (40) y la validación 1 (337).
- Si la indexación hacia atrás no llega a S2, o no hay 400 creaciones con slot ≥ S2: **parar y preguntar**.
- **Coste**: estimación ~1.1M CU; **tope de la ejecución 2M CU**. Al alcanzarlo, la ejecución se detiene.
  - Nota de ejecución, no forma parte del criterio: el `index` actual pide `getTransaction` de cada firma que recorre. Llegar a S2 desde la hora actual costaría del orden de 1.6M CU (unos 39 000 tokens creados desde las 01:00). Hay que recorrer solo las listas de firmas hasta S2 y pedir la transacción únicamente de las 400.
  - Descargar tarde encarece las ventanas de los tokens que siguieron activos (sección 10), así que la estimación de 1.1M es optimista.

**Definiciones sin cambios**:
- T_entry2 y H4', H5, H6', H7', H8 con sus umbrales, incluido 13.65 % en H6' ("Validación de H4', H5, H6', H7', H8 — CONGELADO").
- Pump-y-caída en 1 h (definición vigente).
- Supervivencia: ≥ 1 trade a más de 10 min de T_entry2; n/e si la curva se completa antes de T_entry2 + 10 min (sección 10).
- Los tokens mayhem forman población aparte en todo lo que sea retorno.

**Poblaciones**:
- **Primaria** (decide los veredictos): tokens con T_entry2, **un token por creator** (`CreateEvent.creator`), mayhem incluidos. Se queda el token de menor slot. Si hay empate, el de menor orden de ejecución; si no está disponible, el mint en orden lexicográfico.
- **Secundaria 1**: todos los tokens con T_entry2.
- **Secundaria 2**: todos los tokens con T_entry2 sin mayhem.

**Criterio de CONFIRMACIÓN EN RÉPLICA**, por separado para H4', H5, H6', H7' y H8, con pump-y-caída en 1 h como resultado. **CONFIRMADA EN RÉPLICA** si se cumplen todas:
1. En la población primaria:
   - ≥ 3 tokens en cada grupo;
   - p₁ − p₀ ≥ 20 puntos;
   - Fisher bilateral p < 0.01;
   - p₁ − p₀ > 0 también sin el **token de más trades**: el que tiene más trades en su ventana de 5 min entre los de la población primaria (definición ciega al resultado).
2. p₁ − p₀ > 0 en la secundaria 1 y en la secundaria 2.

Si no, **NO CONFIRMADA EN RÉPLICA**, indicando si la dirección es la opuesta. La odds ratio y su IC se reportan, pero no deciden.

**H10** (secundaria): los tokens con señal tienen **más** supervivencia que los que no la tienen, para cada una de H5, H6', H7' y H8. Se aplica **el mismo criterio de réplica completo**, con la supervivencia como resultado y los n/e excluidos: ≥ 20 puntos, Fisher p < 0.01, ≥ 3 por grupo y misma dirección sin el token de más trades, todo en la primaria, más la misma dirección en las dos secundarias (confirmado por Roi). Como referencia post hoc de la validación 1 (sección 8, robustez): las diferencias fueron +19, +15, +19 y +30 puntos.

**No entran en esta ronda**: H11 y H12. H9 no se pre-registra como "mediana > 0": queda descriptiva.

**Condiciones de parada** (informar y detenerse, sin cambiar definiciones ni umbrales):
- **Algún grupo con < 3 tokens** en alguna hipótesis (H4'–H8 o H10), en cualquiera de las tres poblaciones (confirmado por Roi).

**Baja potencia**: si hay menos de **250 tokens con T_entry2** (secundaria 1), la evaluación **no se detiene**: se hace igual, y todo veredicto NO CONFIRMADA EN RÉPLICA se etiqueta **"baja potencia"**. Con esa etiqueta, el resultado no se interpreta como descarte.

**Ejecución, fase 1: indexado del tramo (2026-09-30, `marxol index-tramo --from 1790730000 --count 400 --provider alchemy --max-rps 10 --max-requests 3000`)**:
- Solo se recorrieron las listas de firmas de `mint-authority` hasta S2: 29 páginas, 28 545 firmas con `blockTime ≥ 1790730000`. Las 1 629 fallidas se descartaron por `err`.
- Se pidió `getTransaction` solo de las 401 firmas exitosas necesarias; 1 no trae `CreateEvent`.
- **S2 = slot 451 810 013** (blockTime 1790730000 = 01:00:00 UTC exacto). La última firma anterior está en el slot 451 810 008 (blockTime 1790729999).
- **Creación n.º 400** en orden `(slot, mint)`: **slot 451 811 800**, mint `H6V4cjHgJ1j8rUbBzfVceevkx7aggaJTyBwyM5LZpump`, timestamp 1790730478. El slot frontera tiene 1 sola creación, así que no hizo falta desempatar, y no se indexó ninguna creación fuera del tramo.
- **Tramo validación 2 = creaciones con slot en [451 810 013, 451 811 800]**: exactamente 400, entre las 01:00:00 y las 01:07:58 UTC.
- **Consumo: 17 440 CU** (401 `getTransaction` + 35 `getSignaturesForAddress`, 6 de ellas reintentos). Es el 0.9 % del tope de 2M CU.
- Aún no se ha descargado ninguna ventana ni precio del tramo, y no se ha mirado ningún resultado.

**Ejecución, fase 2: descargas y chequeo (2026-09-30, sin mirar el resultado)**:
- `windows --limit 400 --provider alchemy --max-rps 10 --max-requests 40000`: **400 ventanas completas, 0 incompletas**, 0 errores ni 429. Consumo: 27 315 `getTransaction` + 518 `getSignaturesForAddress` = **1 113 320 CU**, en ~60 min.
- `prices --limit 400 … --max-requests 21731`: **400 ventanas de precio completas (65 min), 0 incompletas**, 0 errores. Consumo: 5 174 + 422 = **223 840 CU**, en ~25 min.
- Los 18 667 trades de la ventana temprana tienen su punto de precio, y las 400 creaciones tienen precio inicial.
- **Total de la validación 2: 1 354 600 CU** (68 % del tope de 2M). La estimación era de ~1.1M: salió un 23 % por encima, por descargar tarde.
- `entry2 --tramo validacion2` (slots [451 810 013, 451 811 800]) solo admite `--check` hasta que se implemente el veredicto de réplica. Tras la ingesta, la validación 1 sigue idéntica byte a byte (las mismas cinco salidas de la sección 11).
- **`entry2 --check --tramo validacion2`** (sin resultado):
  - 283 tokens con T_entry2 y 117 sin él. Como 283 ≥ 250, **no aplica la etiqueta de baja potencia**.
  - Comisión conocida en 283/283. Orden de ejecución completo en 282/283: la excepción es `DWJzFDAx…` (543 trades en la ventana, T_entry2 a 10 s con 16 trades antes). Se anota; no cambia ninguna definición.
  - Grupos señal / sin señal:

| Población | n (creators) | H4' | H5 | H6' | H7' | H8 |
|---|---|---|---|---|---|---|
| Primaria (1 por creator) | 197 (197) | 59 / 138 | 119 / 78 | 56 / 141 | 102 / 95 | 15 / 182 |
| Secundaria 1 (todos) | 283 (197) | 87 / 196 | 143 / 140 | 69 / 214 | 129 / 154 | 25 / 258 |
| Secundaria 2 (sin mayhem) | 220 (174) | 70 / 150 | 143 / 77 | 67 / 153 | 117 / 103 | 23 / 197 |

  - **Ningún grupo < 3**, así que no hay condición de parada en H4'–H8.
  - En H10 los grupos son los mismos menos los n/e de supervivencia. Esos n/e solo se ven al calcular, y la condición de parada se vuelve a comprobar entonces.
  - H6' es evaluable en los 283 tokens.
  - Medianas en T_entry2: 13 s, 6 trades antes y múltiplo de precio 1.067×.

**Revisión 2026-09-30 15:48 UTC, hecha antes de indexar ningún token del tramo** (0 creaciones con `timestamp ≥ 1790730000` en `marxol.db`): la condición "menos de 250 tokens con T_entry2" pasó de condición de parada a la etiqueta de baja potencia de arriba. Motivo (Roi): parar después de descargar desperdicia la cuota y no protege de nada. No se cambió nada más.

**Resultados descriptivos (sin veredicto)**, solo sin mayhem:
- **Horizontes**: +5 s, +30 s, +5 min, +30 min y +60 min desde T_entry2.
- **Retorno neto en dos versiones**:
  - **(a)** La definición congelada de la validación 1 (`entry2::returns`): último punto con `timestamp ≤ t_T_entry2 + h`, ordenado por `(timestamp, slot)`.
  - **(b)** Igual que (a), pero el precio de salida es el del **último trade en orden de ejecución** (reconstruido encadenando reservas) con `timestamp ≤ t_T_entry2 + h`.
    - Mismo precio de entrada, misma comisión y mismas reglas de n/e que (a).
    - Si el slot del punto de salida no se puede reconstruir, ese token queda n/e en ese horizonte para (b) (confirmado por Roi).
    - Se reporta cuántos slots y cuántos tokens por horizonte no se pudieron reconstruir.
- **Por horizonte y por grupo señal / sin señal** de H4', H5, H6', H7' y H8, y para todos: mediana del retorno neto (a) y (b), y fracción con retorno neto > 0.
- **Métricas de filtro A, B y C** (definiciones de la sección 10) en **las tres poblaciones** (confirmado por Roi):
  - Precisión, cobertura, sacrificio y número de tokens no evitados, con "malo" principal = pump-y-caída.
  - "Malo" secundario = retorno neto a +30 min < 0, solo sin mayhem.
  - Retorno del resto = mediana del retorno neto a +30 min de los no evitados, sin mayhem, en las versiones (a) y (b).
  - Intervalo bootstrap: 2 000 réplicas, semilla 20260930, percentiles 2.5–97.5. Remuestreo simple en la primaria y por clúster de creator en las secundarias.

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
   **Validación congelada (2026-09-29 15:24 UTC)**: ejecutada el mismo día con Alchemy (`windows` → `prices` → `entry2 --check` → `entry2` una vez). Resultado y consumo (906 320 CU) en la sección 8: H5, H6', H7' y H8 confirmadas; H4' no confirmada (+19 pts). Todas marcan tokens **peores** para entrar (retorno neto mediano negativo en todos los grupos). Solo vale para la ráfaga de 8 min del 2026-09-28.
   **Siguiente**: repetir la validación en otra ráfaga u otro día antes de dar ningún indicador por general, y pre-registrar H9/H10 y el resultado medido desde el precio de entrada (sección 8). Pre-registrar también H11 (lanzamiento previo del creator, con N horas y estratificado por mayhem y mismo símbolo), H12 (persistencia de dev-sell entre tokens del mismo creator) y H13 (repetir H5/H6'/H7'/H8 con un token por creator como control de seudorreplicación), surgidas de la exploración de operadores (sección 8).
   **Comprobación de robustez de la validación congelada — POST HOC, SIN VEREDICTO, EJECUTADA 2026-09-30** (definiciones fijadas antes de calcular; resultado en la sección 8, "ROBUSTEZ POST HOC"). Solo `marxol.db`, sin red. No cambia ningún veredicto congelado ni ninguna definición previa.
   - **Población base**: los 251 tokens de validación con T_entry2 (los de las tablas congeladas). Quedan fuera los 40 del piloto y los 86 sin T_entry2.
   - **Un token por creator**: el de menor slot de creación del creator dentro de los 251. Empate: menor orden de ejecución dentro del slot; si no está disponible, mint en orden lexicográfico. En la práctica no hay empates entre los 210 creators.
   - **Fábrica**: creator con ≥ 5 tokens entre las 377 creaciones indexadas (9 creators, 31 tokens de los 251).
   - **Supervivencia** (definición de H10): ≥ 1 trade a más de 10 min de T_entry2. **Decisión confirmada por Roi**: si la curva se completa antes de T_entry2 + 10 min, n/e (1 token), la misma regla que para el retorno.
   - **Retorno neto**: la definición de la validación congelada. Medianas a +10, +30 y +60 min; solo +30 lleva intervalo.
   - **Mayhem**: las métricas de retorno se reportan solo sin mayhem. Las de pump-y-caída y supervivencia, con todos y sin mayhem.
   - **"Malo" principal** = pump-y-caída (definición congelada). **"Malo" secundario** = retorno neto a +30 min < 0. "Bueno" = el complementario en cada caso.
   - **Filtros "evitar"**:
     - A: señal en cualquiera de H5, H6', H7', H8.
     - B: señal en ≥ 2 de las cuatro.
     - C: señal en H5 o H7' (las dos no mecánicas).
     - Además, cada señal por separado, con H4' aparte.
   - **Métricas del filtro**:
     - Precisión: fracción de "malos" entre los evitados.
     - Cobertura: fracción de los "malos" que se evita.
     - Sacrificio: fracción de los "buenos" que se evita.
     - Retorno del resto: mediana del retorno a +30 entre los no evitados, sin mayhem, con IC bootstrap por clúster.
     - Número de tokens no evitados.
   - **Subconjuntos de las tablas 2×2** (sin combinarlos): (a) un token por creator; (b) los 251; (c) los 251 sin fábricas; (d) los 251 sin mayhem.
   - **Bootstrap**: 2 000 réplicas, semilla 20260930, percentiles 2.5–97.5.
     - En (a), remuestreo simple de tokens.
     - En (b), (c) y (d), remuestreo de creators con reemplazo, con todos sus tokens (clúster). **Decisión confirmada por Roi**: (c) y (d) también llevan intervalo.
     - Una réplica con algún grupo vacío se descarta para esa estadística, y se reporta cuántas.
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
- `scripts/` (Python, solo biblioteca estándar; análisis **post hoc**, sin veredicto):
  - `robustez_posthoc.py`: robustez de la validación congelada.
  - `curva_retorno_posthoc.py`: curva de retorno y máxima subida desde T_entry2.
  - Los dos leen `target/release/marxol entry2 --json` y `marxol.db`. Se ejecutan desde la raíz del repo con `python3 -B`.

**Hallazgos operativos (mainnet, 2026-09-28)**:
- **Transacciones versión 1 en mainnet**: `getTransaction` con `maxSupportedTransactionVersion: 0` falla (error -32015) con parte del tráfico de pump. Hay que pedir `1`. `solana-transaction-status-client-types 4.3` las decodifica bien en encoding `json`.
- **~68 % de las firmas del programa pump son tx fallidas** (203 de 300 en una muestra). `getSignaturesForAddress` ya trae `err`, así que el indexador las descarta sin gastar un `getTransaction` (40 CU en Alchemy) en cada una. Es un ahorro grande de cuota que hay que tener en cuenta en el benchmark del paso 4.
- En una muestra de 97 tx exitosas del programa: 57 `TradeEvent`, 22 `DistributeFeeToHoldersEvent`, 3 `CollectCreatorFeeEvent`, 1 `CreateEvent`. Las creaciones son una fracción pequeña del tráfico: recorrer las firmas del programa entero para encontrarlas es caro. Para seguir a un operador concreto es mucho más barato `index --address <creator>`.
- **Vía barata para listar creaciones**: la PDA `mint-authority` (`TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM`, seeds `["mint-authority"]`, la imprime `marxol verify`) solo la tocan `create`/`create_v2`. En 400 firmas suyas: 22 tx fallidas y 378 exitosas, de las que 377 traen `CreateEvent` (la restante no está examinada). `index --address TSLvdd1p…` indexa creaciones sin recorrer el tráfico de trades del programa (~1 creación por cada 100 tx).
- **Ritmo de creación observado**: 377 creaciones en 492 s (2026-09-28, 14:03–14:11 UTC) ≈ 0.77/s. Es una muestra de 8 minutos, no una media diaria.
- **Orden de ejecución dentro de un slot (2026-09-29)**: ni `getTransaction` ni el orden `(slot, timestamp, firma)` lo dan (la firma es un hash). Se reconstruye exacto encadenando reservas: `virtual_token_reserves` antes de un trade = después + `token_amount` (compra) o − `token_amount` (venta), y coincide con el después del trade anterior, empezando por el `CreateEvent`. Encadena 2 831 de 2 831 trades de la ventana temprana del piloto. Hace falta en todo lo que dependa del orden dentro del slot (primer trade que cruza un umbral, precio de cierre). H1 y H1c no dependen de él (ver sección 8). Ignorarlo produjo un falso positivo en H5 (p = 0.026 → 0.42 tras corregir).
- **Tokens mayhem y comisiones (2026-09-29, validación)**: los 36 tokens con `fee_basis_points + creator_fee_basis_points = 0` en T_entry2 son todos mayhem (48 mayhem en total entre 251). En no-mayhem la comisión más común es 125 bps (169 tokens), con valores de 96 a 395 bps. Nunca suponer la comisión de `Global` (95 + 5): leerla del `TradeEvent`.
- **Rate limit de Alchemy**: aparecen 429 esporádicos (8 en ~22 700 peticiones) incluso a ~5 req/s efectivas. Hay que reintentar siempre.
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
