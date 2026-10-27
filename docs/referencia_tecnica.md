# Referencia técnica: pump.fun, identidad del creator, IDL, RPC y hallazgos de mainnet

Movido **literalmente** desde `CLAUDE.md` el 2026-10-21 (antiguas secciones 3–7 y 9, hallazgos operativos de la 11 y fuentes). Se lee bajo demanda.

Las referencias a "sección N" usan la numeración de `CLAUDE.md` anterior al 2026-10-21: secciones 3–7 y 9 → docs/referencia_tecnica.md; 8 → docs/hipotesis_y_resultados.md, docs/validaciones_congeladas.md y docs/exploracion_operadores.md; 10 → docs/estado_y_pasos.md; 11 → `CLAUDE.md`. El detalle de líneas cerradas está en docs/historico_lineas_cerradas.md.

---

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

**Observado en mainnet (2026-10-09, 1 tx, `SellV3`)**: el programa emite el evento **por las dos vías a la vez**. La línea `Program data:` del log trae el mismo payload, byte a byte, que la inner instruction de `emit_cpi` (`TradeEvent`, 383 bytes, discriminador `bddb7fd34ee661ee`). Por eso un grabador sobre `logsSubscribe` puede decodificar sin `getTransaction`, salvo cuando el log se trunca (10 KB). Se ha comprobado en una sola tx: hay que verificarlo en volumen antes de depender de ello (`docs/diseno_record.md`).

### 5.4 Token program: Token-2022, no SPL clásico

El mint se crea con **Token-2022** (`TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb`), no el SPL Token Program clásico. Afecta a cómo se consultan balances y metadata — las librerías `spl-token` clásicas no sirven tal cual, hace falta `spl-token-2022`.

El mint authority real durante la fase bonding curve es una PDA del programa (`mint-authority`), no una wallet de usuario — coherente con que "creador" es un concepto de negocio (campo en la bonding curve), no la autoridad criptográfica del mint.

### 5.5 Quote mints múltiples (SOL y USDC)

Desde `create_v2`, un token puede crearse con quote mint SOL (por defecto, `quote_mint = Pubkey::default()` o wSOL `So11111111111111111111111111111111111111112`) o USDC. **Observado en mainnet**: los tokens SOL traen `quote_mint = 11111111111111111111111111111111` (`Pubkey::default()`), no wSOL. El único quote whitelisteado en `Global.whitelisted_quote_mints` es USDC (`EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v`). El campo `quote_mint` en `BondingCurve`, `CreateEvent` y `TradeEvent` indica cuál. El indexador debe soportar ambos desde el esquema base, no asumir SOL fijo.

**Corrección (2026-09-28, 377 creaciones consecutivas de mainnet)**: hay **muchos más quote mints que SOL y USDC**. 12 distintos en la muestra: SOL 255 (68 %), `N7Q5fYX7YRnDQksfdBKnoUb3awm92n7QNAD35X3Rq1X` 95 (25 %), USDC 11, y otros 9 con 1–4 tokens cada uno (entre ellos `pumpCmXq…`, `SPCXxcqX…`, `Xsc9qvGR…`). La lista `whitelisted_quote_mints` de `Global` no es la lista completa de quotes posibles; probablemente entran por el mecanismo `QuoteControl` (`AddQuoteControlMintEvent`), **no verificado**. Qué es `N7Q5fYX7…` está sin identificar. Consecuencia: nunca comparar importes o mcap en valor absoluto entre tokens de quotes distintos sin convertirlos; las métricas relativas (CV, fracción del volumen, ratios de mcap) sí son comparables.

**Campos de importe del `TradeEvent`** (observado): con quote SOL, `sol_amount == quote_amount` y `virtual_sol_reserves == virtual_quote_reserves`; con otro quote, `sol_amount = 0` y `virtual_sol_reserves = 0`, y el importe va solo en `quote_amount`/`virtual_quote_reserves`. Usar siempre los campos `quote_*`.

---

## 6. Eventos del IDL — referencia de campos (confirmado directamente contra `idl/pump.json`)

