# Pre-registro de la fase 5 (validación fuera de muestra sobre `marxol_vivo.db`) — 2026-10-22

Redactado y versionado **antes de leer ningún retorno, veredicto ni resultado agregado de `marxol_vivo.db`**. Al escribirlo, la base solo se ha consultado para contar firmas, tokens por estado y CU (`collect-status`). `vivo_results` no se ha leído.

- `collect` arrancó en continuo el 2026-10-22 a las 02:15:51 UTC, con `--sample-ppm 4000 --poll-secs 300` y los topes por defecto.
- Las definiciones que se validan están congeladas en docs/congelado_vivo_2026-10-22.md (commit `a74a456`).

## Reglas generales

1. `marxol_vivo.db` queda **reservada para validar**. La fase 3 (métricas nuevas) se explora y se ajusta solo sobre `marxol.db`.
2. Cada evaluación de esta fase se ejecuta **una sola vez**, en la fecha fijada aquí o después, nunca antes. No hay paradas tempranas ni miradas intermedias a los resultados.
3. **Operación permitida mientras tanto** (no lee resultados):
   - `collect-status` y conteos por estado y por hora;
   - CU y 429;
   - cambiar la tasa de muestreo, con el OK del dueño;
   - actualizar el IDL tras un despliegue (procedimiento de referencia_tecnica §6) y reprocesar los tokens con errores de decodificación.

   Todo cambio operativo se anota con su fecha. La tasa no cambia la población, porque cada token muestreado lo es por hash de su firma, uniforme e independiente del resultado.
4. Si el dueño pide ver algo antes de la fecha de un bloque, se le avisa de que ese bloque deja de ser fuera de muestra y se anota aquí.

## Partición por tiempo (blockTime del `CreateEvent`)

| Bloque | Creaciones con `timestamp` en | Uso |
|---|---|---|
| Rodaje | [arranque, 2026-10-23 00:00 UTC) | Solo operación. No entra en ninguna evaluación ni se leen sus resultados |
| **A** | [2026-10-23 00:00, 2026-10-30 00:00) UTC — 7 días completos | Réplica de D2 (V4-D2) |
| **B** | [2026-10-30 00:00, 2026-11-13 00:00) UTC — 14 días | Validación de las métricas de la fase 3 (adendas) |
| C | desde 2026-11-13 00:00 UTC | Reserva para pre-registros futuros |

En unix: A = [1792713600, 1793318400), B = [1793318400, 1794528000).

## V4-D2: réplica del filtro D2 (bloque A)

**Hipótesis**: en tokens nuevos, de 7 días completos que cubren las 24 h, los tokens que pasan D2 tienen mayor retorno neto desde T_entry2 a +30 min que los descartados.

**Definiciones**: todas las de la validación 3 sin cambios (docs/validaciones_congeladas.md, "Validación 3" y su adenda operativa):
- T_entry2, H4', H6', H7' y D2;
- retorno neto (a) a +30 min como resultado principal;
- tratamiento de n/e;
- token de mayor peso.

El cálculo es el de `report::v3_rows` / `v3::*` en el commit congelado.

**Población**:
- Tokens del bloque A con `vivo_tokens.status = 'creado'`, excluidos los mayhem, con T_entry2.
- Un token por creator (`CreateEvent.creator`, el de menor slot, como en V2 y V3).
- Ventana temprana completa y ventana de precio completa (≥ 65 min).

Se cuentan aparte, y no entran:
- los tokens con ventana o precio incompletos por el tope de 400 tx por token (`--max-token-sigs`);
- los tokens `error` o `sin_create`;
- los tokens sin T_entry2;
- los tokens con retorno n/e;
- los creadores "sin datos" (adenda 7 de V3).

**Criterio de éxito**: los mismos 5 criterios de la validación 3, con los mismos números:
1. **n mínimo**: ≥ 20 descartados y ≥ 20 que pasan.
2. Mann-Whitney unilateral (pasan > descartados), p < 0.01.
3. Diferencia de medianas ≥ 10 puntos (tolerancia 1e-9).
4. Misma dirección sin el token de mayor peso.
5. Bootstrap de 2 000 réplicas, semilla 20260930: el IC al 95 % de la diferencia de medianas excluye 0 (estricto).

**Veredictos**:
- **"D2 SE REPLICA"** si se cumplen los cinco criterios.
- **"NO CONCLUYENTE POR POTENCIA"** si falla el 1. No se amplía el bloque A y no cierra D2.
- **"NO REPLICADO"** si se cumple el 1 y falla cualquiera de los criterios 2–5. Se registra igual de bien que un acierto. No se reinterpreta ni se prueba otra variante con el bloque A.

**Sesgo de cobertura** (fijado de antemano):
- Si los tokens excluidos por el tope de tx por token superan el **5 %** de los tokens `creado` del bloque, el veredicto se acompaña de "cobertura limitada: excluye los tokens más activos". Eso no cambia el veredicto.
- Se reporta también cuántos tokens del bloque tienen ventana o precio incompletos por cualquier motivo.

