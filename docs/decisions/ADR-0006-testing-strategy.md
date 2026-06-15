```yaml
titulo: "ADR-0006: Estrategia de Testing"
tipo: decision
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-14"
fecha_modificacion: "2026-06-14"
version: "0.1.0"
estado: aceptado
tags: [decision, testing, vttest, proptest, criterion, ci]
```

# ADR-0006: Estrategia de Testing

## Contexto

El proyecto necesita una estrategia de testing que cubra
los tres niveles necesarios: componentes individuales
(unit), integracion entre componentes, y validacion
contra el estándar VT100/xterm.

Los cinco terminales de referencia analizados usan
distintos niveles de testing. Alacritty y Warp tienen
unit tests extensivos. WezTerm tiene integration tests
distribuidos por crate. Ninguno de los tres usa
property-based testing ni benchmarks formales en sus
repos públicos.

La pregunta es que niveles de testing implementar y
con que herramientas.

## Decision

Se implementa testing en **cuatro niveles complementarios**:

1. **Unit tests en cada modulo.** Tests deterministas
   para invariantes pequenas. Viven en `mod tests`
   dentro del mismo archivo que el código. Cubren:
   - `src/grid/tests.rs`: ring buffer, scroll, reflow,
     resize.
   - `src/parser/tests.rs`: secuencias CSI, OSC, ESC.
   - `src/cursor/tests.rs`: posición, wrap, save/restore.
   - `src/selection/tests.rs`: 4 modos de selección.

2. **Integration tests con PTY real.** Viven en `tests/`.
   Usan `nix::pty::openpty` (o `portable-pty` cuando se
   añada soporte multiplataforma) para crear un PTY real,
   lanzan `bash -c "comando"`, escriben bytes al master,
   y validan que el grid se actualiza correctamente.
   Casos clave: arranque del shell, envio de input,
   lectura de output, resize, exit del child.

3. **Property-based testing con proptest.** Para
   invariantes del grid que son faciles de enunciar pero
   tediosas de cubrir con casos concretos:
   - Scroll siempre preserva el contenido.
   - Wrap se cancela con cualquier movimiento de cursor.
   - Reflow mantiene el contenido total (caracteres no
     se pierden ni se duplican).
   - DECSTBM respeta los margenes.
     Las estrategias de generacion (`proptest!` blocks)
     se escriben para cada invariante.

4. **Validacion visual con vttest y esctest.** vttest
   (Thomas Dickey) es la referencia canonica para VT100/
   VT220. Se ejecuta manualmente durante el desarrollo
   en las categorías 1 (cursor movement), 2 (screen
   features), 6 (VT102 features) y 8 (VT102 mode).
   esctest (George Nachman) automatiza casos que vttest
   no cubre. En CI se ejecuta vttest solo cuando hay
   cambios en `src/parser/`, no en cada commit.

Adicionalmente:

- **Benchmarks con criterion** en `benches/`:
  - `parser_throughput`: bytes/segundo del parser vte.
  - `scroll_latency`: tiempo de scroll de N lineas.
  - `render_time`: tiempo de render de un frame en
    diferentes tamanos (80x24, 200x50).
  - `resize_time`: tiempo de resize del grid.
    Targets del MVP: >100 MB/s parser ground, <10us
    scroll, <16ms render en 80x24. Targets de produccion
    en Fase 5: >500 MB/s, <1us, <2ms respectivamente.

- **CI con GitHub Actions** (workflow en `.github/
workflows/ci.yml`):
  - Job `lint`: `cargo fmt --check` + `cargo clippy --
-D warnings` en cada PR y push a main.
  - Job `test`: `cargo test` en Linux (ubuntu-latest).
  - Job `test-multi`: `cargo test` en macOS y Windows
    cuando se añadan (Fase 5).",
  - Job `bench`: `cargo bench` en nightly solo en
    commits a main, con `cargo-criterion` para
    detectar regresiones >10%.

## Justificacion

1. **Unit tests son la base, no opcional.** Alacritty
   tiene >300 unit tests en `alacritty_terminal/`. WezTerm
   tiene cientos distribuidos por crate. El proyecto
   sigue la misma práctica desde el inicio.

2. **Integration tests con PTY real son necesarios.**
   Sin un test que arranque bash y valide el grid, no
   hay forma de saber que el flujo end-to-end funciona.
   El setup es ~30 lineas con nix y se ejecuta en <1s.

3. **proptest es una innovacion del proyecto.** Ninguno
   de los 3 terminales Rust lo usa publicamente. El
   proyecto lo introduce para encontrar bugs que los
   unit tests manuales no cubren, especialmente en
   combinaciones de operaciones.

