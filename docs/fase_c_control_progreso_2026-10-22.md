# Fase C, control mecánico de C1 por progreso de la curva: declaración previa (2026-10-22)

Exploratorio y post hoc. Escrita **antes de calcular nada** de este control: hasta ahora no se ha cruzado el progreso de la curva en T_entry2 ni con C1 ni con retornos. Los retornos de `marxol.db` ya se conocen por la fase C (resultados/fase_c_banco.txt), así que esto no valida nada. De `marxol_vivo.db` no se lee nada. Su sha256 se anota en docs/informe_noche_2026-10-22.md.

## Hipótesis
"El efecto de C1 persiste dentro de estratos de progreso de la curva (capitalización o reservas) en T_entry2."

Motivo del control: la tenencia del creador sale de compras que mueven la curva, así que C1 puede ser en parte un indicador de progreso (más tokens vendidos, precio más alto) y no de concentración.

## Población y variables (las de la fase C, sin cambios)
- Misma población que `marxol banco-fase-c`: V1–V3 de `marxol.db`, sin mayhem, con T_entry2, un token por creator, retorno (a) a +30 min evaluable (n esperado 438).
- C1 = tenencia del creador en T_entry2 > 5 % del supply. **El umbral no cambia.**
- Resultado: retorno neto (a) desde T_entry2 a +30 min.

## Progreso de la curva en T_entry2
- Estado de la curva **justo antes del trade T_entry2**: las reservas virtuales tras el último trade anterior a T_entry2 en orden de ejecución (la partición de `entry2::compute`), o las iniciales del `CreateEvent` si no hay ninguno.
- **Progreso = 1 − reservas virtuales de token antes de T_entry2 / reservas virtuales de token iniciales.**
- Con producto constante de reservas virtuales, la capitalización (precio × supply, precio = reservas de quote / reservas de token) es función creciente de este progreso, así que da los mismos terciles. Se comprueba y se informa el número de tokens en que los terciles por capitalización difieren de los de progreso.
- **Terciles**: cortes en los cuantiles 1/3 y 2/3 del progreso en la población evaluable (sin mirar retornos). Empates: un token con progreso igual al corte va al tercil inferior.

## Contraste (el mismo de la declaración de la fase C, con estos estratos en vez de los de actividad)
- En cada tercil: n, % con C1, medianas, diferencia de medianas (sin − con) con IC bootstrap por creator (2 000 réplicas, semilla 20260930) y Mann-Whitney unilateral (sin > con).
- Combinado: van Elteren unilateral y diferencia ponderada por n_s con bootstrap estratificado por creator.
- **"Sobrevive"** solo si se cumplen las tres: la diferencia ponderada tiene la dirección declarada (sin C1 > con C1); su IC 95 % excluye 0; ningún tercil con ≥ 10 tokens por grupo va en la dirección contraria.
- Si un tercil tiene < 10 tokens con C1 se informa igual, y se dice expresamente que ese tercil no tiene potencia.

## Descriptivo
- Progreso mediano de los tokens con y sin C1.
- La misma tabla con terciles de capitalización si difiriese de la de progreso.
