# Informe de la tarea nocturna 2 (2026-10-22 21:05 UTC – 2026-10-23 02:10 UTC)

No hay commit ni push. No se ha impreso ni copiado ninguna clave: la de Alchemy se lee de `.env` con un envoltorio equivalente a `run/collect.sh`, y los errores salen por `rpc::redact`. No se ha leído ningún retorno de `marxol_vivo.db`. `marxol.db` se ha abierto solo en lectura (sha256 al final).

## A. Por qué se paró collect

**Causa: un `poweroff` explícito, no un fallo de `collect`.** Lo mismo pasó en la parada anterior.
- `journalctl -b -1`, a las 19:02:22 hora local (**17:02:22 UTC**):
  - `systemd-logind: poweroff requested from client PID 37735 ('poweroff') (unit user@1000.service)`.
  - Después, `session-3.scope: Killing process 28438 (collect.sh)` y `28440 (marxol) with signal SIGTERM`.
- `last -x`: `shutdown system down 19:02 – 21:09` (local). La máquina estuvo apagada de 17:02 a 19:09 UTC y volvió a arrancar sin collect.
- **El hueco de la mañana tiene la misma causa**: `journalctl -b -2` a las 06:39:48 local (**04:39:48 UTC**) registra `poweroff requested from client PID 92218 ('poweroff') (unit user@1000.service)`, y systemd mata `collect.sh` y `marxol` con SIGTERM. La máquina estuvo apagada hasta las 13:22 UTC, y collect no volvió hasta las 16:24 UTC, al lanzarlo a mano.
- Ninguna de las dos fue una suspensión ni una señal de otro origen. `collect.log` no tiene `=== salida rc=` porque el bucle de `collect.sh` murió a la vez.
- **Qué proceso ejecutó `poweroff`: no consta.** El PID no aparece en el journal. Solo se sabe que el proceso estaba en el cgroup del gestor de usuario (`user@1000.service`), no en una sesión de login.
  - En las dos ocasiones fue poco después de cerrarse una sesión SSH (19:01:51 → 19:02:22; 06:25–06:26 → 06:39).
  - En ese gestor también corre el servicio de usuario `openclaw-gateway` (ahora en `start-limit-hit`). No hay ninguna prueba de que fuera él.

## B. `TradeEvent` de 382 bytes

**Campo nuevo**: `quote_transfer_fee: u64`, añadido **al final** del `TradeEvent`.
- Fuente: el IDL de `@pump-fun/pump-sdk` 4.2.0, publicado el 2026-10-19 a las 14:28 UTC (y 4.3.0/4.3.1). Descargado de npm sin CU y tratado como datos: no se ejecutó nada.
- Frente a nuestro `idl/pump.json`, que es igual al del SDK 4.1.0 (sha256 `7616681e…`), el único tipo que cambia es `TradeEvent`. No hay eventos ni instrucciones nuevos.
- El IDL de GitHub (`pump-public-docs`) sigue sin el campo: su último commit es del 2026-10-13.
- Significado, según el SDK (`state.ts`, `transferFee.ts`): es la comisión de transferencia **Token-2022 del mint de quote**. En las compras se cobra encima de la entrada y en las ventas se descuenta de la salida. Vale 0 si el quote es SOL.

**Desde cuándo** (verificado contra la chain):
- Despliegues en ProgramData (`B5MvUw…`) en las 30 firmas más recientes: tandas el 18-10 a las 17:02–17:09 y a las 18:21–18:22 UTC (y varias fallidas a las 21:17–21:21), y el 19-10 a las **15:46:16 UTC (slot 458 888 140)** y a las 17:47:02 UTC (slot 458 921 440).
- Trades de la PDA mint-authority justo antes de cada tanda:
  - antes del 18-10 17:02 y del 18-10 18:21: 374 bytes, sin sobrantes;
  - antes del 19-10 15:46: 374 (slot 458 888 128);
  - antes del 19-10 17:47: **382** (slots 458 921 419–430);
  - ahora (slot 460 182 476): 382.