**IDL vigente (actualizado 2026-10-21)**:
- Fuente: IDL Anchor oficial de `pump-fun/pump-public-docs` (`idl/pump.json`), versión del commit del **2026-10-13** ("depth 2 pump coin quotes"). Tras el del 2026-10-07 ("v3 / v2 trades, multi-hop swap, pump coins as quote mints, fee sweeps") y el del 2026-10-08.
- Copia versionada en `idl/pump.json` (sha256 `7616681e…88d980997207`): **56 instrucciones, 7 cuentas** (`BondingCurve, FeeConfig, Global, GlobalVolumeAccumulator, QuoteControl, SharingConfig, UserVolumeAccumulator`), 32 eventos y 47 tipos. Detalle del cambio: `resultados/idl_actualizacion_2026-10-21.txt`.
- **La cuenta IDL on-chain no sirve como fuente**: `AYgC53tU5BbP2NAnv5nConJxAdpQZctvmZK88pu69xRs` (Anchor, `create_with_seed(find_program_address([], programa), "anchor:idl", programa)`) sigue con el IDL del 2026-09-28 y no se actualiza con los despliegues.
- **Despliegues**: ProgramData `B5MvUwXdiW1NMM6QFFD3ssPKBujD4zMohncbM73Z2BQu`.
  - 2026-09-30 21:13 UTC, slot 452 082 198 (entre V2 y V3).
  - 2026-10-01 07:45 UTC, slot 452 224 060 (75 min antes de V3), y otros 6 entre las 15:55 y las 22:11 de ese día.
  - 2026-10-02, 10-08, 10-13 y 10-18 (varios).
  - **Último: 2026-10-19 17:47 UTC, slot 458 921 440.**
  - V1–V3 no tienen ningún evento desconocido (todas sus ventanas están completas, con 0 errores).
- **Verificación contra chain (2026-10-21)**:
  - `Global` decodifica con 30 campos, 0 faltantes y **0 bytes sobrantes**. Con el IDL anterior: 29 campos y 1 sobrante (`max_curve_depth`).
  - Una `BondingCurve` viva decodifica sus 19 campos.
  - Los `SweepBondingCurveFeeEvent` reales decodifican sin errores.
  - **Excepción conocida**: el `TradeEvent` real lleva **8 bytes más** de lo que describe este IDL (16 más que el anterior). Es un campo añadido por el despliegue del 18/19-10 que aún no está en el IDL público. Se ignora y se avisa (ver `TradeEvent`).
- **Regla: revisar el IDL tras cada despliegue en ProgramData.**
  - Comparar `idl/pump.json` con el de GitHub.
  - Ejecutar `marxol verify`, que desde el 2026-10-22 **falla** si `Global` tiene bytes sobrantes o le faltan campos.
  - Decodificar tx recientes con `marxol tx`. Son señales de desfase: un "evento con discriminador desconocido", el aviso de `TradeEvent` con bytes de más o un fallo de `verify`.
  - Comprobar byte a byte que las salidas de V1–V3 no cambian (`cargo test` incluye fixtures de V1–V3 decodificados con los dos IDL).
- Historial: la copia anterior (sha256 `ffe966c4…c8b56064b`, del 2026-09-28) tenía 47 instrucciones, 28 eventos y 42 tipos. Se conserva en `tests/fixtures/pump_idl_2026-09-28.json`.

### `CreateEvent`
```
name, symbol, uri, mint, bonding_curve, user, creator, timestamp,
virtual_token_reserves, virtual_sol_reserves, real_token_reserves,
token_total_supply, token_program, is_mayhem_mode, is_cashback_enabled,
quote_mint, virtual_quote_reserves, creator_fee_bps, is_holder_reward,
depth (u8, nuevo 2026-10: profundidad de la cadena de quotes cuando el quote es otro pump coin)
```

