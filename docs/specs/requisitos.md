```yaml
titulo: "Requisitos del Producto , Baud"
tipo: especificacion
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-14"
fecha_modificacion: "2026-06-14"
version: "0.1.0"
estado: borrador
tags: [requisitos, rf, rnf, mvp, aceptacion]
```

# Requisitos del Producto , Baud

## 1. Vision

El proyecto es un emulador de terminal para Linux,
escrito en Rust, que cumple con la especificacion VT100/
xterm basica y soporta el uso diario de herramientas
TUI modernas (vim, htop, tmux, ssh). El objetivo es
producir un binario pequeno (<10MB), de bajo consumo
de memoria (<100MB en 200x50), con 60fps de rendimiento,
y con código modular que permita extensiones futuras
(temas, plugins, multiplexacion).

Esta es la primera iteracion del proyecto. La meta
inmediata es alcanzar un MVP funcional al final de la
Fase 3 del roadmap (definido en
`docs/decisions/ADR-0008-roadmap-mvp.md`), que cubra
las herramientas TUI mas comunes.

## 2. Alcance

### 2.1 Incluido en el MVP (Fases 0-3)

- Arranque del shell del usuario (bash, zsh, fish).
- Input de teclado: caracteres ASCII, Enter, Backspace,
  Tab, flechas, Ctrl+letra, Shift+letra, Alt+letra.
- Parser ANSI: SGR (16 colores), cursor movement,
  clear screen/line, scroll.
- Grid 80x24 con 16 colores de foreground y background.
- Render GPU con wgpu (Vulkan/Metal/DX12/OpenGL).
- Resize de ventana con SIGWINCH.
- Copy/Paste con Ctrl+Shift+C y Ctrl+Shift+V.
- Alternate screen (DEC 1049) para vim y htop.
- Shutdown graceful con SIGHUP al child.

### 2.2 Excluido del MVP (Fases 4-5)

- Reflow de lineas al resize.
- Seleccion de texto con mouse.
- Mouse reporting (SGR, Normal, UTF-8).
- Scrollback extenso (MVP: 100 lineas; Fases 4-5:
  1000-10000).
- Temas personalizables.
- Tabs y multiplexacion.
- Configuracion de usuario vía TOML.
- True color (24-bit) y paletas extendidas (256-color).
- IME para CJK.
- macOS y Windows.
- Sixel, iTerm2 image protocol, Kitty graphics.
- Kitty keyboard protocol.

## 3. Requisitos Funcionales (RF)

### RF-01: Arrancar shell del usuario

**Prioridad:** P0 (bloqueante para MVP).

**Descripcion:** el emulador arranca el shell
configurado por el usuario (default: `$SHELL` del
entorno o `/bin/bash` si `$SHELL` no esta definido)
dentro de un nuevo PTY y lo conecta a la ventana
grafica.

**Criterios de aceptacion:**

- Al ejecutar el binario, el shell arranca en menos
  de 500ms desde que la ventana es visible.
- El prompt del shell aparece correctamente
  (color, posición).
- `Ctrl+C` envia SIGINT al shell, no cierra la
  aplicación.
- Si el shell muere, el emulador muestra `[Proceso
  terminado: código N]` y permite escribir nuevos
  comandos.

### RF-02: Enviar input de teclado al shell

**Prioridad:** P0.

**Descripcion:** el emulador captura eventos de
teclado vía winit, los convierte a bytes o secuencias
ANSI, y los envia al PTY master.

**Criterios de aceptacion:**

- Caracteres ASCII imprimibles (0x20-0x7E) se envian
  como su byte.
- `Enter` envia `CR` (0x0D) o `LF` (0x0A) segun
  configuracion de termios.
- `Backspace` envia `DEL` (0x7F) por defecto.
- Flechas envian `ESC [ A`, `ESC [ B`, `ESC [ C`,
  `ESC [ D`.
- `Ctrl+C` genera `0x03` (SIGINT). `Ctrl+D` genera
  `0x04` (EOF). `Ctrl+Z` genera `0x1A` (SIGTSTP).
- `Shift+Tab` envia `ESC [ Z` (back-tab).

### RF-03: Parsear ANSI basico (SGR, cursor, clear)

**Prioridad:** P0.

**Descripcion:** el emulador parsea las secuencias
ANSI criticas usando el crate `vte` 0.15 y despacha
las acciones correspondientes al grid.

**Criterios de aceptacion:**

- SGR (`CSI Ps m`) aplica: 0 (reset), 1 (bold),
  3 (italic), 4 (underline), 7 (reverse), 30-37
  (fg 8 colores), 40-47 (bg 8 colores), 90-97
  (fg bright), 100-107 (bg bright).
