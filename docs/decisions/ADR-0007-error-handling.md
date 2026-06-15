```yaml
titulo: "ADR-0007: Error Handling y Robustez"
tipo: decision
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-14"
fecha_modificacion: "2026-06-14"
version: "0.1.0"
estado: aceptado
tags: [decision, errores, robustez, panic, anyhow, thiserror, recovery]
```

# ADR-0007: Error Handling y Robustez

## Contexto

El proyecto necesita definir una estrategia consistente de
manejo de errores. Las opciones son:

1. Solo `anyhow` (como WezTerm): simple, contexto
   dinamico, sin tipos en domain.
2. Solo `thiserror` (como algunas librerias): tipos
   propios, verbose en main.
3. Combinacion `anyhow` + `thiserror` (como Warp):
   `anyhow` en bordes (main, modulos que cruzan
   capas), `thiserror` en domain (modulos con tipos
   `Error` propios).

Ademas, hay decisiones sobre panic handling (catch_unwind
vs dejar que panique), logging (log vs tracing), recovery
(que pasa si el child muere), y shutdown (Ctrl+D, SIGTERM).

## Decision

Se adopta la combinación **anyhow + thiserror**:

- **`anyhow::Result<T>` en bordes de aplicación.**
  Usado en `main.rs`, en handlers de UI (procesamiento
  de WindowEvent), y en modulos que cruzan capas
  (ej: `event_loop.rs` que conecta GUI con PTY).
- **`thiserror` en modulos de domain.** Define tipos
  `Error` propios con `#[derive(thiserror::Error)]`:
  - `src/pty/error.rs`: `PtyError` con variantes
    `Open`, `Fork`, `Exec`, `Io`, `Closed`.
  - `src/parser/error.rs`: `ParserError` con
    variantes `InvalidSequence`, `UnhandledCsi`,
    `UnhandledEsc`.
  - `src/grid/error.rs`: `GridError` con variantes
    `OutOfBounds`, `InvalidResize`.
    Conversion automatica entre variantes vía `#[from]`.

**Panic handling:** no se usa `catch_unwind`. Se
configura un panic hook custom en Fase 5 (similar a
WezTerm) que muestra notificacion al usuario con el
backtrace y permite copiar al clipboard. En MVP, el
default de Rust (imprimir en stderr y abortar) es
aceptable.

**Logging:** se usa `tracing` (no `log`). Configuracion
con `tracing-subscriber`:

- `error`: solo fallos recuperables.
- `warn`: situaciones anormales pero manejadas (ej:
  child exit, PTY close).
- `info`: arranque, configuracion, primer render.
- `debug`: detalles del flujo PTY I/O.
- `trace`: cada byte del PTY (solo en dev).

**Recovery:**

- Si el child muere, el emulador muestra
  `[Proceso terminado: código N]` en la pantalla y
  espera input del usuario. No sale.
- Si el PTY se cierra, se intenta reabrir. Si falla,
  se sale con error.
- Si OpenGL falla al iniciar (wgpu devuelve error), se
  muestra error fatal y se sale. No hay fallback a
  software en MVP.
- Si el font no carga, se usa font de sistema por
  defecto (DejaVu Sans Mono en Linux, Menlo en macOS,
  Consolas en Windows).

**Shutdown graceful:** secuencia de cierre al recibir
`CloseRequested`, `SIGTERM`, o `Ctrl+D` (EOF):

1. Enviar `SIGHUP` al child process.
2. Esperar hasta 100ms.
3. `Drop` del struct PTY (que envia SIGHUP en su
   `Drop` impl, como hacen los 3 terminales
   analizados).
4. Salir con código 0 (exito) o 1 (error).

## Justificacion

1. **Warp documenta y recomienda el patron
   anyhow+thiserror.** Es el patron mas extendido en
   la comunidad Rust para proyectos no-triviales.

2. **`anyhow` simplifica los bordes.** En `main.rs` y
   en handlers de UI, los tipos de error son
   heterogeneos y agregar contexto con `.context()`
   es mas util que tipar.

3. **`thiserror` permite API limpia en domain.** Si
   `parser.advance()` falla con `ParserError`, el
   caller sabe exactamente que variante y puede
   decidir que hacer (loguear, recovery, panic).

4. **Sin `catch_unwind` en MVP.** Alacritty y Warp no
   lo usan. Solo WezTerm tiene un panic hook custom;
   se evalua agregar uno en Fase 5.

5. **`tracing` sobre `log`.** `tracing` permite spans
   estructurados, lo que es util para diagnosticar
   problemas de performance y entender el flujo de
   datos entre hilos.

