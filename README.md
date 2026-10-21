<p align="center">
  <img src="assets/marXol.jpg" alt="marXol" width="400">
</p>
# marXol

CLI en Rust para análisis on-chain de operadores en **pump.fun** (Solana). Proyecto hermano de [marXi](https://github.com/idrode/MarXi), misma filosofía, red distinta.

No es un bot de trading: es una herramienta de búsqueda y análisis de solo lectura. No firma transacciones, no tiene wallet operativa, no custodia claves. Sin interfaz gráfica ni TUI — CLI directa desde el inicio.

## Qué hace

Vigila wallets/identidades que crean tokens en pump.fun ("operadores"), indexa su historial de lanzamientos y cambios de identidad, y busca patrones que distingan actividad orgánica temprana de bombeo artificial o de bots.

El diseño completo — modelo de datos, hallazgos verificados contra mainnet, hipótesis pre-registradas (H1/H1b) y qué queda pendiente de confirmar — vive en [`CLAUDE.md`](./CLAUDE.md), que es la fuente de verdad del proyecto.

## Estado

En desarrollo activo. El indexado de identidad (creación, cambios de creador, graduación) ya está probado contra mainnet; la señal de actividad orgánica temprana (H1) está en construcción.

## Uso

```
marxol verify                        # comprueba los supuestos de CLAUDE.md contra la chain real
marxol global                        # lee la cuenta Global (parámetros económicos en vivo)
marxol curve <mint>                  # lee la bonding curve de un token
marxol tx <firma>                    # decodifica los eventos pump.fun de una transacción
marxol scan --address <addr>         # recorre transacciones recientes y muestra sus eventos
marxol index --address <creator>     # indexa en SQLite local los eventos de una dirección
marxol operator <creator>            # informe de un operador (identidad = campo `creator`, no el payer)
marxol stats                         # recuento de filas de la base local
```

Añade `--json` para salida en JSON, o `--rpc <url>` para usar un proveedor propio (por defecto, RPC público de Solana).

### Comprobación previa de un token: `marxol check`

Aplica el filtro de descarte **D2** de la validación 3: ≥ 2 de H4' (el creador vendió), H6' (curva llenada ≥ 13.65 %) y H7' (≥ 1 wallet 1 compra + 1 venta), evaluadas con los trades anteriores a T_entry2 y con las definiciones congeladas de `CLAUDE.md` sección 8. Lee la cadena en solo lectura: la bonding curve hasta la creación y las transacciones de los 5 primeros minutos. Tiene un tope de 400 peticiones por llamada (16 000 CU en Alchemy); el consumo se guarda en `rpc_usage`.

```
marxol --rpc <url> check https://pump.fun/coin/<mint> --provider alchemy
marxol check <mint> --offline        # sin red, con los datos ya descargados en marxol.db
```

```
VEREDICTO: EVITAR  (D2: 2 de 3 banderas; descarta con 2 o más)
  H4' el creador vendió:            SÍ  (0.48 % del supply vendido por identidades de creador)
  H6' curva llenada ≥ 13.65 %:      no  (0.04 % de las reservas reales de token vendidas)
  H7' wallets 1 compra + 1 venta ≥ 1: SÍ  (wallets ajenas al creador con 1 + 1: 1)
```

- **Veredictos**: `EVITAR` (D2 descarta), `SIN BANDERAS DE D2` (pasa) y `NO EVALUABLE`. Este último sale cuando no hay T_entry2, cuando el token es demasiado antiguo o activo para el tope, o cuando H6' es n/e y solo se cumple una de H4'/H7'.
- **Código de salida**: 0 con veredicto, 3 si no es evaluable y 1 si falla la lectura.
- **Avisos**:
  - D2 solo descarta; no hay veredicto de compra.
  - Los tokens que pasan D2 siguieron con retorno mediano negativo en las validaciones (−5.6 % a +30 min en la validación 3).
  - En los tokens mayhem, D2 casi no actúa y su retorno medido es peor.
  - Está validado en tres franjas de 8 minutos.
  - No es consejo financiero.

## Configuración

```
cp .env.example .env
```

Rellena `ALCHEMY_API_KEY` y/o `HELIUS_API_KEY` si quieres usar un proveedor dedicado en vez del RPC público (ver `CLAUDE.md` sección 7 sobre límites de cada uno). `.env` nunca se versiona.

## Build

```
cargo build --release
cargo test
```