### `TradeEvent` (35 campos en el IDL vigente — el más rico; ya trae `creator` embebido, no hace falta cruzar con `CreateEvent` por `mint`)
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
holder_rewards_bps, holder_rewards,
creator_fee_unclaimed (u64, nuevo 2026-10)
+ quote_transfer_fee (u64, desde el despliegue del 2026-10-19 15:46:16 UTC, slot 458 888 140; está en el IDL de @pump-fun/pump-sdk 4.2.0, no en el de GitHub ni en idl/pump.json, así que marXol lo trata como 8 bytes sobrantes)
```
- `ix_name` **no distingue la versión** de la instrucción. En mainnet (2026-10-21), un `buy_v2` emite `ix_name = "buy"` y un `sell_v2` emite `"sell"`. La instrucción concreta se ve en `TxEvents.instructions`.
- Con el IDL vigente, el `TradeEvent` real mide 382–383 bytes (según la longitud de `ix_name`), de los que 8 no están descritos.
- **Identificado el 2026-10-22**: los 8 bytes son `quote_transfer_fee`, la comisión de transferencia Token-2022 del mint de quote. Según el SDK, se cobra encima de la entrada en las compras y se descuenta de la salida en las ventas; vale 0 si el quote es SOL.
  - Acotación: el último evento sin sobrantes es del slot 458 888 128 y el primero con ellos del 458 921 419.
  - En 11 trades reales de antes y de después cuadran la constante de la curva, las reservas reales, las comisiones y el paso creación → trade.
  - Detalle: docs/informe_noche_2_2026-10-23.md.
- Desde el 2026-10-22, la decodificación cuenta estos eventos con bytes de más y avisa una sola vez por ejecución, con la longitud observada (`tx::TRADE_EXTRA_BYTES`). `check`, `windows`, `prices` e `index-tramo` imprimen el contador al final. No es un error: el prefijo descrito se decodifica igual.
- En los fixtures de V1–V3 no sobra ningún byte.

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

### `SweepBondingCurveFeeEvent` (nuevo 2026-10; discriminador `742b4dbd117a482b`) — **no es un trade**
```
timestamp, mint, bonding_curve, quote_mint, recipient, amount (u64), bucket (u8: 0 = protocolo, 1 = creador)
```
- Lo emiten `sweep_protocol_fee` y `sweep_creator_fee`, que barren hacia `recipient` las comisiones que ahora se acumulan en la propia curva (`BondingCurve.protocol_fees` / `creator_fee`).
- Lo ejecuta un crank (p. ej. `FHpcNSe6…`, ~1 000 tx cada 2 min), a menudo varios tokens por tx, o lo hace antes de una migración a fee sharing.
- Sin usuario, importes de token ni reservas: no sirve para precio ni volumen.
- Con el IDL del 2026-09-28, era el "evento con discriminador desconocido" que hacía fallar `check` en vivo.

### `PostCompleteBuyEvent` (nuevo 2026-10; `6fb06d8b316cd5fb`) — compra sobre una curva ya completada, **sin `TradeEvent`**
```
user, mint, bonding_curve, quote_mint, timestamp, base_out, quote_in, fee_basis_points, fee,
creator_fee_basis_points, creator_fee, buyback_fee,
pool_base_reserves_before, pool_quote_reserves_before, pool_base_reserves_after, pool_quote_reserves_after
```
- Las ventanas de marXol son n/e a partir del completado, así que no les afecta.
- Un indexador de precio posterior al completado tendría que leerlo.
- No observado aún en mainnet.

### Otros eventos nuevos (2026-10)
`SetQuoteControlMintReservesEvent` (`a943276d6686b6e8`), `SetQuoteControlReservesAdminEvent` (`43fbb3b6575ca921`): administración de `QuoteControl`.

### Instrucciones nuevas (2026-10), con discriminador
- Trades:
  - `buy_v3` `07051dc4f5176550`
  - `sell_v3` `1c92de7726c469d5`
  - `buy_exact_quote_in_v3` `e1f7501ed5b38488`
  - `multi_hop_curve_swap` `e19a7516d756f667` (swap encadenado entre curvas con pump coins como quote)
- Comisiones:
  - `sweep_creator_fee` `20f6bf3408c949ba`
  - `sweep_protocol_fee` `0830be07b644b7e5`
- Administración:
  - `set_max_curve_depth` `209a98db8e6cdaa5`
  - `set_quote_control_mint_reserves` `74dc6b53bf2c2049`
  - `set_quote_control_reserves_admin` `a1c3dea5e165c03e`
- `buy`, `buy_v2`, `buy_exact_sol_in` y `buy_exact_quote_in_v2` añaden el arg `partial_fill` al final (mismo discriminador y cuentas).
- `buy_v3`, `sell_v3` y `multi_hop_curve_swap` no se han observado aún. No está verificado que emitan `TradeEvent`, aunque es lo esperable. El encadenado de reservas de `entry::execution_order` detectaría un trade que faltara en una ventana.

### Otros eventos del IDL (28 hasta 2026-09, no todos relevantes para el MVP)
`AddQuoteControlMintEvent, AdminCtoEvent, AdminSetIdlAuthorityEvent, AdminUpdateTokenIncentivesEvent, ClaimCashbackEvent, ClaimTokenIncentivesEvent, CloseUserVolumeAccumulatorEvent, ExtendAccountEvent, InitUserVolumeAccumulatorEvent, MinimumDistributableFeeEvent, RemoveQuoteControlMintEvent, ReservedFeeRecipientsEvent, SetMetaplexCreatorEvent, SetQuoteControlAdminEvent, SyncUserVolumeAccumulatorEvent, UpdateCreatorFeeConfigEvent, UpdateGlobalAuthorityEvent, UpdateMayhemVirtualParamsEvent`

### Structs de cuenta relevantes

**`BondingCurve`**:
```
virtual_token_reserves, virtual_quote_reserves, real_token_reserves, real_quote_reserves,
token_total_supply, complete, creator, is_mayhem_mode, is_cashback_coin,
quote_mint, creator_fee_bps, can_edit_creator_fee, is_holder_reward,
creator_fee, protocol_fees, depth, initial_virtual_quote_reserves,
post_complete_base_out, post_complete_quote_in   (los 6 últimos, nuevos 2026-10)
```

**`Global`**: ver sección 4 ("Parámetros económicos"). Nuevo en 2026-10: `max_curve_depth` (u8), al final.

**`QuoteControl`**: en 2026-10 cambia el orden (`reserves_admin: pubkey` ocupa parte de `_reserved`). marXol no la decodifica.

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
| Alchemy | 30M CU/mes | 25 req/s (500 CU/s) | `getTransaction`=40 CU, `getSignaturesForAddress`=40 CU, `getBlock`=40 CU, `getProgramAccounts`=20 CU | Yellowstone gRPC facturado aparte por bandwidth ($75/TB, prorrateado); **solo PAYG/Enterprise, no en el free tier** (verificado en la doc, 2026-10-09). WebSocket de Solana facturado por byte (sección 7, corrección) |
| Helius | 1M créditos/mes | 10 req/s, 2 req/s DAS, 2 conexiones WebSocket | Métodos de archivo (`getTransaction`, `getBlock`, `getSignaturesForAddress`) = 10 créditos (10x el resto) | `logsSubscribe`/WebSocket estándar sí disponible en free tier; LaserStream gRPC solo en plan Professional (pago) |
| QuickNode | 10M créditos/mes | — | — | Streams como add-on de pago |
| dRPC | 210M CU/mes | Solo contra nodos públicos compartidos | — | — |

**Ritmo por defecto de marXol (`rpc::Provider`, desde 2026-10-22)**:
- Alchemy **12 req/s** (× 40 CU = **480 CU/s**, bajo el límite de 500 CU/s). Antes eran 20 req/s = 800 CU/s, por encima del límite.
- Helius 8 req/s (bajo sus 10 req/s) y RPC público 4 req/s.
- Se cambia con `--max-rps` o con `MARXOL_MAX_RPS`.
- El límite que manda en Alchemy es el de CU/s, no el de 25 req/s: con métodos de 40 CU, 25 req/s serían 1 000 CU/s.
- Consecuencias:
  - Una descarga secuencial de N `getTransaction` tarda al menos N / 12 s: ≈ 83 ms por tx, frente a los ≈ 50 ms de las mediciones del 2026-10-21, hechas a 20 req/s.
  - En `check`, una ventana de 40 tx pasa de ~2 s a ~3.5 s.
  - El tope de 400 peticiones (16 000 CU) lleva ≥ 33 s, frente a 20 s antes.
- Cada reintento por 429 se avisa ahora en stderr (proveedor, método, qué se pedía, nº de reintento y espera; nunca la URL, que lleva la clave). Al final se imprime el total de reintentos, junto al contador de `TradeEvent` con bytes de más.

### Por qué el polling puro no escala aquí (a diferencia de Robinhood Chain)

Volumen citado: 9,000–24,000 lanzamientos diarios solo en pump.fun en picos, sin contar trades. Con Alchemy (25 req/s nominales, pero 500 CU/s = 12.5 `getTransaction` por segundo; marXol usa 12), indexar vía `getSignaturesForAddress` + `getTransaction` por firma agota la cuota mensual antes de fin de mes y el rate limit deja el indexador sistemáticamente por detrás del chain en momentos de actividad alta (bloques cada ~400ms).

### Decisión de arquitectura

1. **Captura en tiempo real de creaciones**: WebSocket `logsSubscribe` filtrado por el program ID de pump.fun, reforzado con `getTransaction` puntual cuando el log se trunca o hace falta el payload completo decodificado. Nota: `blockSubscribe` está marcado "inestable" oficialmente por el equipo de Solana (desconexiones reportadas en carga alta) — no usar como vía principal.
   **Corrección (2026-10-09, Etapa 1, `scripts/bench_stream.py`): en Alchemy NO es gratis.**
   - Se factura por bytes entregados. La página de costes da dos tarifas sin aclarar cuál aplica: 0.0002 CU/byte (sección "Solana: WebSocket Subscriptions") y 0.04 CU/byte ("All other subscriptions").
   - Verificado que funciona con la clave del plan gratuito (`wss://solana-mainnet.streaming.alchemy.com/v2/…`).
   - Con `mentions` = programa pump y commitment `confirmed` (15 s): 117 msg/s, 246 kB/s (≈ 21 GB/día), 65 % de tx fallidas (no se pueden filtrar), latencia mediana 1.2 s y p99 2.1 s respecto a `blockTime`, 0 firmas perdidas frente a `getSignaturesForAddress`.
   - Extrapolado: ≈ 4.2 M CU/día con la tarifa barata y ≈ 850 M con la genérica. Ninguna de las dos cabe en el free tier con este filtro. Detalle en `resultados/benchmark_streaming.txt`.