4. **vttest es la verdad definitiva.** Sin ejecutar
   vttest contra el emulador, no se puede reclamar
   conformidad VT100. El proyecto adopta vttest
   categoría 1-4 como objetivo del MVP, y categoría
   5-11 en Fase 5.

5. **criterion para detectar regresiones.** Sin
   benchmarks, una optimizacion puede empeorar el
   rendimiento sin que nadie lo note. criterion con
   cargo-criterion detecta automaticamente.

6. **GitHub Actions es gratuito para proyectos
   públicos** y tiene runners Linux/macOS/Windows
   listos.

## Alternativas Consideradas

| Alternativa                       | Pros                     | Contras                                              | Veredicto                                       |
| :-------------------------------- | :----------------------- | :--------------------------------------------------- | :---------------------------------------------- |
| Solo unit tests                   | Simple                   | No cubre integracion, no detecta bugs de interaccion | Rechazada                                       |
| Unit + integration (sin proptest) | Suficiente para MVP      | No cubre combinaciones raras                         | Parcial, MVP; proptest se agrega en Fase 0      |
| proptest en todo                  | Coverage máxima          | Estrategias complejas, lento                         | Solo en domain (grid, parser)                   |
| Mockall para todo                 | Sin I/O real             | Mocks son fragiles, divergen del comportamiento real | Solo en bordes, no en core                      |
| quickcheck en vez de proptest     | Maduro                   | proptest es mas activo, mejor Shrinking              | **proptest**                                    |
| Iai (benchmarks sin tiempo real)  | Estable en CI            | No captura variabilidad del usuario final            | **criterion** (complementado con iai en Fase 5) |
| Travis CI                         | Maduro                   | GitHub Actions es nativo y mas barato                | **GitHub Actions**                              |
| GitLab CI                         | Similar a GitHub Actions | Requiere self-hosting para repos privados            | N/A (proyecto no usa GitLab)                    |

## Consecuencias

### Positivas

- Cobertura en multiples niveles: bugs se detectan
  en la capa correcta (unit para invariantes,
  integration para flujos, vttest para conformidad).
- CI automatico: cada PR se valida sin intervencion
  manual.
- Benchmarks detectan regresiones de rendimiento.
- property-based testing encuentra bugs que los
  tests manuales no anticipan.

### Negativas

- **Tiempo de CI.** Unit + integration + lint +
  clippy toma ~3-5 min en Linux. Los benchmarks
  en nightly pueden tomar 10+ min. El proyecto
  acepta esto.
- **vttest es interactivo.** La automatizacion
  completa es dificil. La estrategia adoptada
  es ejecutar manualmente + complementar con
  esctest, lo que deja algunos tests sin
  automatizar.
- **proptest requiere escribir estrategias.** Las
  estrategias no son triviales: generar operaciones
  de scroll que respeten la scroll región requiere
  invariantes en el generador.
- **criterion en CI es ruidoso.** Los runners
  compartidos tienen varianza alta. Se requiere
  alert-threshold del 10-20% para evitar falsos
  positivos.

### Mitigacion

- Los tests se organizan en `#[cfg(test)] mod
tests` dentro de cada archivo, lo que permite
  correr subsets con `cargo test <module>`.
- vttest se automatiza con `expect` scripts en
  `tests/vttest/` que envian respuestas al menu
  interactivo.
- Las estrategias de proptest empiezan simples
  (operaciones unitarias) y se extienden
  progresivamente.
- criterion usa `--output-format bencher` y
  `cargo-criterion` con threshold del 15% para
  reducir falsos positivos.

## Referencias

- docs/prompts/iter-06-investigacion-D.md
  (investigacion completa, 827 lineas, 15 URLs
  verificadas HTTP 200).
- docs/specs/testing-strategy.md (spec técnica
  completa, complemento de este ADR).
- docs/research/04-ansi-parser.md (vttest
  categorías).
- docs/research/05-terminal-grid.md (invariantes
  del grid para proptest).
- Alacritty CI: .github/workflows/ci.yml en el
  repo de Alacritty.
- WezTerm tests: distribuidos por crate en el
  monorepo.
- https://crates.io/crates/proptest
- https://crates.io/crates/criterion
- https://invisible-island.net/vttest/
- https://github.com/MarcusJohnson91/esctest

## Cambios

| Version | Fecha      | Cambios                                                          |
| :------ | :--------- | :--------------------------------------------------------------- |
| 0.1.0   | 2026-06-14 | Primer borrador. Decision adoptada. 4 niveles + benchmarks + CI. |