**Secundario** (descriptivo, sin criterio):
- Las tablas a +5 s, +30 s, +5 min y +60 min, y la versión (b).
- % descartado y fracción con retorno > 0.
- La tabla de mayhem.
- La diferencia de medianas por franja de 6 h UTC (00–06, 06–12, 12–18, 18–24), para ver si depende de la hora.
- Coste en CU por token.

**Fecha de evaluación**:
- No antes del **2026-11-01 00:00 UTC**, 48 h tras el fin del bloque, para que terminen las descargas de precio.
- Si aún quedan descargas pendientes del bloque, se espera a que no quede ninguna o hasta el 2026-11-03 00:00 UTC, lo que ocurra antes. Lo que siga pendiente cuenta como "sin datos".

**Implementación**:
- Un tramo nuevo de `entry2` sobre `--vivo-db` que reutiliza las funciones de V3 sin cambiarlas.
- Antes de ejecutarlo se prueba solo contra `marxol.db`: tiene que reproducir el veredicto de V3. No se prueba con el rodaje ni con el bloque A.

**Lo que NO se concluye**:
- Un veredicto positivo no implica que los tokens que pasan sean rentables. En V1–V3 su mediana fue negativa.
- Tampoco implica que el filtro sirva fuera de este periodo.

## Métricas de la fase 3 (bloque B)

Las métricas de la fase 3 todavía no están definidas: se exploran sobre `marxol.db`. Para validarlas:

1. Cada métrica que vaya a usarse como razón de veto en `check` (fase 4) se fija en una **adenda a este documento**, con commit antes de calcular nada sobre el bloque B. La adenda incluye:
   - la definición exacta y el código;
   - la dirección esperada;
   - el umbral;
   - el resultado que se compara: el retorno (a) a +30 min, u otro resultado de la curva fijado en la adenda, por ejemplo el estado a +7 d o +60 d de `vivo_outcomes`;
   - la población.
2. Plantilla de criterio, salvo que la adenda la endurezca:
   - ≥ 20 tokens por grupo;
   - Mann-Whitney unilateral en la dirección registrada;
   - diferencia de medianas ≥ 10 puntos para retornos (o, para proporciones, diferencia ≥ 10 puntos porcentuales con Fisher unilateral);
   - bootstrap de 2 000 réplicas (semilla 20260930) por clúster de creator, con un IC al 95 % que excluya 0.
3. **Comparaciones múltiples**: si la adenda registra k razones, el umbral de p es **0.01 / k** (Bonferroni) para cada una.
4. Una razón que no se valide queda en `check` solo como descriptiva ("sin validar"), nunca como causa de "evitar".
5. Los datos que la fase 3 necesite y `collect` no guarde (por ejemplo, la financiación de las wallets) se pueden descargar para los tokens del bloque B a posteriori, porque siguen en la cadena. Esa descarga se fija en la adenda.
6. **Fecha de evaluación**:
   - No antes del 2026-11-15 00:00 UTC para resultados de +1 h o +24 h.
   - Para resultados a +7 d o +60 d, no antes de que venzan en todos los tokens del bloque: el 2026-11-20 o el 2027-01-12, más 48 h.

## Adenda 1 (2026-10-22): razones de veto R1 y R2 para el bloque B

Redactada antes de calcular nada sobre el bloque B. Tampoco se han cruzado R1 ni R2 con ningún retorno, ni en `marxol.db`: la exploración (resultados/fase3_cprima.txt y fase3_explora.txt) solo contó tokens por grupo y resultados del creador.

Se puede revisar, con commit y anotándolo en el registro de cambios, **hasta que se calcule algo de R1 o R2 sobre el bloque B**. Después queda fija.

**k = 2 razones → umbral de p = 0,01 / 2 = 0,005 para cada una (Bonferroni).**

### R1: pagador en serie

- **Definición**:
  - Pagador = `CreateEvent.user`, la wallet que firma y paga la creación.
  - Previos = creaciones sin error en `[t0 − 7 d, t0)` cuya tx incluye al pagador. Se cuentan como las firmas sin error de la PDA mint-authority que también aparecen en `getSignaturesForAddress(pagador)`, sin la del propio token.
  - Es el método de `marxol payer-history` (`historial::prior_launches`), con 7 días y un tope de 5 páginas de 1 000 firmas por pagador.
- **R1 = verdadero si previos ≥ 5.** El umbral sale de los cortes de la exploración (0 / 1–4 / 5–49 / ≥ 50), fijados sin mirar retornos.
  - Un pagador truncado por el tope cuenta como verdadero si ya tiene ≥ 5 contados (es un mínimo). Si tiene menos, es n/e: se excluye y se cuenta aparte.
- **Fuente del listado de mint-authority en el bloque B**: `vivo_create_sigs` de `marxol_vivo.db`, solo firmas con `failed = 0`, sin coste extra.
  - Un token es n/e si su ventana `[t0 − 7 d, t0)` se solapa con un hueco de `vivo_gaps` o queda antes del inicio del listado.
  - El listado empieza el 2026-10-22 02:15 UTC y el bloque B el 30-10, así que cubre los 7 días previos de todos sus tokens.
  - La retención de 30 días no borra nada necesario antes de la evaluación.