2. **Medir consumo real antes de descartar Alchemy**: el caso de uso (solo eventos de creación + cambios de creador, no todo el volumen de swaps) es mucho más ligero que indexar trades completos. No asumir que el free tier no alcanza sin medirlo con tráfico real de un día.
3. **Benchmarking Helius como alternativa**, especialmente por el coste distinto por método (10 créditos por llamada de archivo vs. 40 CU-equivalente de Alchemy) — puede rendir distinto según el patrón real de consumo.
4. **Devnet para probar el pipeline de decodificación** sin gastar cuota de mainnet (el programa pump.fun también está desplegado en devnet).
5. Preferencia declarada: no depender de plataformas de pago — free tier combinado de RPC público + Alchemy free, igual que en marXi.

---

## 9. No verificado — pendiente de confirmar contra la chain real antes de construir encima

Esta sección existe porque el principio 1 de la filosofía marXi lo exige: nada de esta lista se ha confirmado contra un nodo RPC en vivo, solo contra documentación pública oficial (repo `pump-fun/pump-public-docs` en GitHub) y el IDL descargado de ahí. Antes de escribir código que dependa de estos valores, verificarlos directamente contra `getAccountInfo` / lectura de la cuenta `Global` real en mainnet:

- ~~Que el program ID es ejecutable y corresponde al programa pump.fun activo~~ → **verificado 2026-09-28** (sección 4).
- ~~El umbral de graduación real actual~~ → **verificado 2026-09-28**: 85.0054 SOL reales (sección 4). Nota: `final_real_sol_reserves` no está en `Global`, solo en `SetParamsEvent`; el umbral se deriva de `initial_*`.
- ~~Si Yellowstone gRPC de Alchemy está disponible en el plan free~~ → **no lo está** (2026-10-09): "only available to PAYG (Pay-As-You-Go) and Enterprise teams" (`yellowstone-grpc-quickstart`).
- Qué tarifa por byte aplica Alchemy a los WebSocket de Solana (0.0002 o 0.04 CU/byte): mirarlo en el panel con el consumo del 2026-10-09 a las 08:15 UTC (3.56 MB).
- Cualquier cambio reciente en el IDL no reflejado en la copia descargada (el repo está activo, con actualizaciones frecuentes — releer antes de asumir que esta sección sigue vigente si ha pasado tiempo desde la última actualización de este documento). Señales de desfase: `marxol verify` muestra campos ausentes o bytes sobrantes en `Global`, o aparecen "evento con discriminador desconocido" / "campos del IDL ausentes" al decodificar tx recientes. Desde el 2026-10-22, `verify` falla en ese caso y los `TradeEvent` con bytes de más se avisan. Procedimiento tras cada despliegue: sección 6.
- Casos reales de `SetCreatorEvent` (no observados aún, ver 5.2). `MigrateBondingCurveCreatorEvent` ya observado (2026-09-28).