- Cursor movement (`CSI A/B/C/D/H/V`) mueve el cursor
  correctamente respetando DECOM (origin mode).
- Clear screen (`CSI Ps J`) limpia 0=atras, 1=adelante,
  2=todo.
- Clear line (`CSI Ps K`) limpia 0=atras, 1=adelante,
  2=todo.
- Scroll up (`CSI Ps S`) y down (`CSI Ps T`) funcionan
  respetando la scroll región.

### RF-04: Mantener scrollback basico

**Prioridad:** P1 (no bloqueante para MVP funcional
mínimo, pero requerido para usabilidad).

**Descripcion:** el emulador conserva al menos 100
lineas de historial que el usuario puede revisar con
`PageUp`/`PageDown`.

**Criterios de aceptacion:**

- El ring buffer almacena al menos 100 lineas mas
  alla de la pantalla visible.
- `PageUp` desplaza la vista hacia las lineas
  anteriores (hasta el limite del scrollback).
- `PageDown` regresa a la vista normal (cursor al
  fondo).
- Las lineas de scrollback se renderizan con los
  mismos colores y estilos que la pantalla principal.
- El scrollback se descarta al llegar al limite
  (drop oldest).
- `Shift+PageUp` y `Shift+PageDown` también funcionan.

### RF-05: Renderizar grid de 80x24

**Prioridad:** P0.

**Descripcion:** el emulador renderiza una pantalla
de 80 columnas por 24 filas con texto monospace,
16 colores, y estilos basicos (bold, italic, underline,
reverse).

**Criterios de aceptacion:**

- La pantalla muestra exactamente 80 columnas y 24
  filas en una ventana de tamano por defecto.
- Cada celda contiene un carácter, color fg, color bg,
  y flags (bold, italic, etc.).
- El glyph atlas cachea glyphs ASCII (32-126) desde
  el primer frame.
- Los caracteres wide (CJK) se renderizan como
  espacios o glyph de fallback, sin panic.
- Las celdas vacias se renderizan con color bg
  configurable.

### RF-06: Renderizar texto con estilos SGR

**Prioridad:** P0.

**Descripcion:** el emulador renderiza texto con
los estilos aplicados por SGR: colores fg/bg de 16,
bold, italic, underline, reverse, dim.

**Criterios de aceptacion:**

- `ls --color=auto` muestra archivos con colores
  diferentes segun tipo.
- `git status` muestra verde para staged, rojo para
  untracked.
- Bold (SGR 1) usa variante bold de la fuente.
- Italic (SGR 3) usa variante italic.
- Underline (SGR 4) dibuja una linea bajo el texto.
- Reverse (SGR 7) invierte fg y bg.

### RF-07: Manejar resize de ventana

**Prioridad:** P0 (bloqueante para que TUIs funcionen).

**Descripcion:** el emulador maneja el redimensionamiento
de la ventana recalculando el grid y enviando SIGWINCH
al PTY.

**Criterios de aceptacion:**

- Al redimensionar, el grid se recalcula con el
  nuevo tamano en menos de 100ms.
- Se envia `ioctl(TIOCSWINSZ)` al PTY master para
  notificar al shell.
- El shell (bash) ajusta `COLUMNS` y `LINES`
  automaticamente.
- `vim` ajusta su layout al nuevo tamano.
- No se pierde contenido visible durante el resize
  (las celdas fuera del nuevo tamano se descartan).

### RF-08: Copy/Paste basico

**Prioridad:** P0.

**Descripcion:** el emulador soporta copiar texto
seleccionado al clipboard del sistema y pegar texto
del clipboard al shell como input.

**Criterios de aceptacion:**

- `Ctrl+Shift+C` copia la selección actual al
  clipboard del sistema (X11 selection, Wayland
  data-control, o equivalente segun plataforma).
- `Ctrl+Shift+V` pega el contenido del clipboard al
  shell como input.
- El pegado convierte newlines (LF) a `CR+LF` para
  que el shell los interprete como Enter.
- El pegado respeta bracketed paste mode (DEC 2004)
  cuando esta activo: envuelve en `ESC[200~ ... ESC
  [201~`.
- El pegado filtra bytes 0x1B (ESC) y 0x03 (ETX) del
  contenido para evitar inyección de secuencias.

### RF-09: Soportar alternate screen (DEC 1049)

**Prioridad:** P0 (sin esto, vim, htop, less no
funcionan).

**Descripcion:** el emulador soporta la pantalla
alternativa activada por `CSI ? 1049 h` y restaurada
por `CSI ? 1049 l`.

**Criterios de aceptacion:**

- `vim` abre correctamente, permite editar texto,
  insertar, guardar, y salir.
- Al salir de vim (`:q`), la pantalla original se
  restaura exactamente como estaba.