6. **Recovery de child muerte es estándar en los 3
   terminales.** Los 3 detectan vía `SIGCHLD` (signal
   pipe) y muestran mensaje al usuario.

7. **Shutdown con SIGHUP es el patron Unix.** Los 3
   terminales analizados envian SIGHUP en `Drop` del
   struct PTY. El proyecto sigue el mismo patron.

## Alternativas Consideradas

| Alternativa                          | Pros                                            | Contras                                               | Veredicto                              |
| :----------------------------------- | :---------------------------------------------- | :---------------------------------------------------- | :------------------------------------- |
| Solo anyhow                          | Simple, sin tipos                               | Pierde información de error en domain                 | Rechazada                              |
| Solo thiserror                       | Tipos en todo                                   | Verbose en main.rs, agrega boilerplate                | Rechazada                              |
| **anyhow + thiserror**               | Mejor de ambos mundos, patron recomendado       | Requiere disciplina para saber cuando usar cada uno   | **Seleccionada**                       |
| anyhow + snafu (en vez de thiserror) | snafu tiene mejor API para errores contextuales | thiserror es mas estándar, mejor documentado          | thiserror                              |
| `eyre` en vez de anyhow              | Mejor presentacion de errores                   | anyhow es mas usado, mejor integracion con ecosistema | anyhow                                 |
| `catch_unwind` en main               | Evita abort total                               | Enmascara bugs, dificulta debugging                   | Descartado para MVP, revisar en Fase 5 |
| Panic hook custom (como WezTerm)     | UX mejor: notificacion al usuario               | Requiere UI thread, agrega complejidad                | Fase 5, no MVP                         |
| `log` crate en vez de tracing        | Maduro, simple                                  | Sin spans, limitado para multihilo                    | tracing                                |
| `slog` en vez de tracing             | Maduro, estructurado                            | Menos activo, tracing gana en adoption                | tracing                                |
| Reintentar al fallar child           | Automatico                                      | Puede confundir al usuario, no permite inspeccionar   | Mensaje y espera, no auto-retry        |

## Consecuencias

### Positivas

- API consistente: errores tipados en domain,
  contextuales en bordes.
- Mensajes utiles al usuario: `.context("no se pudo
abrir el PTY")` da información accionable.
- Logging estructurado con tracing permite
  diagnosticar problemas de multihilo.
- Shutdown graceful: el child recibe SIGHUP antes
  de que el emulador salga, evitando procesos
  huerfanos.

### Negativas

- **Requiere disciplina.** Hay que decidir para cada
  función si devuelve `Result<T, MyError>` (domain) o
  `anyhow::Result<T>` (borde). El proyecto documenta
  esto en la guia de estilo.
- **panic hook custom se difiere a Fase 5.** En MVP,
  un panic resulta en abort con backtrace, que es
  feo pero funcional.
- **Recovery de OpenGL no tiene fallback.** Si wgpu
  falla, el emulador no arranca. Esto puede ser
  frustrante en hardware antiguo; se evalua en
  Fase 5.

### Mitigacion

- La guia de estilo (en este ADR y en el doc maestro 06) documenta cuando usar `Result<T, MyError>` vs
  `anyhow::Result<T>`.
- En Fase 5, se agrega un panic hook custom
  similar a WezTerm.
- En Fase 5, se evalua agregar fallback a software
  rendering (no prioritario).

## Referencias

- docs/prompts/iter-06-investigacion-E.md
  (investigacion completa, 526 lineas, 7 URLs
  verificadas HTTP 200).
- docs/specs/error-handling.md (spec técnica
  completa, complemento de este ADR).
- docs/research/01-pty-shell.md (errores de spawn).
- docs/research/05-terminal-grid.md (panic en wrap
  y edge cases).
- https://docs.rs/anyhow/latest/anyhow/
- https://docs.rs/thiserror/latest/thiserror/
- https://docs.rs/tracing/latest/tracing/
- Alacritty main: alacritty/src/main.rs (uso de
  `Result<(), Box<dyn Error>>`).
- WezTerm panic hook: busqueda de `set_hook` en
  wezterm-gui/src/.
- Warp error handling: combinación anyhow +
  thiserror documentada en su repo.

## Cambios

| Version | Fecha      | Cambios                                                                          |
| :------ | :--------- | :------------------------------------------------------------------------------- |
| 0.1.0   | 2026-06-14 | Primer borrador. Decision adoptada. anyhow+thiserror, tracing, sin catch_unwind. |
