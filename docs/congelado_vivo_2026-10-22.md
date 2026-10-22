# Congelado antes de la recolección continua (2026-10-22)

Fijado **antes de recoger ningún token en `marxol_vivo.db`** (la base no existía al redactarlo). Definiciones vigentes en el commit `a3125fb`, sin cambios respecto a la validación 3.

## Qué queda fijo

**Filtro D2**:
- Un token se descarta si cumple al menos 2 de H4', H6' y H7' (`v3::d2`).
- Si H6' es n/e y se cumple exactamente una de H4'/H7', el token es "no evaluable" (adenda 1 de V3).
- Texto literal: docs/validaciones_congeladas.md, "Validación 3: filtro de descarte D2 — CONGELADO" y su adenda operativa.

**Indicadores, T_entry2 y resultados**: definición literal en docs/validaciones_congeladas.md ("Validación de H4', H5, H6', H7', H8 — CONGELADO"). Valores en el código (`entry2::PARAMS`, `h1b::PARAMS_1H`):

| Parámetro | Valor |
|---|---|
| Ventana temprana | 300 s desde t0 |
| T_entry2 | primer trade en orden de ejecución, slot > creación y t ≥ t0 + 10 s |
| H4' | ≥ 1 venta de una identidad de creador (unión del paso 2 de H1) antes de T_entry2 |
| H6' | llenado `(V0 − V) / 793.1e12` ≥ 13.65 %; n/e si V0 ≠ 1.073e15 |
| H7' | ≥ 1 wallet ajena al creador con 1 compra + 1 venta antes de T_entry2 |
| H5 | ≥ 1 wallet ajena al creador compra en el slot de creación |
| H8 | identidades de creador compran ≥ 15 % del supply (10^15) en el slot de creación |
| Retorno neto (a) | `entry2::return_at`: último punto con timestamp ≤ salida; comisión del trade T_entry2 (fee + creator fee, bps) a la entrada y a la salida; n/e si la curva se completa en o antes de la salida o si la salida cae fuera de lo descargado |
| Retorno neto (b) | `entry2::return_b` (definición de la validación 2) |
| Horizontes | +5 s, +30 s, +5 min, +30 min (primario), +60 min |
| Pump-y-caída (1 h) | pico ≥ 2× el precio inicial y caída > 30 % del pico al cierre, en [t0, t0 + 1 h) |

**Código de cálculo** (sha256 en `a3125fb`). Estos archivos no se modifican mientras dure la recolección; si hiciera falta, se avisa antes y se documenta aquí:

```
6d9dad740e1ef05f0dea8df8d2973a76df437cf60384258503238e06a2689100  src/entry2.rs
5719b27160fc8d65f654e0f2e368d0b3fb4a270a06926ce35e8d4fbedf75e97b  src/entry.rs
f152e9537d837342bdf2a5dd0949aac06f774c8899dfd78b0d23203575e31af3  src/v3.rs
aa5f9cf37d257d9ed9937201bca64cc86fc0ec20a9712a320120c4a3e2a3032d  src/h1.rs
9e45d281199bf1f69693c662ffbaf9a27893cdf54f350d31e88ee3ef03c0235d  src/h1b.rs
7616681ea557975f9f623ac211080f22a93060781eaf4765b0a488d980997207  idl/pump.json
```

`src/store.rs`, `src/tx.rs`, `src/idl.rs` y `src/check.rs` pueden cambiar en la entrada y salida (base, presupuesto, recolección), pero no en lo que calculan. La garantía es que las salidas de referencia sobre `marxol.db` sigan idénticas byte a byte (entry2 de todos los tramos, entry, h1, h1b y `check --offline` de los 973 mints). Si el IDL cambia por un despliegue del programa, se actualiza con el procedimiento de docs/referencia_tecnica.md §6 y se anota aquí con la fecha.

## Regla sobre los datos nuevos

- Todo lo que entre en `marxol_vivo.db` es **fuera de muestra** respecto a los 973 tokens de `marxol.db` (piloto, V1, V2 y V3).
- Cada token lleva su fecha de captura (`vivo_tokens.captured_at`), su hora UTC y la versión de este congelado.
- **No se ajusta nada contra estos datos** (umbrales, definiciones, filtros, selección de tokens) sin avisar antes al dueño.
- Cualquier evaluación que decida algo sobre ellos (fase 5) necesita un pre-registro escrito antes de mirar el resultado.
- La recolección calcula y guarda D2 y los retornos de cada token con este código (agregación previa al borrado de crudos), pero no se resumen ni se leen los resultados hasta ese pre-registro.