- **Conclusión**: el campo llega con el despliegue del **2026-10-19 15:46:16 UTC, slot 458 888 140**. Lo acotan el último trade de 374 (slot 458 888 128) y el primero de 382 observado (458 921 419), entre los que solo hay ese despliegue.
- En el registro anterior se atribuía a "18/19-10" sin más precisión.

**¿Decodifica bien el parser? Sí.** Se comprobaron 11 trades reales: 6 de antes del cambio (374, 387 y 389 bytes, estos dos últimos por `ix_name`/`shareholders`) y 5 de después (382). Todos son compras; 3 con quote distinto de SOL.

| Comprobación | 374 / sin sobrantes | 382 |
|---|---|---|
| Producto constante: reservas antes (después ∓ importes) frente a después, a 1e-6 | 6/6 | 5/5 |
| `real_token_reserves = virtual − 279,9e12` y, en SOL, `real_sol = virtual − 30 SOL` | 6/6 | 5/5 |
| `fee = ⌈importe × fee_bps / 10⁴⌉` y `creator_fee` igual | 6/6 | 5/5 |
| `CreateEvent` → trade: reservas iniciales ∓ importes = reservas del trade | 6/6 | 5/5 |
| Campos ausentes (`missing`) | 0 | 0 |
| 8 bytes sobrantes = `quote_transfer_fee` | — | 0 en los 5, incluido un quote no SOL (`CARDSccU…`) |

**Por qué es seguro**:
1. El campo va al final, así que el resto de campos decodifica byte a byte igual. El decodificador cuenta los 8 bytes como sobrantes y avisa, sin error.
2. `collect`, `windows`, `prices` y `check` solo usan campos del prefijo: reservas, importes, comisiones, `user` y `creator`.
3. Las comprobaciones de coherencia dan lo mismo antes y después del cambio.

**Amenaza a la validez**, anotada en CLAUDE.md, docs/estado_y_pasos.md y docs/referencia_tecnica.md:
- El retorno congelado no descuenta `quote_transfer_fee`.
- Solo puede afectar a tokens con un quote Token-2022 que la cobre. En las muestras siempre fue 0.
- V1–V3 son anteriores al cambio. Todo `marxol_vivo.db` (rodaje y bloques A y B) es posterior.
- `collect` no guarda el campo. Si se quiere medir, hay que actualizar el IDL; eso es una decisión tuya y no lo he hecho.

**Coste**: **880 CU** de la bolsa manual en 21 peticiones (11 `getTransaction` + 10 listados; `cu_ledger_runs` 12–21), dentro del tope de 3 000. El SDK y el IDL de GitHub se descargaron por HTTP, sin CU.

**Cambios de código para B** (sin commit):
- `marxol scan` acepta `--before <firma>` y `--solo-firmas` (lista firmas sin pedir las tx).
- `scan --json` saca `trade_sizes`: tamaño de cada `TradeEvent` y sus bytes sobrantes en hex.
- `TxEvents` lleva ese campo nuevo.
- `cargo test --release`: 96 tests, todos pasan. Los resultados de V1–V3 no cambian (`banco-fase-c` sigue siendo idéntico byte a byte a resultados/fase_c_banco.txt).

## C. Reinicio de collect

**Reiniciado el 2026-10-22 a las 21:12:32 UTC** como servicio de usuario de systemd. **Lo pararon a mano el 2026-10-23 a las 02:03:54 UTC; no lo he vuelto a arrancar.**

**Instalación**:
- `~/.config/systemd/user/marxol-collect.service`: `Restart=always`, `RestartSec=30`, `StartLimitIntervalSec=0` y `WantedBy=default.target` (habilitado).
- Ejecuta `run/collect-service.sh`, que es `run/collect.sh` sin el bucle (el reinicio lo hace systemd): `run/marxol --vivo-db marxol_vivo.db collect --sample-ppm 4000 --poll-secs 300`, topes por defecto.
- La salida va a los mismos ficheros: `run/collect.out` y `run/collect.log`.

