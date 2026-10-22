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

## Registro de cambios

- 2026-10-22: versión inicial, antes de leer ningún resultado de `marxol_vivo.db`.