---

**Hallazgos operativos (mainnet, 2026-09-28)**:
- **Transacciones versión 1 en mainnet**: `getTransaction` con `maxSupportedTransactionVersion: 0` falla (error -32015) con parte del tráfico de pump. Hay que pedir `1`. `solana-transaction-status-client-types 4.3` las decodifica bien en encoding `json`.
- **~68 % de las firmas del programa pump son tx fallidas** (203 de 300 en una muestra). `getSignaturesForAddress` ya trae `err`, así que el indexador las descarta sin gastar un `getTransaction` (40 CU en Alchemy) en cada una. Es un ahorro grande de cuota que hay que tener en cuenta en el benchmark del paso 4.
- En una muestra de 97 tx exitosas del programa: 57 `TradeEvent`, 22 `DistributeFeeToHoldersEvent`, 3 `CollectCreatorFeeEvent`, 1 `CreateEvent`. Las creaciones son una fracción pequeña del tráfico: recorrer las firmas del programa entero para encontrarlas es caro. Para seguir a un operador concreto es mucho más barato `index --address <creator>`.
- **Vía barata para listar creaciones**: la PDA `mint-authority` (`TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM`, seeds `["mint-authority"]`, la imprime `marxol verify`) solo la tocan `create`/`create_v2`. En 400 firmas suyas: 22 tx fallidas y 378 exitosas, de las que 377 traen `CreateEvent` (la restante no está examinada). `index --address TSLvdd1p…` indexa creaciones sin recorrer el tráfico de trades del programa (~1 creación por cada 100 tx).
- **Ritmo de creación observado**: 377 creaciones en 492 s (2026-09-28, 14:03–14:11 UTC) ≈ 0.77/s. Es una muestra de 8 minutos, no una media diaria.
- **Orden de ejecución dentro de un slot (2026-09-29)**: ni `getTransaction` ni el orden `(slot, timestamp, firma)` lo dan (la firma es un hash). Se reconstruye exacto encadenando reservas: `virtual_token_reserves` antes de un trade = después + `token_amount` (compra) o − `token_amount` (venta), y coincide con el después del trade anterior, empezando por el `CreateEvent`. Encadena 2 831 de 2 831 trades de la ventana temprana del piloto. Hace falta en todo lo que dependa del orden dentro del slot (primer trade que cruza un umbral, precio de cierre). H1 y H1c no dependen de él (ver sección 8). Ignorarlo produjo un falso positivo en H5 (p = 0.026 → 0.42 tras corregir).
- **Tokens mayhem y comisiones (2026-09-29, validación)**: los 36 tokens con `fee_basis_points + creator_fee_basis_points = 0` en T_entry2 son todos mayhem (48 mayhem en total entre 251). En no-mayhem la comisión más común es 125 bps (169 tokens), con valores de 96 a 395 bps. Nunca suponer la comisión de `Global` (95 + 5): leerla del `TradeEvent`.
- **Rate limit de Alchemy**: aparecen 429 esporádicos (8 en ~22 700 peticiones) incluso a ~5 req/s efectivas. Hay que reintentar siempre.
  - En la prueba en vivo del 2026-10-22, a 20 req/s, hubo latencias de hasta 37.6 s con 16 peticiones, compatibles con esperas de reintento (1 + 2 + 4 + 8 s). No se pudieron confirmar porque los reintentos no se registraban.
  - Desde el 2026-10-22 se registran, y el ritmo por defecto es de 12 req/s (ver tabla de proveedores).
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

**`creator` en las instrucciones de creación (verificado 2026-10-22, idl/pump.json y 6 tx de creación de tests/fixtures/tx)**: en `create` y `create_v2`, `creator` es un argumento, no una cuenta. La única wallet del creador que aparece en la tx (y firma) es `user`, el pagador. `getSignaturesForAddress(creator)` no encuentra las creaciones con ese `creator` pagadas por otra wallet. En `marxol.db`, `creator = user` en 842 de 1 015 creaciones (83 %).