**Linger: no se pudo activar.** `loginctl enable-linger igt` da "Access denied" y `sudo` pide contraseña. Consecuencias:
- Sobrevive a suspensiones y a que se caiga el proceso.
- Sobrevive a que se cierre una sesión SSH, mientras quede alguna sesión abierta (la de tty1 lo estaba).
- **No sobrevive** a cerrar todas las sesiones ni a un apagado. Tras un apagado arranca solo al iniciar sesión, por cualquier vía, porque está habilitado en `default.target`.
- Para que arranque al encender sin iniciar sesión: `sudo loginctl enable-linger igt`.

**Primer ciclo (38), 21:12:32–21:21:36 UTC**: se puso al día del apagado.
- 12 534 firmas en 13 páginas, 51 muestreadas, 50 creaciones, 38 ventanas, 45 precios y 49 agregados; 83 940 CU.
- Sin huecos en el listado (`vivo_gaps` vacía). Los ciclos siguientes bajaron a lo normal: ~2 300–5 700 CU, 1 página y ~150–280 firmas.
- El aviso de bytes de más en `TradeEvent` es el conocido de §B (397 = 389 + 8, por un `ix_name` más largo).

**Mientras funcionó** (21:12:32 → 02:03:54 UTC, 4 h 51 min):
- Ciclos 38–95 (58 ciclos), 331 960 CU y 89 tokens muestreados.
- **El bloque A empezó a las 00:00 UTC con collect en marcha**: 25 ciclos y 11 tokens muestreados hasta la parada.
- 429: 532 reintentos (466 en el primer intento, 54 en el segundo, 10 en el tercero y 2 en el cuarto). Ninguno agotó los 4.
- Libro: 2026-10-22, `collect` 483 220 CU (de 700 000); 2026-10-23, `collect` 112 340 CU hasta la parada.
- `collect-status` a las 02:04 UTC: 200 `creado` + 1 `sin_create`; ventanas completas 184/199, de precio 177/179; huecos 0; punto de reanudación en el slot 460 265 328.

**La parada de las 02:03:54 UTC**: `journalctl --user -u marxol-collect` registra `Stopping…` y `Stopped…` del gestor de usuario. Es una parada ordenada (`systemctl --user stop` o equivalente), no una caída: no hubo `Restart`.
- 2 min 42 s antes se abrió una sesión SSH nueva (`session 4`, pts/1, 02:01:12 UTC).
- **Decisión conservadora**: no vuelvo a arrancarlo. Ya usé el único reinicio autorizado y la parada parece deliberada.
- El servicio sigue *habilitado*, así que volverá a arrancar en el próximo arranque más inicio de sesión. Si no lo quieres, ejecuta `systemctl --user disable marxol-collect`.
- **Mientras siga parado, el bloque A pierde cobertura en tiempo real.** El listado se pone al día al reanudar, como pasó en el ciclo 38.

**Comandos**:
- Parar: `systemctl --user stop marxol-collect`
- Parar y que no vuelva a arrancar solo: `systemctl --user disable --now marxol-collect`
- Arrancar: `systemctl --user start marxol-collect`
- Estado: `systemctl --user status marxol-collect` y `journalctl --user -u marxol-collect`
- **No lances `run/collect.sh` a la vez que el servicio.**

## D. Adenda 2 reescrita

docs/borrador_adenda_2.md, sin commit:
- k = 2 (R1 y R2), con p < 0,005.
- C1 pasa a nota descriptiva, con sus motivos.
- R1 sigue usando el pagador, el mismo que usa GMGN (20/20).

**Decisión conservadora**: no he calculado "la mitad del efecto observado" de R1 y R2. La adenda 1 los registró sin cruzarlos con ningún retorno, y cruzarlos les quitaría esa condición. En su lugar:
- La potencia se da en función del efecto verdadero d. Los grupos se simulan con los retornos agregados de V1–V3 y la prevalencia por conteos (R1 ≈ 64 %; R2 ≈ 44 %, aproximada).
- Como referencia se usa la mitad de los efectos conocidos de C1 (8,3 pts) y de D2 (9,5 pts).