- El scrollback no se mezcla con el contenido de
  la alternate screen.
- `htop` muestra su interfaz correctamente y permite
  navegar procesos.
- `less` permite navegacion con flechas y `q` para
  salir.
- `tmux` detecta correctamente el tamano de la
  terminal y crea sesiones usables.

### RF-10: Reflow de lineas al resize (Fase 4)

**Prioridad:** P1.

**Descripcion:** al redimensionar la ventana, las
lineas que exceden el nuevo ancho se re-dividen
(reflow) para mantener legibilidad.

**Criterios de aceptacion:**

- Una linea que era una sola (sin wrap) se divide
  en multiples cuando la ventana se hace mas angosta.
- Multiples lineas con `WRAPLINE` flag se fusionan
  en una sola cuando la ventana se hace mas ancha.
- El reflow solo ocurre en la pantalla primaria; en
  la alternate screen el resize es simple (truncar
  si se reduce, invalidar si se agranda).
- El contenido total (caracteres) se preserva: no
  se pierden ni se duplican caracteres en el reflow.

### RF-11: Seleccion de texto con mouse (Fase 4)

**Prioridad:** P1.

**Descripcion:** el usuario puede seleccionar texto
con el mouse usando click, drag, y triple-click.

**Criterios de aceptacion:**

- Click izquierdo posiciona el cursor de selección
  en la celda correspondiente.
- Click + drag selecciona un rango de caracteres.
- Triple-click selecciona la linea completa.
- La selección se renderiza con color reverse sobre
  el texto.
- `Ctrl+Shift+C` copia la selección al clipboard.
- Doble-click selecciona una palabra (delimitada por
  espacios o simbolos no-alfanumericos).
- `Shift` como bypass: mantener Shift durante el
  click permite seleccionar aunque el child tenga
  mouse reporting activo.

### RF-12: Mouse reporting basico (Fase 4)

**Prioridad:** P2 (nice-to-have, no esencial para
bash basico).

**Descripcion:** el emulador envia eventos de mouse al
child cuando este activa mouse reporting (DEC 1000,
1002, 1003, 1006 SGR).

**Criterios de aceptacion:**

- DEC 1000 (Normal): envia `ESC [ M Cb Cx Cy` para
  click + movimiento + release.
- DEC 1006 (SGR): envia `ESC [ < Cb ; Cx ; Cy M/m`
  (soporta coordenadas >223).
- DEC 1002 (Button-event): incluye drag con boton
  presionado.
- DEC 1003 (Any-event): incluye movimiento sin boton
  presionado.
- `Shift` + click bypass: el evento se usa para
  selección local, no se envia al child.
- Mouse wheel: envia `ESC [ M Cb Cx Cy` con
  `Cb = 64` (up) o `Cb = 65` (down).

## 4. Requisitos No Funcionales (RNF)

### RNF-01: Rendimiento

**Categoria:** rendimiento.

**Metricas y targets:**

| Metrica | Target MVP | Target Produccion | Metodo de medicion |
|:--------|:-----------|:------------------|:-------------------|
| Frames por segundo | 60 fps en 80x24 | 60 fps en 200x50 | `cargo bench` + manual |
| Latencia input -> display | <32ms | <16ms | oscilloscope log en dev |
| Tiempo de render por frame | <16ms | <2ms | criterion bench |
| Throughput parser | >50 MB/s | >500 MB/s | criterion bench |
| Scroll latency (100 lineas) | <5ms | <1ms | criterion bench |
| Tiempo de resize | <200ms | <50ms | manual con cronometro |

### RNF-02: Compatibilidad VT100/xterm

**Categoria:** compatibilidad.

**Metricas y targets:**

| Metrica | Target | Metodo |
|:--------|:-------|:-------|
| vttest categoría 1 (cursor) | 100% pass | Ejecucion manual |
| vttest categoría 2 (screen features) | 100% pass | Ejecucion manual |
| vttest categoría 3 (character sets) | >80% pass | Ejecucion manual |
| vttest categoría 6 (VT102 features) | 100% pass | Ejecucion manual |
| vttest categoría 8 (VT102 mode) | 100% pass | Ejecucion manual |
| esctest subset critico | 100% pass | CI automatizado |
| DA1 response | `ESC[?6c` | verificado en IT-001 |

### RNF-03: Portabilidad

**Categoria:** portabilidad.

**Metricas y targets:**

| Plataforma | Estado MVP | Estado Produccion |
|:-----------|:-----------|:-------------------|
| Linux x86_64 | Soportado (Fase 0) | Soportado (Fase 5) |
| Linux aarch64 | Soportado (Fase 5) | Soportado (Fase 5) |
| macOS x86_64 | No soportado (MVP) | Soportado (Fase 5) |
| macOS aarch64 (Apple Silicon) | No soportado (MVP) | Soportado (Fase 5) |
| Windows x86_64 | No soportado (MVP) | Soportado (Fase 5) |