- **Límite**: es un mínimo. No ve las creaciones con el mismo `creator` pagadas por otra wallet, ni operadores con varias wallets.
- **Datos**:
  - Las firmas de los pagadores del bloque B se descargan después del 2026-11-13, **con `collect` parado** (regla del dueño: no en paralelo), en la bolsa manual.
  - Estimación: ≈ 2 700 tokens × 1,8 páginas × 40 CU ≈ 200 000 CU, repartidas en los días que haga falta.

### R2: creador con resultado positivo en T_entry2

- **Por qué en T_entry2 y no a +5 min**: el veto se decide en T_entry2. La versión a +5 min de la exploración usa trades posteriores, así que solo se reporta como descriptiva.
- **Identidades del creador**: `creator` y pagador del `CreateEvent`. Cuentan si su primera compra es en el slot de creación, igual que en scripts/fase3_explora.py, sección 3. Si no compran en ese slot, R2 = falso.
- **Trades**: los de esas identidades **anteriores a T_entry2** en orden de ejecución, la misma partición que H4'–H7' (`entry2::compute`).
- **Resultado realista**, en SOL:
  - Ventas − compras, con las comisiones de cada trade: compra = `sol_amount` × (1 + (fee_bps + creator_fee_bps)/10⁴); venta = `sol_amount` × (1 − …).
  - Más la venta de una vez de lo que aún tienen, por la curva de producto constante sobre las reservas virtuales justo antes de T_entry2, pagando la comisión del último trade anterior.
  - La comisión de creador de sus propios trades cuenta como coste, y no se suma su ingreso por comisión.
- **R2 = verdadero si el resultado > 1 000 lamports (10⁻⁶ SOL).**
- **n/e**: tokens no cotizados en SOL, tokens con algún trade sin bps de comisión, y tokens sin T_entry2 (no entran en la población).
- **Implementación**: una función pura nueva en Rust. Antes de calcularla sobre el bloque B se prueba sobre `marxol.db`: tiene que reproducir el mismo cálculo hecho en Python con el corte en T_entry2. El commit queda anotado en el registro de cambios.

### Criterio (igual para R1 y R2)

- **Resultado**: retorno neto (a) a +30 min desde T_entry2 (congelado).
- **Población**: la de V4-D2 aplicada al bloque B:
  - sin mayhem, con T_entry2, un token por creator;
  - ventana temprana y de precio completas;
  - sin los n/e de cada razón, que se cuentan aparte.
- **Hipótesis**: tokens con R = verdadero tienen menor retorno que con R = falso.
- **"R VALIDADA" solo si se cumplen todos**:
  1. ≥ 20 tokens por grupo.
  2. Mann-Whitney unilateral (falso > verdadero) con **p < 0,005**.
  3. Diferencia de medianas (falso − verdadero) ≥ 10 puntos (tolerancia 1e-9).
  4. Misma dirección sin el token de mayor peso.
  5. Bootstrap de 2 000 réplicas, semilla 20260930, remuestreo por creator: IC al 95 % de la diferencia de medianas que excluye 0 (estricto).
- **Otros veredictos**: si falla el 1, "NO CONCLUYENTE POR POTENCIA" (no se amplía el bloque). Si se cumple el 1 y falla alguno de los criterios 2–5, "NO VALIDADA".
- Una razón no validada queda en `check` solo como descriptiva.
- **Secundario** (descriptivo):
  - cada razón dentro de los tokens que pasan D2 (¿añade algo a D2?);
  - solapamiento de R2 con H4';
  - R1 por subgrupos 5–49 / ≥ 50;
  - R2 a +5 min;
  - retornos a +5 min y +60 min.
- **Fecha**: una sola evaluación, no antes del **2026-11-15 00:00 UTC** y con los datos de R1 descargados.

## Registro de cambios

- 2026-10-22: versión inicial, antes de leer ningún resultado de `marxol_vivo.db`.
- 2026-10-22: adenda 1 (R1 pagador en serie, R2 creador con resultado positivo en T_entry2; Bonferroni k = 2), antes de calcular nada sobre el bloque B y sin cruzar R1/R2 con retornos.
- 2026-10-27: adenda 3 (docs/adenda_3_2026-10-27.md, sha256 `e6364813b582d4d384bcbbfe2e2bdff6a7323a856a9663d3618be81c08787a8b`): protocolo de recolección y amenazas a la validez, reglas de cobertura, mínimos en tokens evaluables (A ≥ 900, B ≥ 1 800) con ampliación por días UTC y fecha tope (sustituye «no se amplía el bloque» de V4-D2 y de la adenda 1, y las fechas fijas de B y C), sensibilidad por retraso de captura (2 h), huecos, inventario de cortes exploratorios. Incorpora la adenda 2 (k = 2, p < 0,005; C1 solo descriptiva). Comprometida antes del fin nominal de A y sin leer resultados de `marxol_vivo.db`.