Resultado (resultados/potencia_adenda2_r.txt):
- **Efecto mínimo detectable ≈ 11–12 pts para R1 y R2.**
- Probabilidad de validar con la mitad de un efecto conocido: 0,04–0,16 con 8,3 pts y 0,30–0,41 con 9,5 pts.
- La n no limita. Lo que limita es el suelo de 10 pts del criterio 3.

## E. ¿Cuánto de C1 explican ya H6′ y el progreso?

Fuente: resultados/c1_vs_h6_progreso.txt (scripts/c1_vs_h6_progreso.py). Post hoc, sobre los 438 tokens de V1–V3.

- **Llenado de H6′ y progreso son la misma variable en rangos**: Spearman = +1,000. Medir C1 frente al progreso es medirlo frente al llenado de H6′.
- **Correlación tenencia del creador–llenado**: rho = +0,285 [+0,193, +0,374]. Explica un 8 % de la varianza de rangos. Como variable continua, C1 no es una copia de H6′.
- **Solapamiento C1 × H6′**:

  |  | H6′ sí | H6′ no |
  |---|---|---|
  | C1 sí | 40 | 27 |
  | C1 no | 104 | 267 |

  phi = +0,24. El 60 % de los C1 tienen H6′ y el 28 % de los H6′ tienen C1; 29 de los 67 C1 los descarta ya D2.
- **Retorno a +30 min** (descriptivo):
  - Dentro de "H6′ sí", C1 no separa: −2,2 pts, p = 0,74.
  - Dentro de "H6′ no", separa +9,2 pts (p = 2e-11).
  - Pero si dentro de "H6′ no" solo se comparan tokens con un progreso que C1 pueda alcanzar (≥ 4,71 %, el mínimo entre los C1), la diferencia baja a **+1,6 pts (p = 0,13)**.
- **Lectura**: casi todo el efecto de C1 lo explica el progreso de la curva, que es el llenado de H6′. Lo que queda es pequeño y no se distingue de 0.

## Decisiones ambiguas tomadas

1. **Linger**: `loginctl enable-linger` da "Access denied" y `sudo` pide contraseña. Instalé el servicio de usuario igualmente, porque es la mejor alternativa sin root (detalle en C).
2. **Binario de collect**: el servicio usa `run/marxol`, la misma copia de las 05:08 que corría antes, no el `target/release` recompilado. Así el código que recolecta no cambia.
3. **Descargas sin CU** (npm y GitHub): las entendí permitidas, porque el permiso limita CU y no prohíbe HTTP público. Se trataron como datos.
4. **Muestra de trades**: hay 11 en vez de 10 (uno de más), y todos son compras: la PDA mint-authority solo ve creaciones con compra. No se comprobaron ventas en vivo, aunque los fixtures de V1–V3 con ventas siguen pasando los tests.
5. **R1 y R2**: no se cruzaron con retornos (§D).
6. **Hallazgos registrados** en CLAUDE.md, docs/estado_y_pasos.md (amenaza a la validez y control de C1) y docs/referencia_tecnica.md (`TradeEvent`).

## Lo que no se pudo hacer

- Activar el linger (necesita root): `sudo loginctl enable-linger igt`.
- Saber qué proceso ejecutó los `poweroff`.
- Comprobar ventas con el campo nuevo en vivo.

## Integridad

- sha256 de `marxol.db`: `d513e1f3a0fa0459aad5e89be918d99eab9d39d47005b0dc437bc09479fb8dae`, el mismo que en el informe anterior.
- `collect-status` (02:04 UTC) abrió `marxol_vivo.db` en escritura sin otro proceso conectado: SQLite volcó el WAL al fichero, igual que la noche anterior. No cambia ningún dato.
