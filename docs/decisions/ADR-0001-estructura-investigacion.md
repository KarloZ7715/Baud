```yaml
titulo: "Estructura de Investigacion del Proyecto"
tipo: decision
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-13"
fecha_modificacion: "2026-06-14"
version: "0.7.0"
estado: borrador
tags: [decision, investigacion, fases, roadmap, terminal]
```

# ADR: Estructura de Investigacion del Proyecto

## Contexto

Se esta investigando como construir un emulador de terminal en
Rust. La investigacion debe producir documentacion técnica
confiable que sirva de base para la implementación.

Sin una estructura clara de investigacion, los documentos se
vuelven incompletos, contradictorios o superficiales.

## Decision

Definir 7 iteraciones de investigacion, donde cada una produce
un documento técnico verificado. Las iteraciones se ejecutan en
orden pero permiten retroceso cuando se descubre algo nuevo.

---

## Iteraciones definidas

### Iteracion 0: Fundamentos (COMPLETADA)

**Documento:** `research/00-fundamentos.md` (353 lineas)

**Que investigo:**

- Que es una terminal (TTY, PTY, shell, 3 capas)
- Como funciona el flujo de datos (tecla -> PTY -> shell -> output)
- Secuencias ANSI (ECMA-48, VT100)
- Que es Rust (historia, ownership, features)
- Por que Rust para terminales

**Que descubrimos:**

- La terminal NO es un solo programa. Son 3 capas independientes
- El VT100 (1978) era monochrome, sin color ni function keys
- Rust fue creado por Graydon Hoare en 2006, 1.0 en mayo 2015
- Todos los terminales modernos usan secuencias ANSI heredadas
del hardware de los 70s

**Fuentes:** Man pages, articulos de The Valuable Dev y
The Linux Field Guide, documentacion oficial de Rust

---

### Iteracion 1: PTY y Shell Management (COMPLETADA)

**Documento:** `research/01-pty-shell.md` (448 lineas)

**Que investigo:**

- Estandar POSIX para creacion de PTYs
- openpty vs forkpty (analisis profundo con código real)
- Como 7 terminales implementan PTY
- I/O no-bloqueante, patron de senales
- Decision de crates

**Que descubrimos (hallazgos criticos):**

- TODOS los terminales usan openpty. NINGUNO usa forkpty
- Alacritty usa rustix_openpty, Warp usa nix::pty::openpty
- La secuencia correcta es: openpty + Command + pre_exec
(setsid, TIOCSCTTY, dup2, close FDs, reset signals)
- forkpty es un atajo sin control, no para produccion
- forkpty hace openpty + fork + login_tty junto, impidiendo
configuracion individual de cada paso

**Fuentes verificadas en código:**

- Alacritty: tty/unix.rs
- Warp: local_tty/unix.rs
- WezTerm: portable-pty crate
- Man pages: pty(7), forkpty(3), setsid(2), ioctl_tty(2)
- crates.io: rustix-openpty v0.2.0, nix v0.29, portable-pty

---

### Iteracion 2: Rendering de Texto (COMPLETADA)

**Documento:** `research/02-rendering.md` (525 lineas)

**Que investigo:**

- Que librerias usan los terminales para rendering
- Glyph atlas (1024x1024 texture)
- Glyph cache (HashMap, pre-carga ASCII)
- Mapeo de grid a coordenadas de pantalla
- Damage tracking
- OpenGL vs WebGPU

**Que descubrimos (hallazgos criticos):**

- Alacritty NO usa wgpu ni fontdue (error común extendido)
- Alacritty usa: winit v0.30 + glutin (OpenGL) + crossfont
- crossfont fue creado POR el equipo de Alacritty
- Atlas de 1024x1024, llenado fila por fila
- Cuando el atlas se llena, se crea uno nuevo automaticamente
- Damage tracking reduce el trabajo de GPU en ~95%

**Fuentes verificadas en código:**

- Alacritty: display/mod.rs, renderer/mod.rs, renderer/text/atlas.rs,
renderer/text/glyph_cache.rs
- crates.io: crossfont v0.9, winit v0.30.13

