# Exploración de operadores (2026-09-29) e hipótesis H11–H13

Movido **literalmente** desde `CLAUDE.md` (antigua sección 8) el 2026-10-21. Exploración post hoc, sin veredicto; H11–H13 siguen sin pre-registrar.

Las referencias a "sección N" usan la numeración de `CLAUDE.md` anterior al 2026-10-21: secciones 3–7 y 9 → docs/referencia_tecnica.md; 8 → docs/hipotesis_y_resultados.md, docs/validaciones_congeladas.md y docs/exploracion_operadores.md; 10 → docs/estado_y_pasos.md; 11 → `CLAUDE.md`. El detalle de líneas cerradas está en docs/historico_lineas_cerradas.md.

---

### EXPLORACIÓN (2026-09-29): identidad del operador y persistencia de patrones — sin veredicto

**Exploración, no validación.** Los resultados de los 337 ya se habían visto, así que nada de esta sección decide ni confirma nada. Solo `marxol.db`, sin red.
- **Muestra**: los 377 tokens (40 del piloto + 337 de validación), todos con ventana completa. Operador = campo `creator` del `CreateEvent`, no el payer (98 tokens tienen `user ≠ creator`).
- **Banderas**:
  - H5 y H8 recalculadas para los 377, porque solo dependen del slot de creación.
  - H4' solo existe en los 279 tokens con T_entry2.
  - Pump-y-caída en 1 h, con la definición vigente, para los 377.

**1. Descriptivo**:
- 377 tokens de **272 creators distintos**. 248 creators lanzan 1 solo token; **24 repetidores** (≥ 2 tokens) suman **129 tokens (34 %)** en solo 8 min.
- Distribución de tokens por creator: 1 (248), 2 (8), 3 (4), 4 (3), 5 (2), 6, 7, 8 (2), 10, 16, 24.
- Los 5 mayores suman 66 tokens (18 % de la muestra):
  - `HSF95wu9aG…`: 24 tokens con **el mismo símbolo en 40 s**.
  - `DsT2JZ2mKq…`: 16 tokens, mismo símbolo.
  - `A9emkNzaqK…`: 10.
  - `EjcwXG6ywz…`: 8, todos mayhem.
  - `Dz1JFxDUEW…`: 8 en 16 s, mismo símbolo.
- Todos los repetidores pagan sus propias creaciones (`user = creator`, un solo payer por creator). Los grandes son **fábricas de lanzamientos en serie**, no operadores con proyectos distintos.
- **Mayhem**: 42 de los 129 tokens de repetidores son mayhem, frente a 10 de 248 en creators de un solo token. 10 de los 24 repetidores tienen algún mayhem y 9 lanzan solo mayhem. Ser repetidor y ser mayhem están muy confundidos.

**2. Consistencia dentro de cada operador**:

| Tasa por token | Todos (n) | Creator de 1 token | Tokens de repetidores |
|---|---|---|---|
| H5 | 0.41 (377) | 0.58 (248) | 0.09 (129) |
| H8 | 0.09 (377) | 0.14 (248) | **0.00** (129) |
| H4' | 0.32 (279) | 0.39 (211) | 0.10 (68) |
| Pump-y-caída | 0.20 (377) | 0.25 (248) | 0.10 (129) |

- Los tokens de repetidores llegan a T_entry2 con menos frecuencia (53 % frente a 85 %). Casi todo el efecto viene de las fábricas grandes: `HSF95wu9aG…` llega en 1 de 24. Sin las 5 mayores, llegan el 89 %.
- **Primer token → siguientes** (tokens siguientes agregados):

| Bandera | Primer token = sí → siguientes | Primer token = no → siguientes |
|---|---|---|
| H5 | 2/4 (2 creators) | 7/101 (22 creators) |
| H4' | 2/3 (2 creators) | 2/41 (18 creators) |
| Pump-y-caída | 4/9 (3 creators) | 6/96 (21 creators) |
| H8 | 0 creators | 0/105 (24 creators) |

- **Unanimidad** (todos los tokens de un creator con la misma bandera), frente a lo esperado al barajar las banderas entre los tokens de repetidores (1 000 permutaciones):

| Bandera | Unánimes / creators | Esperado (p5–p95) |
|---|---|---|
| H5 | 16/24 | 15.9 (14–18) |
| H8 | 24/24 | 24.0 (trivial: nadie la tiene) |
| H4' | 17/19 | 13.8 (13–15) |
| Pump-y-caída | 16/24 | 14.9 (13–17) |

- Solo H4' queda por encima del percentil 95 del azar, con pocos casos y muchos n/e.
- Sin las 5 mayores quedan 19 repetidores con 63 tokens: H5 0.14, H8 0.00, H4' 0.07, pump-y-caída 0.17.
- **Lanzamiento previo del mismo creator en la muestra** (dato que se conoce antes de entrar): 105 tokens con un lanzamiento previo, pump-y-caída 10 % y H5 9 %. 272 tokens sin lanzamiento previo, pump-y-caída 24 % y H5 53 %.

**3. ¿Es medible "persisten los patrones por operador"?**
- **Con estos datos, no.** Los repetidores tienen las banderas casi siempre en 0, así que la unanimidad es sobre todo trivial (todo ceros) y cuadra con el azar. Los casos con "primer token = sí" son 2–3 creators: no hay variación que medir.
- Lo único que se ve es un efecto **entre** operadores (repetidor frente a un solo token), no una persistencia **dentro** de cada uno. Y está confundido con mayhem y con las fábricas de un mismo símbolo.
- **n necesario, orden de magnitud**:
  - Comparar la tasa en los tokens siguientes según el primero (del orden del 50 % frente al 7 %, como aquí) con α = 0.05 y potencia 0.8 pide del orden de **15–20 creators con el primer token marcado** por grupo.
  - Con una tasa de H5 del ~10 % en el primer token de un repetidor, eso son **~150–200 repetidores**.
  - En 8 minutos aparecieron 24, pero muchos serían los mismos si se alarga la ventana. Hace falta indexar **horas o días** de creaciones: al ritmo observado (~0.77/s), del orden de 5 000–10 000 creaciones.
  - Seguir después a cada repetidor con `index --address <creator>` sería más barato que recorrer todo el programa.
- **Aviso para la validación ya hecha**: 64 de los 251 tokens de sus tablas son de repetidores (210 creators distintos). Los tokens de un mismo creator no son independientes (seudorreplicación moderada), y Fisher los trata como si lo fueran. No cambia el veredicto congelado, pero hay que controlarlo en la siguiente ráfaga (H13).

**Hipótesis nuevas (sin evaluar; para la segunda ráfaga, con pre-registro previo de umbrales y criterio):**
- **H11** "lanzamiento previo del creator": un token cuyo `creator` ya había creado ≥ 1 token en las N horas **[P]** anteriores a su creación tiene **menor** tasa de pump-y-caída que uno sin lanzamientos previos. Se conoce antes de entrar. Control obligatorio: estratificar por mayhem y por "mismo símbolo que el lanzamiento previo", porque aquí está confundido con fábricas de lanzamientos en serie.
- **H12** "persistencia de dev-sell": si en el token anterior del mismo creator el dev vendió antes de T_entry2, en el siguiente también. Es la única bandera que en la exploración queda por encima del azar en unanimidad (17/19 frente a 13.8 esperado), con n muy pequeño.
- **H13** (control de seudorreplicación, no es hipótesis de señal): repetir la prueba de H5, H6', H7' y H8 en la segunda ráfaga también con **un token por creator** (el primero). Solo se da por buena una señal si mantiene la dirección y el criterio en ese análisis.
