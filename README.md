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
