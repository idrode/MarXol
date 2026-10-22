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

## Espacio para C con retención de 60–90 días (2026-10-22, medido; sin cambiar nada)

**Medido en `marxol_vivo.db`**:
- 4 357 firmas en `vivo_create_sigs` ocupan 1 044 480 bytes: tabla 503 808, índice de la clave 475 136 e índice de blockTime 65 536. Son unos **240 bytes por firma**: la firma en texto base58 (88 caracteres) se guarda dos veces, en la tabla y en su índice.
- Ritmo: 0,49–0,59 firmas por segundo en las medidas, unas **42 000–51 000 al día**. El 4 % tiene error.

**Tabla actual** (firma completa):

| Retención | Filas | Espacio |
|---|---|---|
| 30 d (la actual) | 1,3–1,5 M | **300–370 MB** |
| 60 d | 2,5–3,1 M | 600–735 MB |
| 90 d | 3,8–4,6 M | 0,9–1,1 GB |

Ya con la configuración actual son unos 300–370 MB en el teléfono cuando se estabilice, más las exportaciones.

**Propuesta compacta**, solo para el cruce de C:
- `vivo_create_h_AAAAMMDD (h INTEGER PRIMARY KEY, slot INTEGER NOT NULL)`, una tabla por día UTC.
  - `h` = los 8 primeros bytes de la firma decodificada, como entero con signo.
  - Las firmas ed25519 son uniformes, así que no hace falta otro hash.
  - Solo se guardan las firmas sin error.
- `h` es el rowid, así que no hay índice aparte. Unos 20 bytes de registro; con las claves aleatorias, las páginas se llenan al ~70 %, así que quedan **≈ 30 bytes por firma**.
- **Retención**: `DROP TABLE` del día que caduca. Es instantáneo, sin borrados fila a fila ni índice por tiempo.
- **Cruce**: las firmas del pagador se convierten a `h` y se buscan en las tablas de los 7 días de la ventana.
- **Colisiones**: con 4,6 M filas (90 días), la probabilidad de alguna colisión en toda la tabla es ≈ n²/2⁶⁵ ≈ 6·10⁻⁷. Una colisión solo sumaría 1 previo a un pagador. Se documenta como límite.

| Retención | Espacio compacto |
|---|---|
| 60 d | ≈ 76–92 MB |
| 90 d | ≈ 113–138 MB |

**Además**, si se activa C, se puede bajar la retención de la tabla completa `vivo_create_sigs` de 30 a 7 días: de 300–370 MB a 70–85 MB. Después de listar no se usa salvo para exportarla, y las exportaciones CSV.gz se conservan.

**Comprobación antes de fiarse**: una semana con las dos tablas a la vez. Los previos calculados con `h` tienen que coincidir con los de la firma completa en todos los tokens muestreados.

No se ha cambiado nada. Requiere el OK del dueño, que lo decide tras el informe de 24 h.
