# Propuesta: medir creadores en serie con un muestreo del 0,4 % (2026-10-22)

Pendiente de decisión del dueño. No hay nada implementado.

## Hechos verificados (sin red)

- En `create` y `create_v2` (idl/pump.json), `creator` es un **argumento** de la instrucción, no una cuenta. La única wallet del creador que aparece como cuenta, y que firma, es `user` (el pagador).
  - Por eso `getSignaturesForAddress(wallet)` solo encuentra las creaciones **pagadas** por esa wallet.
  - Las creaciones con `creator = X` pagadas por otra wallet no se ven así.
- En `marxol.db`, `creator = user` en 842 de 1 015 creaciones (83 %): 632 creators distintos, 562 pagadores distintos.
  - En la muestra, 555 creators tienen 1 token y 77 tienen varios (hasta 40).
  - Comprobado también en las 6 tx de creación de `tests/fixtures/tx`: el pagador firma y el creator está entre las cuentas porque coincide con él.
- `collect` ya guarda **todas** las firmas de mint-authority que lista (`vivo_create_sigs`, unas 51 000 al día), muestreadas o no. Es la lista de todas las creaciones del periodo, sin su creador.

## Opciones

**A. Aceptar el límite (0 CU).**
- Con p = 0,004, un creador con k lanzamientos en el periodo aparece dos o más veces en la muestra con probabilidad ≈ C(k,2)·p²:
  - k = 10: ≈ 0,07 %;
  - k = 40: ≈ 1,2 %;
  - k = 250: ≈ 26 %.
- Los creadores en serie quedan prácticamente invisibles: la muestra solo permite describir tokens, no creadores.

**B. Muestreo por creador (hash del creator) — inviable en el free tier.**
- Habría que conocer el creator de todas las creaciones. Es un `getTransaction` por creación: unas 49 000 × 40 ≈ **1,96 M CU/día**, el doble del total diario.
- Solo sería viable con `logsSubscribe` (el `CreateEvent` va en el log). Pero la tarifa por byte del WebSocket de Alchemy sigue sin aclarar, con un factor 200 (resultados/benchmark_streaming.txt).

**C. Historial del pagador por intersección (recomendada).**
- Para cada token muestreado, se listan las firmas de su pagador (`getSignaturesForAddress`, 1 000 por petición, 40 CU).
- Se cruzan con `vivo_create_sigs`, sin pedir ninguna tx. El resultado es el número de lanzamientos de esa wallet en el periodo observado, antes y después del token.
- Se repite a +7 d y a +30 d para contar los lanzamientos posteriores: "lanzó N tokens en los 30 d siguientes", un dato verificable.
- **Coste**:
  - Una página en una wallet nueva o puntual: 40 CU.
  - Tope propuesto de 5 páginas por consulta (200 CU), con la consulta marcada como truncada si se alcanza.
  - Con unos 196 tokens al día y 2 consultas cada uno: ≈ 16 000–31 000 CU/día si la media es de 1–2 páginas, y ≈ 78 000 CU/día en el peor caso, todas en el tope.
  - Sumado a la cota pesimista actual de `collect` (≈ 686 000), el peor caso pasaría de la bolsa de 700 000. Habría que bajar el muestreo a unos 3 600 ppm, o vigilarlo con el informe de 24 h.
- **Límites**:
  - Mide por pagador. No ve las creaciones con el mismo `creator` pagadas por otras wallets (17 % de las creaciones en `marxol.db`), ni un operador que use muchas wallets. Eso es la métrica de fábricas (fuente de fondos), aparte.
  - Solo cuenta lo ocurrido desde que `collect` lista mint-authority (2026-10-22 02:15 UTC).
  - Requiere guardar `vivo_create_sigs` al menos 60–90 días: unas 51 000 filas al día, unos 7 MB al día. Hoy la retención es de 30 días para todos los crudos, así que haría falta una retención por tabla (un cambio pequeño de código).
- Encaje con el pre-registro: los datos de los bloques A y B se recogen, pero la métrica se define y se ajusta sobre `marxol.db` (punto siguiente). Solo se validaría sobre el bloque B con una adenda.

**C' (fase 3, solo sobre `marxol.db`): el mismo cruce, hacia atrás.**
- Listar mint-authority desde ahora hasta el 2026-09-28: unos 24 días × 51 000 ≈ 1,2 M firmas, 1 200 páginas, ≈ **48 000 CU**.
- Listar las firmas de los 562 pagadores de V1–V3 en ese periodo, con un tope de 5 páginas: ≈ **25 000–110 000 CU**.
- Total: ≈ **75 000–160 000 CU**, una vez. Da, para cada token de V1–V3, cuántos lanzamientos hizo su pagador en las ±3 semanas, sin ningún `getTransaction`.
- Cabe en la bolsa manual (300 000 al día) si se reparte en dos días.

## Recomendación

- **C' ahora**, en la fase 3 sobre `marxol.db`, para definir la métrica "creador en serie" y ver si sirve.
- **C en `collect`** solo si C' da algo que valga la pena validar, con retención por tabla de 90 días para `vivo_create_sigs`.

Hasta entonces, **A**: el límite queda documentado y `collect` sigue guardando todas las firmas de creación. C sigue siendo posible a posteriori sobre los bloques A y B, mientras esas firmas no se borren.