---

### Iteracion 3: Input Handling (COMPLETADA)

**Documento:** `research/03-input.md` (916 lineas)

**Que investigo:**

- Captura de eventos de teclado (winit)
- Mapeo de teclas a secuencias ANSI
- Modos cooked vs raw vs cbreak
- Teclas especiales (Ctrl+C, Ctrl+Z, Ctrl+D)
- Copy/paste (bracketed paste, OSC 52)
- Mouse reporting
- IME (Input Method Editor)

**Que descubrimos (hallazgos criticos):**

- **El emulador NO toca termios.** El line discipline del kernel
(drivers/tty/n_tty.c) ya hace echo, buffering, conversion de
senales, y edición de linea. El emulador solo escribe bytes al
master y el kernel se encarga. Alacritty confirma esto: solo
activa IUTF8 en el master, deja todo lo demas en default.
- **El child es responsable de raw mode.** Cuando arranca vim,
htop, o ssh, ese programa modifica los termios de su stdin
(el slave). El emulador permanece pasivo. Esto es lo que hacen
los 3 terminales analizados (Alacritty, WezTerm, Warp).
- **Ctrl+[ es la misma tecla que ESC** (byte 0x1B). El VT100
original tenia ESC mapeado fisicamente a Ctrl+[. Por eso
Ctrl+[ inicia todas las secuencias ANSI.
- **Backspace por default es DEL (0x7F), no BS (0x08).** El man
page termios(3) documenta VERASE como 0x7F. La confusion viene
de terminales ASCII antiguos que usaban BS.
- **Ctrl+D no es una senal.** Es un carácter especial del line
discipline que flush el buffer pendiente o retorna EOF (read
retorna 0) si es el primer carácter. Esta distincion es
importante para no confundirlo con SIGINT (Ctrl+C).
- **Bracketed paste filtra ESC y ETX.** Alacritty implementa
ESC[200~ / ESC[201~ alrededor del texto pegado, pero antes
filtra los bytes 0x1B y 0x03 para evitar que el usuario
inyecte secuencias y cierre el bracket prematuramente. Esto
es seguridad, no solo feature.
- **Shift es el bypass estándar de mouse reporting.** Cuando
una aplicación (vim, tmux) activa mouse mode, mantener
Shift durante el click intercepta el evento para selección
local de texto. Alacritty lo tiene hardcoded; WezTerm lo
expone como configuracion.
- **Hay 3 protocolos de mouse coexistiendo:** SGR (1006, el
moderno, decimal, >223), Normal (1000, legado, offset 32),
y UTF-8 (1015, intermedio). SGR y UTF-8 son mutuamente
excluyentes en el TermMode de Alacritty.

**Patrones comunes observados en los 3 terminales
(verificados en source):**

- `winit` es el estándar de facto para captura de eventos
de ventana (Alacritty, Warp). WezTerm usa su propio
`window` crate.
- Ningun emulador configura raw mode en el arranque. El
child (shell/vim) es responsable de hacerlo en su stdin.
- En el arranque, solo se activa IUTF8 en el master del
PTY. Otros flags de termios quedan en default.
- Alacritty hardcodea Shift como bypass de mouse reporting;
WezTerm lo expone como configuracion. Warp usa SGR (1006)
por defecto sin bypass explicito.
- Bracketed paste esta soportado en los 3 terminales.
Alacritty filtra ESC y ETX del texto pegado para
evitar inyección.
- Los 3 terminales delegan IME a winit (o su abstraccion
propia): winit implementa XIM (X11), text-input-v3
(Wayland), TSF (Windows), NSTextInputClient (macOS).

**Limitaciones de la investigacion (gaps no resueltos):**

- SGR-PIXEL (1016): solo WezTerm lo implementa. No se
documenta porque no es soportado por la mayoria de
terminales Rust.
- Clipboard en Windows: verificado. No tiene diferencias
significativas vs X11/Wayland mas alla de la ausencia
de primary selection.
**Fuentes verificadas en código:**
- Alacritty: input/mod.rs, input/keyboard.rs, event.rs, clipboard.rs
- WezTerm: termwiz/src/input.rs, window/src/os/x11/keyboard.rs
- Linux man pages: termios(3), pty(7), signal(7)
- xterm ctlseqs: invisible-island.net (mouse, bracketed paste, F-keys)
- Wayland text-input-v3 protocol

---

### Iteracion 4: ANSI Parser (COMPLETADA)

**Documento:** `research/04-ansi-parser.md`

**Que investigo:**

- Parser de secuencias ANSI (crate vte vs propio)
- State machine del parser
- Secuencias mas usadas (MVP: ~30-50 secuencias)
- Documento xterm ctlseqs (la referencia)
- vttest para testing

**Que descubrimos (hallazgos criticos):**

- **Los 3 terminales usan state machines de 14-17 estados derivadas
del trabajo de Paul Williams sobre el parser DEC ANSI.** vte
(Alacritty) tiene 14 estados, vtparse (WezTerm) tiene 17, el
fork de vte de Warp tiene 16. La tabla de transiciones esta
precalculada y empacada (4 bits o u16) para acceso rápido.
- **Alacritty y Warp comparten el ecosistema vte.** Alacritty usa
vte v0.15.0 como dependencia externa (`vte = "0.15.0"`).
Warp usa un fork antiguo (v0.13.0, rev 4b399c8). WezTerm
implementa su propio parser en dos crates: `vtparse`
(state machine) y `wezterm-escape-parser` (alto nivel con
enum `Action`).
- **El trait `Handler` de vte define ~60 métodos** que mapean
secuencias CSI/ESC/OSC a operaciones sobre el grid. Alacritty
implementa `Handler for Term<T>` en `alacritty_terminal/src/ term/mod.rs` linea 1059. Warp tiene un trait `Handler` custom
con ~60 métodos adaptados de alacritty_terminal.
- **El set MVP es de ~45 secuencias**, clasificadas en 3
prioridades. Las criticas son: C0 basics (BEL/BS/HT/LF/CR),
DECSC/DECRC, CUP, CUU/CUD/CUF/CUB, ED/EL, SGR 0-9 + 256-color
  - true color, DECAWM/DECOM/DECTCEM, y DSR/DA.
- **DEC private modes criticos soportados por los 3:** modos 1
(cursor key), 6 (origin), 7 (auto-wrap), 25 (cursor visible),
1000/1006 (mouse), 1049 (alt screen), 2004 (bracketed paste).
Modos avanzados como 1016 (SGR pixel) son opcionales.
- **Pending wrap state es un detalle critico documentado en
DEC STD 070 y VT510.** Los 3 terminales lo implementan: el
cursor permanece en la ultima columna despues de escribirla,
y solo hace wrap al recibir el siguiente carácter imprimible.
El wrap se cancela con cualquier secuencia de movimiento de
cursor.
- **vttest es la herramienta estándar para validar conformidad
VT100/VT220.** Mantenido por Thomas E. Dickey (mismo
mantenedor de xterm). Tiene 11 categorías de testing
interactivo. Para testing automatizado se complementa con
`esctest` (George Nachman).
- **Diferencia clave WezTerm vs vte:** WezTerm incluye soporte
nativo para Sixel, Kitty Image (APC), y tmux CC mode. vte
base no soporta ninguno. WezTerm eligio `vtparse` sobre vte
probablemente para tener control total del parser.

**Patrones comunes observados en los 3 terminales
(verificados en source):**

- State machine con tabla de transiciones precalculada (los 3).
- Arquitectura en 2-3 capas: Parser, Performer, Handler (los 3).
- Optimizacion SIMD en Ground state (vte: memchr, vtparse:
tabla de transiciones).
- SGR con 256-color y true color soportado (los 3).
- DECAWM con pending wrap state (los 3).
- OSC 0/2/4/7/8 soportados, OSC 1 (icon name) rechazado por
Alacritty por incompatibilidad con Wayland.

**Limitaciones de la investigacion (gaps no resueltos):**

- No se midio el throughput (bytes/segundo) de cada parser.
Sin benchmarks reales no es posible comparar rendimiento.
- ECMA-48 oficial inaccesible (URL da 404). Se usa VT510
Programmer Reference Manual como alternativa.
- Warp tiene documentacion pública limitada (no hay pagina
de escapes como Alacritty o WezTerm). Soporte exacto se
deduce de issues y código fuente.
- vte sin soporte Sixel nativo. Para implementar Sixel, el
emulador tendria que usar parser diferente o implementar
reconocimiento Sixel a nivel de DCS.
- Sin datos de ECMA-48 oficial; VT510 es la mejor referencia
pública disponible.
- Soporte exacto de DECCOLM (modo 132 columnas) en Warp no
verificado.

**Fuentes verificadas en código:**

- Alacritty: vte/src/lib.rs, vte/src/ansi.rs, alacritty_terminal/
src/term/mod.rs (linea 1059), alacritty_terminal/src/event_loop.rs
(linea 154, 404)
- WezTerm: wezterm-escape-parser/src/lib.rs, vtparse/src/enums.rs,
wezterm-escape-parser/src/parser/mod.rs
- Warp: app/src/terminal/model/ansi/mod.rs, app/src/terminal/
model/ansi/handler.rs, fork de warpdotdev/vte (rev 4b399c8)
- xterm ctlseqs (invisible-island.net/xterm/ctlseqs/ctlseqs.html)
- VT510 Programmer Reference Manual (vt100.net/docs/vt510-rm/
chapter4.html)
- vttest (invisible-island.net/vttest/)
- 23 referencias IEEE en el doc con URLs verificadas HTTP 200

### Iteracion 5: Terminal Grid (COMPLETADA)

**Documento:** `research/05-terminal-grid.md` (1334 lineas)

**Que investigo:**

- Estructura de datos del grid (Vec con ring buffer, VecDeque, ring + flat storage)
- Cell (struct, bytes por celda, atributos wide/zero-width)
- Scrollback buffer (mecanismos de limitacion, drop oldest)
- Cursor (posición, visibilidad, forma, save/restore, pending wrap)
- Scroll regions DECSTBM e interaccion con scroll
- Line wrapping (DECAWM, pending wrap state, reflow)
- Reflow (recalculo de layout al redimensionar)
- Selection local (modos Simple/Block/Semantic/Lines)
- Testeo del grid (vttest categorías, unit tests)

**Que descubrimos (hallazgos criticos):**

- **Los 3 terminales usan ring buffer (o equivalente) para el grid,
pero combinan de forma diferente con el scrollback.** Alacritty
almacena grid y scrollback en un único `Storage<T>` con campo
`zero` que permite rotacion O(1). WezTerm usa `VecDeque<Line>`
donde la cola y la cabeza conviven naturalmente. Warp, fork de
Alacritty, separa el grid activo (`GridStorage`, ring buffer) del
scrollback (`FlatStorage`, chunks de 1000 filas).
- **El pending wrap state se implementa como flag booleano en
los 3 terminales.** Alacritty y Warp lo llaman `input_needs_wrap`
y lo guardan en el struct `Cursor`. WezTerm lo llama `wrap_next`
y lo guarda en `TerminalState`. El flag se activa al escribir
en la ultima columna con DECAWM ON, y se desactiva con
`wrapline()` o cualquier movimiento de cursor.
- **La semantica de Point varia entre terminales.** Alacritty usa
`Line(i32)` con signo para la fila, WezTerm usa `VisibleRowIndex = i64` con signo, Warp usa `VisibleRow(usize)` sin signo. Las
columnas son siempre `usize`. Esta diferencia es relevante para
el diseno del grid.
- **DECSTBM es consistente en los 3 terminales.** Parametros
1-indexed (top, bottom), conversion a 0-indexed internamente,
cursor se mueve a (0, 0) despues de ejecutar, región invalida
(top >= bottom) se ignora.
- **scroll_up delega en scroll_up_relative con origin = scroll_region.start.**
En Alacritty y Warp, el comando CSI S (SU) se traduce a
`scroll_up_relative(origin, lines)` donde origin es el inicio
de la scroll región. WezTerm logra el mismo efecto pasando
las margenes a `screen.scroll_up_within_margins`.
- **IL/DL (insert/delete line) son scroll relativo desde el
cursor.** Los 3 terminales implementan CSI L y CSI M como
`scroll_down_relative(cursor.line, N)` y
`scroll_up_relative(cursor.line, N)`. Esto se valida contra
la scroll región actual antes de ejecutar.
- **Reflow solo en pantalla primaria.** Los 3 terminales
habilitan el recalculo de layout (unir lineas con wrap,
re-dividir con nuevo ancho) unicamente en la pantalla
principal. En la pantalla alternativa (DEC mode 1049),
el resize es simple: truncar si se reduce, invalidar si
se agranda. Esto evita corrupcion del contenido de apps
como vim o htop que usan alt screen.
- **Tamanos de Cell:** Alacritty <= 24 bytes (verificado vía
test `cell_size_is_below_cap`). WezTerm potencialmente menor
gracias a `TeenyString` que inline-a strings de hasta 7 bytes
en un u64. Warp similar a Alacritty (estimado).
- **Scrollback defaults:** Alacritty 10000, WezTerm 3500
(configurable hasta 999.999.999), Warp sin limite fijo
conocido. El mecanismo es drop oldest en los 3.
- **Selection tiene 4 modos en los 3 terminales** (Simple/stream,
Block/Rect rectangular, Semantic, Lines). Alacritty usa `Block`,
Warp usa `Rect` para el rectangular. WezTerm delega la
selection a la capa GUI.

**Patrones comunes observados en los 3 terminales
(verificados en source):**

- Ring buffer como estructura fundamental del grid (Alacritty/Warp
con offset explicito, WezTerm con VecDeque).
- Pending wrap como flag booleano guardado en DECSC/DECRC.
- `scroll_up` delega en `scroll_up_relative` con origin fijo en
scroll_region.start.
- IL/DL como scroll relativo desde cursor.
- DECSTBM con parametros 1-indexed, mueve cursor a (0,0), ignora
si región invalida.
- DECSC/DECRC guardan todo el estado del cursor (posición,
plantilla, charsets, pending wrap).
- Reflow solo en pantalla primaria (no en alt screen).
- 4 modos de selection (Simple/Block/Semantic/Lines).
- vttest como herramienta de validacion común (categorías 1, 2, 8
son las mas relevantes para el grid).

**Limitaciones de la investigacion (gaps no resueltos):**

- Definicion exacta de `Cell` en Warp no localizada (vive en el
crate externo `warp_terminal`).
- Scrollback default exacto de Warp no identificado en el código
fuente examinado.
- DECSCUSR blinking implementation no verificada en detalle
(timer de parpadeo).
- DEC mode 12 (cursor blink) no verificado en los 3 terminales.
- Interaccion exacta entre pending wrap y scroll región
requiere testing empirico.
- Wide chars en reflow: solo Alacritty verificado en detalle
con `LEADING_WIDE_CHAR_SPACER`.
- Scroll horizontal (DECSLRM) no se investigo.
- vttest no se ejecuto contra los terminales (resultados de
paso/fallo son por verificar).
- Benchmarks de rendimiento no realizados (throughput de scroll,
latencia de input).
- WezTerm selection no esta en el terminal crate (vive en
`wezterm-gui`).

**Fuentes verificadas en código (20 referencias IEEE con URLs
verificadas HTTP 200):**

- Alacritty: `alacritty_terminal/src/grid/storage.rs`,
`grid/mod.rs`, `grid/row.rs`, `grid/resize.rs`,
`term/cell.rs`, `term/mod.rs`, `index.rs`, `selection.rs`
- WezTerm: `term/src/screen.rs`, `term/src/terminalstate/mod.rs`,
`term/src/terminalstate/performer.rs`, `term/src/lib.rs`,
`wezterm-cell/src/lib.rs`, `config/src/config.rs`
- Warp: `app/src/terminal/model/grid/grid_handler.rs`,
`grid/storage.rs`, `grid/grid_storage.rs`,
`grid/ansi_handler.rs`, `grid/resize.rs`, `selection.rs`
- vttest: [https://invisible-island.net/vttest/](https://invisible-island.net/vttest/)
- VT510: [https://vt100.net/docs/vt510-rm/DECAWM.html](https://vt100.net/docs/vt510-rm/DECAWM.html)

---

### Iteracion 6: Arquitectura Final (COMPLETADA)

**Documento:** `research/06-arquitectura.md` (1077 lineas).

Adicionalmente, esta iteracion genero 6 archivos de
investigacion de subagentes
(`docs/prompts/iter-06-investigacion-A.md` a
`iter-06-investigacion-F.md`), 6 nuevos ADRs
(ADR-0003 a ADR-0008), y 4 specs técnicas
(`docs/specs/requisitos.md`, `testing-strategy.md`,
`error-handling.md`, `roadmap.md`).

**Que investigo:**

- Arquitectura de 3 capas (presentation, domain,
  infrastructure).
- Patron de event loop (2 hilos sin async runtime).
- Seleccion final de crates (vte, nix, winit, wgpu,
  glyphon, y 9 dependencias mas).
- Estrategia de testing (4 niveles + benchmarks + CI).
- Error handling (anyhow + thiserror + tracing).
- MVP y roadmap por fases (6 fases, MVP en Fase 3).

**Que descubrimos (hallazgos criticos):**

- Los 5 terminales de referencia (Alacritty, WezTerm,
  Warp, Rio, Ghostty) convergen en 3 capas, validando
  el patron.
- Alacritty es el patron de referencia mas limpio y
  replicable (2 hilos + polling::Poller + mpsc +
  FairMutex, sin runtime async).
- La MSRV efectiva del proyecto queda en 1.87.0
  (impuesta por wgpu 29).
- vte 0.15 es el parser estándar; wezterm-escape-parser
  es interno al monorepo de WezTerm.
- Ninguno de los 3 terminales Rust usa proptest o
  criterion publicamente; el proyecto los introduce.
- El MVP debe ser Linux-only; macOS/Windows se abordan
  en Fase 5 con migracion de nix a portable-pty.

**Decisiones tomadas (referencias a ADRs):**

- ADR-0003: Estructura en 3 capas (Presentation/
  Domain/Infra).
- ADR-0004: 14 crates + 1 dev-dep, MSRV 1.87.0.
- ADR-0005: Event loop de 2 hilos sin async runtime.
- ADR-0006: Testing en 4 niveles (unit + integration
  + proptest + vttest).
- ADR-0007: anyhow en bordes, thiserror en domain,
  tracing para logging.
- ADR-0008: MVP en Fase 3 (8 sprints para produccion).

**Limitaciones y riesgos consolidados:**

- MSRV alta (1.87.0) excluye Rust < 1.87.
- glyphon es relativamente nuevo, depende de wgpu.
- vte no soporta Sixel, iTerm2 image protocol, ni
  Kitty graphics (no objetivos del MVP).
- Warp esta parcialmente open source; algunos archivos
  no son accesibles.
- vttest es interactivo; la automatizacion completa
  es dificil.
- Ningun terminal Rust usa proptest o criterion
  publicamente; el proyecto los introduce.
- El reflow de resize es complejo; se copia
  implementación de Alacritty en Fase 4.
- MVP es Linux-only; macOS/Windows en Fase 5.
- Latencia de mpsc no medida; migrar a crossbeam si
  es problema.

**Stack final del proyecto (tabla de crates):**

| Categoria | Crate | Version | MSRV |
|:----------|:------|:--------|:-----|
| Parser ANSI | vte | 0.15 | 1.65 |
| PTY | nix | 0.31 | 1.65 |
| Ventana | winit | 0.30 | 1.70 |
| Render | wgpu | 29 | 1.87 |
| Texto | glyphon | 0.11 | (heredada) |
| Unicode | unicode-width | 0.2 | 1.66 |
| Flags | bitflags | 2 | 1.56 |
| Logging | tracing | 0.1 | 1.65 |
| Errores entry | anyhow | 1 | 1.68 |
| Errores domain | thiserror | 2 | 1.68 |
| Paths | dirs | 6 | N/A |
| Serializacion | serde | 1 | 1.56 |
| CLI | clap | 4 | 1.85 |
| Lock | parking_lot | 0.12 | 1.65 |
| Benchmarks | criterion | 0.8 | 1.86 |

**Roadmap del proyecto (resumen):**

- **Fase 0 (Sprint 1):** Esqueleto + PTY funcional.
- **Fase 1 (Sprint 2):** Parser ANSI basico.
- **Fase 2 (Sprint 3):** Grid 80x24 + Render GPU.
- **Fase 3 (Sprints 4-5):** MVP funcional (vim/htop
  funcionan).
- **Fase 4 (Sprints 6-7):** Reflow + scrollback +
  mouse + selection.
- **Fase 5 (Sprints 8-9):** Testing exhaustivo +
  performance 60fps + packaging + portabilidad.

El detalle por sprint con tareas, estimaciones y
criterios vive en `docs/specs/roadmap.md`.

**Documentos generados en iter 6:**

- `research/06-arquitectura.md` (1077 lineas).
- `decisions/ADR-0003-estructura-codigo.md` (151 lineas).
- `decisions/ADR-0004-seleccion-crates.md` (195 lineas).
- `decisions/ADR-0005-event-loop-io.md` (169 lineas).
- `decisions/ADR-0006-testing-strategy.md` (217 lineas).
- `decisions/ADR-0007-error-handling.md` (206 lineas).
- `decisions/ADR-0008-roadmap-mvp.md` (297 lineas).
- `specs/requisitos.md` (487 lineas).
- `specs/testing-strategy.md` (555 lineas).
- `specs/error-handling.md` (615 lineas).
- `specs/roadmap.md` (374 lineas).
- `prompts/iter-06-investigacion-A.md` (559 lineas).
- `prompts/iter-06-investigacion-B.md` (467 lineas).
- `prompts/iter-06-investigacion-C.md` (519 lineas).
- `prompts/iter-06-investigacion-D.md` (827 lineas).
- `prompts/iter-06-investigacion-E.md` (526 lineas).
- `prompts/iter-06-investigacion-F.md` (834 lineas).

**Fuentes a consultar (referencias externas):**

- Codigo de Alacritty: github.com/alacritty/alacritty
- Codigo de WezTerm: github.com/wez/wezterm
- Codigo de Warp: github.com/warpdotdev/Warp
- Codigo de Rio: github.com/raphamorim/rio
- Codigo de Ghostty (Zig): github.com/ghostty-org/ghostty

---

## Que falta por investigar

### Componentes no documentados


| Componente       | Estado                  | Prioridad |
| ---------------- | ----------------------- | --------- |
| ANSI Parser      | No investigado          | Alta      |
| Terminal Grid    | No investigado          | Alta      |
| Event Loop       | No investigado          | Alta      |
| Testing strategy | No investigado          | Media     |
| Error handling   | No investigado          | Media     |
| Config system    | No investigado          | Baja      |
| Clipboard        | Documentado en 03-input | -         |
| IME              | Documentado en 03-input | -         |


### Decisiones pendientes


| Decision                | Alternativas          | Estado               |
| ----------------------- | --------------------- | -------------------- |
| Crate para ANSI parsing | vte vs propio         | Pendiente            |
| Estructura del grid     | Matriz vs piece table | Pendiente            |
| Patron de event loop    | winit events vs async | Pendiente            |
| Backend de rendering    | OpenGL vs wgpu        | Decision: OpenGL MVP |
| Crate para PTY          | nix (decidido)        | Decidido             |
| Crate para fuentes      | crossfont (decidido)  | Decidido             |
| Crate para ventana      | winit (decidido)      | Decidido             |


---

## Consecuencias

### Positivas

- Documentacion verificada en código fuente
- Decisiones basadas en evidencia, no en suposiciones
- Cada iteracion produce un documento util para implementar

### Negativas

- La investigacion toma mas tiempo que escribir código
- Algunos componentes (ANSI parser) son complejos de investigar
- Los documentos pueden quedar desactualizados si los crates
cambian significativamente

### Neutrales

- Las iteraciones 4-6 dependen de hallazgos de iteraciones
anteriores (especialmente la 1 y 2)
- El documento de Input (iteracion 3) ya esta completo,
lo que facilita las siguientes iteraciones

---

## Referencias

[1] Man page. pty(7). [https://man7.org/linux/man-pages/man7/pty.7.html](https://man7.org/linux/man-pages/man7/pty.7.html)

[2] Alacritty source. tty/unix.rs.
[https://github.com/alacritty/alacritty/blob/master/](https://github.com/alacritty/alacritty/blob/master/)
alacritty_terminal/src/tty/unix.rs

[3] Warp source. local_tty/unix.rs.
[https://github.com/warpdotdev/warp/blob/master/](https://github.com/warpdotdev/warp/blob/master/)
app/src/terminal/local_tty/unix.rs

[4] crossfont crate. crates.io/crates/crossfont.
"Originally made solely for rendering monospace fonts
in Alacritty".

[5] Alacritty source. renderer/text/atlas.rs.
[https://github.com/alacritty/alacritty/blob/master/](https://github.com/alacritty/alacritty/blob/master/)
alacritty/src/renderer/text/atlas.rs

[6] Alacritty source. renderer/text/glyph_cache.rs.
[https://github.com/alacritty/alacritty/blob/master/](https://github.com/alacritty/alacritty/blob/master/)
alacritty/src/renderer/text/glyph_cache.rs

---

## Cambios


| Version | Fecha      | Cambios                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ------- | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 0.1.0   | 2026-06-13 | Primer borrador                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| 0.2.0   | 2026-06-13 | Reescritura completa. ADR con decisiones concretas. Analisis de iteraciones 0-3. Iteraciones 4-6 definidas con preguntas clave.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| 0.3.0   | 2026-06-13 | Iter 3: agregados descubrimientos criticos del documento 03-input.md.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| 0.4.0   | 2026-06-13 | Iter 3 correccion: secciones 'Decisiones' reescritas como 'Patrones comunes observados' (los docs de research son de investigacion, no de decision). Limitaciones actualizadas a gaps reales despues de 3ra pasada.                                                                                                                                                                                                                                                                                                                                                                                                                        |
| 0.5.0   | 2026-06-13 | Iter 4 completada: estado pasa de PENDIENTE a COMPLETADA. Se agregan descubrimientos criticos del doc 04-ansi-parser.md, patrones comunes observados (state machines, arquitectura en capas, optimizacion SIMD, etc.), limitaciones (throughput no medido, ECMA-48 inaccesible, documentacion Warp limitada), y fuentes verificadas en código (Alacritty, WezTerm, Warp con paths exactos).                                                                                                                                                                                                                                                |
| 0.6.0   | 2026-06-14 | Iter 5 completada: estado pasa de PENDIENTE a COMPLETADA. Se agregan 10 descubrimientos criticos del doc 05-terminal-grid.md (ring buffer + scrollback, pending wrap flag, semantica de Point, DECSTBM consistente, scroll_up vs scroll_up_relative, IL/DL como scroll relativo, reflow solo en primary, tamanos de Cell, scrollback defaults, 4 modos de selection), 9 patrones comunes observados, 10 limitaciones (Cell de Warp, scrollback default Warp, blinking, DEC mode 12, interaccion wrap+región, wide chars en reflow, DECSLRM, vttest no ejecutado, benchmarks, selection WezTerm en GUI), y 20 fuentes verificadas HTTP 200. |


---

*Estado: borrador*