**Restriccion de MVP:** Linux unicamente. El crate `nix`
es Unix-only; migrar a `portable-pty` para soportar
Windows/macOS es decision de Fase 5.

### RNF-04: Estabilidad

**Categoria:** estabilidad.

**Metricas y targets:**

| Metrica | Target MVP | Target Produccion | Metodo |
|:--------|:-----------|:------------------|:-------|
| Panics en uso normal (10 min) | 0 | 0 | sesión manual |
| Panics en vttest categoría 1-4 | 0 | 0 | ejecución automatizada |
| Memory leaks (valgrind) | 0 detectados | 0 detectados | valgrind CI |
| Uso de CPU en idle | <1% | <1% | htop en idle |
| Uptime sin restart | 24h sin degradacion | 7 dias | sesión larga |

### RNF-05: Uso de Memoria

**Categoria:** rendimiento (memoria).

**Metricas y targets:**

| Configuracion | Target MVP | Target Produccion |
|:--------------|:-----------|:-------------------|
| 80x24 + 100 lineas scrollback | <30MB | <25MB |
| 80x24 + 10000 lineas scrollback | N/A (MVP) | <60MB |
| 200x50 + 10000 lineas scrollback | N/A (MVP) | <100MB |
| Binary size (release) | <15MB | <10MB |
| Binary size (debug) | N/A (no se distribuye) | N/A |

**Metodo de medicion:** `/usr/bin/time -v` o `ps -o
rss= -p $PID` despues de 10 segundos de uso normal.

### RNF-06: Cobertura de Testing

**Categoria:** testing.

**Metricas y targets:**

| Modulo | Target MVP | Target Produccion |
|:-------|:-----------|:-------------------|
| `src/grid/` | >60% | >80% |
| `src/parser/` | >50% | >75% |
| `src/pty/` | >40% | >65% |
| `src/renderer/` | N/A (requiere GPU) | >50% (con mock GPU) |
| `src/input/` | >40% | >65% |
| Global | >50% | >60% |

**Herramientas:** `cargo tarpaulin` (Linux) o
`cargo-llvm-cov` para medir cobertura. Reporte en
cada PR vía codecov.

## 5. Restricciones

### 5.1 Tecnicas

- **Lenguaje:** Rust, edition 2021, MSRV 1.87.0.
- **Plataforma MVP:** Linux (kernel >= 4.4 para PTY
  improvements).
- **Dependencias core:** vte, nix, winit, wgpu, glyphon
  (ver ADR-0004 para lista completa).
- **Render backend:** WebGPU vía wgpu (Vulkan, Metal,
  DX12, OpenGL ES).

### 5.2 De equipo

- **Desarrollador:** 1 persona (Carlos Canabal Cordero).
- **Tiempo disponible:** ~10-15 horas semanales.
- **Presupuesto:** 0 USD (proyecto personal).

### 5.3 De diseno

- **Sin emojis en UI.** Solo texto y caracteres ASCII.
- **Codigo en espanol (comentarios y docs), identificadores
  en ingles.** Los nombres de variables, funciones y
  tipos siguen convencion de Rust (ingles).
- **Ortografia espanol correcta:** tildes y enie
  obligatorias en documentacion.
- **Sin em-dashes (---) en contenido.** Solo como
  separador de sección markdown.

### 5.4 De plazo

- **MVP (Fase 3):** target 4 sprints (~2 meses).
- **Produccion (Fase 5):** target 8 sprints (~4 meses).
- **Sin fecha dura.** El proyecto se entrega cuando
  este listo, no por deadline externo.

## 6. Referencias

- docs/decisions/ADR-0008-roadmap-mvp.md (roadmap
  detallado).
- docs/prompts/iter-06-investigacion-F.md
  (investigacion base).
- docs/research/00-fundamentos.md a 05-terminal-grid.md
  (componentes individuales).
- ECMA-48. "Control Functions for Coded Character
  Sets". 5ta edición, 1991. Estandar de secuencias
  ANSI.
- ISO/IEC 6429. Estandar internacional equivalente a
  ECMA-48.
- VT510 Programmer Reference Manual.
  https://vt100.net/docs/vt510-rm/
- xterm ctlseqs (Thomas Dickey).
  https://invisible-island.net/xterm/ctlseqs/ctlseqs.html

## Cambios

| Version | Fecha      | Cambios |
|:--------|:-----------|:--------|
| 0.1.0   | 2026-06-14 | Primer borrador. 12 RF + 6 RNF con criterios verificables. |
