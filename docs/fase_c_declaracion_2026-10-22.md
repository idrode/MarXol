# Fase C: declaración previa (2026-10-22), exploratoria y post hoc

Escrita **antes de cruzar ninguna de estas variables con retornos**. Hasta este momento solo se han contado tokens por condición y su solapamiento con H4', H6' y H7' (resultados/auditoria_fase_a.txt).

Límites de esta declaración:
- Los retornos de `marxol.db` (V1–V3) ya se conocen por fases anteriores, así que esto **no es una validación**: es una exploración post hoc que no decide nada.
- Lo que se valide irá a una adenda del pre-registro y se evaluará en el bloque B de `marxol_vivo.db`.
- De `marxol_vivo.db` no se lee nada.

## Población
- Tokens de V1, V2 y V3 de `marxol.db` sin mayhem y con T_entry2 (`entry2::compute`, código congelado).
- Un token por creator: `v3::population`, el de menor slot de los tres tramos juntos.
- El piloto queda fuera, porque es calibración y no tiene comisiones.
- Retorno n/e: se cuenta aparte y no entra.

## Variables (solo con información disponible en T_entry2)
- **Antes de T_entry2**: los `trades_before` primeros trades de la ventana en orden de ejecución, la misma partición que H4'–H7'.
- **Identidades de creador en T_entry2**:
  - `creator` y `user` del `CreateEvent`;
  - `new_creator`/`old_creator` de los cambios con slot < slot de T_entry2;
  - el `creator` embebido en los trades anteriores a T_entry2.
- **Principal, C1**: tenencia del creador en T_entry2 = Σ compras − Σ ventas de token de esas identidades antes de T_entry2, en % del supply (10^15). **C1 = tenencia > 5 %.**
  - El umbral sale de los cortes de la auditoría (0 / 1 / 5 / 10 / 20 %), elegidos sin mirar retornos.
  - No ve transferencias de token ni wallets del creador fuera de esas identidades: es un **mínimo**.
- **Secundaria, C2**: hay ≥ 1 tx anterior a T_entry2 con compras (`TradeEvent` de compra) de ≥ 2 `user` distintos del mismo mint.
  - El `user` de una compra firma la tx, así que son varias firmas en una tx.
  - Es un **mínimo**: no ve bundles de Jito con varias tx, wallets fondeadas desde exchanges ni operadores con muchas wallets que compran en tx separadas.
- Fuera: el top-N de cuentas ajenas (Spearman 0,82 con el llenado de H6').

## Hipótesis (dirección fijada aquí)
- **C1**: los tokens con C1 tienen **menor** retorno neto (a) desde T_entry2 a +30 min que los tokens sin C1. Motivo: el creador conserva una posición grande que puede vender sobre los compradores posteriores.
- **C2**: los tokens con C2 tienen **menor** retorno (a) a +30 min que los tokens sin C2.

## Métrica principal y contrastes
- Diferencia de medianas (sin − con) del retorno (a) a +30 min, y Mann-Whitney unilateral (sin > con).
- Bootstrap de 2 000 réplicas, semilla 20260930, por creator.
- Todo con `banco::evaluar`: modelo trivial, MCC, AUPRC, IC, fragilidad, censura y bloques temporales V1 / V2 / V3.

## Control por actividad (la cuota del creador sube mecánicamente con pocos trades)
- **Estratos fijos** por número de trades antes de T_entry2: **≤ 2, 3–9, ≥ 10**.
  - Los cortes salen de la distribución de la auditoría (p25 = 2, mediana = 5, p75 ≈ 18), sin mirar retornos.
- En cada estrato: n, % con la variable, medianas, diferencia y Mann-Whitney unilateral.
- **Combinado**: test de van Elteren unilateral (Wilcoxon estratificado, pesos 1/(n_s + 1)).
  - También la media de las diferencias de medianas por estrato, ponderada por n_s, con bootstrap estratificado por creator.
- **"El efecto sobrevive al control"** solo si se cumplen las tres:
  - la diferencia ponderada tiene la dirección declarada;
  - su IC excluye 0;
  - ningún estrato con ≥ 10 tokens por grupo va en la dirección contraria.

## Etiqueta secundaria (por criterio, no por datos)
- **Pérdida de la mitad o más**: retorno (a) a +30 min ≤ −50 %.
  - Es un umbral redondo y convencional, que no se ha ajustado a la distribución.
  - MCC, precisión, recall y AUPRC se dan con esta etiqueta y con "retorno ≤ 0".

## Descriptivo, sin criterio
- C1 y C2 dentro de los tokens que pasan D2 y dentro de los descartados.
- Retorno por tramos de tenencia: 0, (0, 1], (1, 5], (5, 10] y > 10 %.
- Solapamiento de C1 con H8 y con H4'.
