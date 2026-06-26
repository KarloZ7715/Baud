```yaml
titulo: "Roadmap Operativo , Detalle por Sprint"
tipo: especificacion
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-14"
fecha_modificacion: "2026-06-14"
version: "0.1.0"
estado: borrador
tags: [roadmap, sprints, fases, operativo, implementacion, milestones]
```

# Roadmap Operativo , Detalle por Sprint

## 1. Resumen

Este documento complementa
`docs/decisions/ADR-0008-roadmap-mvp.md` con el
detalle operativo del roadmap: tareas por sprint,
estimaciones de tiempo, dependencias técnicas, y
criterios de aceptacion por tarea. La decision de
alto nivel (6 fases, MVP en Fase 3) vive en el
ADR-0008. Este doc se enfoca en la implementación
semana a semana.

## 2. Convenciones de Estimacion

- **S (Small):** 1-2 dias. Tarea con diseno claro,
  sin investigacion previa necesaria.
- **M (Medium):** 3-5 dias. Tarea que requiere
  investigacion de API o integracion, pero sin
  ambiguedad arquitectonica.
- **L (Large):** 1-2 semanas. Tarea con multiples
  sub-componentes, requiere diseno cuidadoso.
- **XL (Extra Large):** 3+ semanas. Dividir en
  sub-tareas antes de empezar.

## 3. Fase 0: Esqueleto + PTY

**Duracion total:** 1 sprint (~2 semanas)

**Objetivo:** ventana winit que se abre, PTY creado,
bash arranca, output basico se ve (puede ser texto
sin render elaborado).

### Sprint 1 (Semanas 1-2)

| Tarea                                                            | Estimacion | Dependencias  | Criterio de aceptacion                         |
| ---------------------------------------------------------------- | ---------- | ------------- | ---------------------------------------------- |
| Setup del proyecto Cargo                                         | S          | Ninguna       | `cargo run` ejecuta un hello world             |
| Configurar `cargo fmt`, `cargo clippy`                           | S          | Setup         | `cargo clippy` pasa sin warnings               |
| Crear `src/main.rs` con hello world                              | S          | Setup         | Binario ejecuta                                |
| Crear estructura de modulos vacia                                | S          | Setup         | `src/pty/`, `src/grid/`, etc. existen          |
| Implementar `Pty::open()` con `nix::pty::openpty`                | M          | Setup         | Test unitario: openpty retorna FDs validos     |
| Implementar `Pty::spawn(shell, args)` con `Command` + `pre_exec` | L          | Pty::open     | Test integration: bash arranca                 |
| Crear ventana con winit                                          | M          | Setup         | Ventana visible al ejecutar                    |
| Loop de eventos basico                                           | M          | ventana       | WindowEvent::CloseRequested termina el proceso |
| Hilo PTY separado que lee bytes                                  | M          | Pty::spawn    | Bytes del shell se leen                        |
| Comunicacion GUI <-> PTY vía mpsc                                | M          | Hilo PTY      | Input se envia al shell                        |
| Logging basico con tracing                                       | S          | Setup         | tracing::info!() funciona                      |
| Documento iter-06-investigacion-X.md limpio                      | S          | Investigacion | Subagentes entregan archivos                   |

**Demo al final del Sprint 1:**

```bash
$ cargo run
# Ventana se abre
# En la ventana: prompt de bash "$"
# Escribir "echo hola\n" muestra "hola"
# Ctrl+C no mata el emulador
# Cerrar ventana termina el proceso
```

## 4. Fase 1: Parser ANSI Basico

**Duracion total:** 1 sprint (~2 semanas)

**Objetivo:** el parser vte reconoce SGR (color),
cursor movement, clear screen/line, y escribe al grid.

### Sprint 2 (Semanas 3-4)

| Tarea                                                  | Estimacion | Dependencias   | Criterio de aceptacion          |
| ------------------------------------------------------ | ---------- | -------------- | ------------------------------- |
| Integrar crate `vte` 0.15                              | S          | Fase 0         | `use vte;` funciona             |
| Definir `Term` que implementa `Handler`                | M          | vte            | Compila                         |
| Wire bytes del PTY al parser                           | M          | Hilo PTY, vte  | Bytes alimentan el parser       |
| Manejar `print(c)` en el Handler                       | M          | Term           | Caracteres se escriben al grid  |
| Manejar CSI cursor movement (A/B/C/D/H)                | M          | Handler        | Cursor se mueve correctamente   |
| Manejar CSI SGR (colores 30-37, 40-47, 90-97)          | M          | Handler        | Color se aplica                 |
| Manejar CSI clear (J, K)                               | M          | Handler        | Pantalla y linea se limpian     |
| Manejar ESC[?25h/l (cursor visible)                    | S          | Handler        | Cursor aparece/desaparece       |
| Unit tests para cada secuencia                         | M          | Implementacion | `cargo test` pasa               |
| Render placeholder: dibujar grid como bloques de color | L          | Grid, parser   | Colores son visibles en ventana |

**Demo al final del Sprint 2:**

```bash
$ cargo run
# Ventana se abre
# Prompt de bash aparece
# $ echo -e "\e[31mROJO\e[0m"
# "ROJO" se ve en rojo
# $ echo -e "\e[2J"
# Pantalla se limpia
# $ echo -e "\e[5;10H"
# Cursor salta a linea 5, columna 10
```

## 5. Fase 2: Grid Basico + Render

**Duracion total:** 1 sprint (~2 semanas)

**Objetivo:** grid de 80x24 se renderiza en pantalla
con wgpu y glyphon. Texto monospace, 16 colores, SGR
basico (bold, italic, underline, reverse).

### Sprint 3 (Semanas 5-6)

| Tarea                                                | Estimacion | Dependencias        | Criterio de aceptacion               |
| ---------------------------------------------------- | ---------- | ------------------- | ------------------------------------ |
| Integrar crate `winit` 0.30 (ya en Fase 0)           | S          | -                   | -                                    |
| Integrar crate `wgpu` 29                             | M          | winit               | Contexto wgpu se crea                |
| Integrar crate `glyphon` 0.11                        | M          | wgpu                | Texto se renderiza                   |
| Crear `WgpuContext` con surface, device, queue       | M          | wgpu                | Contexto inicializa                  |
| Cargar font del sistema con glyphon                  | M          | glyphon             | Font carga                           |
| Glyph atlas: textura 1024x1024                       | L          | wgpu                | Atlas se crea                        |
| Glyph cache con precarga ASCII                       | M          | atlas               | ASCII pre-cacheado                   |
| Mapear grid a coordenadas de pantalla                | M          | Grid, atlas         | Posicion correcta                    |
| Render de texto desde grid                           | L          | Atlas, cache, mapeo | Texto visible                        |
| Damage tracking (solo renderizar celdas modificadas) | L          | Render              | Render eficiente                     |
| Render de SGR (bold, italic, underline, reverse)     | M          | Render              | Estilos visibles                     |
| Integrar con event loop (redraw en UserEvent)        | M          | Event loop          | Render se actualiza al recibir bytes |
| Unit tests para grid, atlas, cache                   | M          | Implementacion      | Pasan                                |
| Benchmarks con criterion                             | M          | Implementacion      | criterion compila                    |

**Demo al final del Sprint 3:**

```bash
$ cargo run
# Ventana 800x600 con texto monospace
# 80x24 grid visible
# Prompt de bash con colores
# $ ls --color=auto
# Salida de ls con colores
# $ git status
# Salida de git con verde/rojo
# Resize de ventana se ve fluido (sin lag visible)
```

## 6. Fase 3: MVP Funcional

**Duracion total:** 2 sprints (~4 semanas). Fase mas
larga y riesgosa del proyecto.

**Objetivo:** integracion completa. El usuario puede
ejecutar comandos basicos, ver output con colores,
hacer clear, resize, y abrir apps TUI simples (vim,
htop).

### Sprint 4 (Semanas 7-8): TUI apps

| Tarea                                       | Estimacion | Dependencias     | Criterio de aceptacion      |
| ------------------------------------------- | ---------- | ---------------- | --------------------------- |
| Soporte de alternate screen (DEC 1049)      | L          | Fase 1           | vim/htop usan alt screen    |
| Backup de pantalla primaria al entrar alt   | M          | Alternate screen | Restauracion correcta       |
| Scroll región (DECSTBM)                     | M          | Grid             | Scroll respeta región       |
| DECSC/DECRC (save/restore cursor)           | M          | Cursor           | Save/restore funciona       |
| DECAWM (auto wrap)                          | M          | Cursor           | Wrap en ultima columna      |
| Insert/delete line (IL/DL)                  | M          | Grid             | IL/DL funcionan             |
| Insert/delete char (ICH/DCH)                | M          | Grid             | ICH/DCH funcionan           |
| Mouse parsing mínimo (no reporting todavia) | S          | Parser           | Eventos de mouse se ignoran |
| Unit tests para cada feature nueva          | M          | Implementacion   | Pasan                       |
| Integration test: vim abre y edita          | M          | Alt screen       | Test pasa                   |

### Sprint 5 (Semanas 9-10): Resize, copy/paste, polish

| Tarea                                   | Estimacion | Dependencias    | Criterio de aceptacion    |
| --------------------------------------- | ---------- | --------------- | ------------------------- |
| SIGWINCH: `ioctl(TIOCSWINSZ)` al resize | M          | Event loop      | bash ajusta COLUMNS       |
| Resize del grid al cambiar tamano       | M          | SIGWINCH        | Grid se redimensiona      |
| Resize del renderer al cambiar tamano   | M          | SIGWINCH        | Render se actualiza       |
| Clipboard X11/Wayland basico            | L          | Event loop      | Ctrl+Shift+C/V funciona   |
| Bracketed paste mode (DEC 2004)         | M          | Parser          | Pegado respeta mode       |
| Filtrar ESC/ETX del paste               | S          | Bracketed paste | Sin inyección             |
| Scroll basico (sin reflow)              | M          | Grid            | Scroll up/down funciona   |
| Decodificar 7-bit y 8-bit control       | S          | Parser          | Ambos funcionan           |
| Manejar errores de I/O del PTY          | M          | Hilo PTY        | Errores no panic          |
| Shutdown graceful con SIGHUP            | M          | Pty::Drop       | Child recibe SIGHUP       |
| 10-min smoke test (sesión manual)       | L          | Todo            | 0 panics en 10 min        |
| Documentacion de usuario basica         | M          | Todo            | README explica uso basico |

**Demo al final del Sprint 5 (MVP completo):**

```bash
$ cargo run
# Ventana se abre
# vim archivo.txt -> edita, guarda, sale
# htop -> muestra procesos, navega, sale
# less archivo_grande -> navega, sale
# tmux -> crea sesiones, multiples paneles
# ssh usuario@host -> conecta, trabaja, sale
# Ctrl+Shift+C/V -> copy/paste
# Resize de ventana -> contenido se ajusta
# Cerrar ventana -> child recibe SIGHUP, no hay huerfanos
```

## 7. Fase 4: Refinamiento

**Duracion total:** 2 sprints (~4 semanas)

**Objetivo:** reflow de lineas al resize, selección
de texto con mouse, mouse reporting, scrollback 100
lineas.

### Sprint 6 (Semanas 11-12): Reflow y scrollback

| Tarea                                     | Estimacion | Dependencias | Criterio de aceptacion |
| ----------------------------------------- | ---------- | ------------ | ---------------------- |
| Reflow de lineas al resize                | L          | Grid         | Lineas se re-dividen   |
| Scrollback ring buffer (100 lineas MVP)   | M          | Grid         | Scrollback funciona    |
| PageUp/PageDown navega scrollback         | M          | Scrollback   | Teclas funcionan       |
| Reflow solo en pantalla primaria          | M          | Reflow       | Alt screen sin reflow  |
| Benchmarks de scroll latency              | S          | Scrollback   | criterion compila      |
| Integration test: comando con 1000 lineas | M          | Scrollback   | Test pasa              |

### Sprint 7 (Semanas 13-14): Mouse y selection

| Tarea                                              | Estimacion | Dependencias     | Criterio de aceptacion |
| -------------------------------------------------- | ---------- | ---------------- | ---------------------- |
| Render del cursor de mouse                         | M          | Renderer         | Cursor visible         |
| Click + drag selecciona texto                      | L          | Mouse, selection | Seleccion visual       |
| Triple-click selecciona linea                      | M          | Selection        | Funciona               |
| Doble-click selecciona palabra                     | M          | Selection        | Funciona               |
| Mouse reporting SGR (1006)                         | L          | Parser           | vim recibe eventos     |
| Mouse reporting Normal (1000)                      | M          | Parser           | Mouse basico funciona  |
| Shift bypass para selección local                  | S          | Mouse            | Shift+click selecciona |
| Copy de selección al clipboard                     | M          | Selection        | Copia funciona         |
| 4 modos de selección (Simple/Block/Semantic/Lines) | L          | Selection        | Todos funcionan        |
| 10-min smoke test con mouse                        | M          | Mouse            | Sin panics             |

**Demo al final del Sprint 7:**

```bash
$ cargo run
# Resize a ventana mas pequena -> lineas se re-dividen
# Resize a ventana mas grande -> lineas se fusionan
# Scrollback: ejecutar "for i in {1..200}; do echo $i; done"
#   -> se ven 200 lineas, PageUp/PageDown navega
# vim con mouse: click posiciona cursor, drag selecciona
# Click + Ctrl+Shift+C -> copia al clipboard
# Mouse reporting: tmux responde a clicks
```

## 8. Fase 4.5: Extensiones y Personalizacion

**Duracion total:** 2 sprints (~4 semanas)

**Objetivo:** agregar archivo de configuracion TOML, expandir el modelo de color a
True Color (24-bit) y 256 colores, permitir personalizacion de tema, fuente y
transparencia de ventana. Sin alterar la funcionalidad existente.

### Sprint 8A1 (Semanas 19-20): Config TOML, color y SGR

| Tarea                                           | Estimacion | Dependencias        | Criterio de aceptacion          |
| ----------------------------------------------- | ---------- | ------------------- | ------------------------------- |
| Config TOML con serde (carga ~/.config/baud/)   | M          | serde + toml + dirs | Archivo se lee al inicio        |
| Tema configurable en TOML (16 ANSI + 8 brights) | M          | Config TOML         | Tema se aplica al renderer      |
| Expansion Color enum (Bright, Indexed, Rgb)     | L          | -                   | Compila con 19 variantes        |
| SGR handler 90-97, 100-107, 38;5, 38;2, 48;5    | L          | Color enum          | Programas usan true color       |
| Paleta 256 colores (6x6x6 cube + 24 grises)     | M          | Color enum          | color_to_glyphon mapea correcto |
| Integracion de Config en App                    | S          | Config TOML         | App carga config al iniciar     |
| Tests de color, SGR y config                    | M          | Implementacion      | Pasan                           |

### Sprint 8A2 (Semanas 21-22): Fuente y transparencia

| Tarea                                          | Estimacion | Dependencias          | Criterio de aceptacion      |
| ---------------------------------------------- | ---------- | --------------------- | --------------------------- |
| Fuente configurable (family + size) en TOML    | M          | Config TOML           | Font cambia segun config    |
| Transparencia de ventana (opacity)             | L          | winit + wgpu + Config | Fondo translucido funcional |
| Integracion fina del tema (selection_bg, etc.) | S          | Sprint A1             | Tema se aplica a seleccion  |
| Tests de fuente y transparencia                | M          | Implementacion        | Pasan                       |

**Demo al final de Sprint A2:**

```bash
$ cargo run --release
# Ventana con fondo semitransparente
# ~/.config/baud/config.toml cambia colores, fuente, opacidad
# vim con syntax highlighting en true color
# ls --color usa 256 colores
# Temas Catppuccin, Tokyo Night, etc. desde el TOML
```

## 9. Fase 5: Produccion

**Duracion total:** 2 sprints (~4 semanas)

**Objetivo:** testing exhaustivo, performance 60fps en
200x50, benchmarks, packaging, portabilidad.

### Sprint 9 (Semanas 23-24): Testing exhaustivo

**Objetivo:** alcanzar cobertura >50%, property-based testing con proptest, integracion de vttest/esctest, benchmarks en CI, y documentacion de API con rustdoc.

| Tarea                                               | Estimacion | Dependencias             | Criterio de aceptacion                        |
| --------------------------------------------------- | ---------- | ------------------------ | --------------------------------------------- |
| proptest como dev-dep                               | S          | -                        | `cargo test` incluye tests property-based     |
| Property tests: grid (reflow, resize, scrollback)   | L          | proptest                 | 1000 casos aleatorios, invariantes se cumplen |
| Property tests: parser ANSI (SGR, secuencias)       | L          | proptest                 | Cualquier secuencia valida no causa panic     |
| Property tests: selection (coordenadas, normalize)  | M          | proptest                 | normalize() siempre produce start <= end      |
| Property tests: color mapping (Indexed, Rgb)        | M          | proptest + Sprint 8A1    | 256 colores mapean a RGB sin panic            |
| vttest categorias 1-4: guia de ejecucion            | M          | Build release            | Documento con pasos + resultado esperado      |
| esctest subset critico (~30 tests)                  | M          | Build release            | Todos los tests del subset pasan              |
| Cobertura cargo-tarpaulin >50%                      | L          | Tests + proptest         | `cargo tarpaulin --out Html` reporta >50%     |
| Benchmarks en CI (cargo bench)                      | M          | criterion (ya existe)    | CI ejecuta benchmarks sin regresiones         |
| rustdoc API publica (config, ansi, selection, grid) | M          | Codigo con `///`         | `cargo doc --no-deps` genera HTML navegable   |
| CI: coverage + benchmarks + proptest                | M          | tarpaulin + CI existente | Jobs nuevos en GitHub Actions                 |

**Stack:**

```toml
[dev-dependencies]
criterion = { version = "0.8", features = ["html_reports"] }
proptest = "1"
```

**Herramientas externas (no son deps Rust):**

- `cargo-tarpaulin` — cobertura (`cargo install cargo-tarpaulin`)
- `vttest` — suite VT100/VT520 (`sudo pacman -S vttest`)
- `esctest` — suite escapes ANSI (`git clone https://github.com/esctest/esctest`)

**Demo al final:**

```bash
cd /home/carloscc/Documentos/Dev/baud
cargo test 2>&1 | tail -5          # 200+ tests pasan
cargo tarpaulin --out Html          # >50% cobertura
cargo bench 2>&1 | head -10        # benchmarks corren
cargo doc --no-deps                # docs generadas
vttest ./target/release/baud       # categorias 1-4 pasan
```

**Criterios de exito:**

- [ ] `cargo test` pasa con 200+ tests
- [ ] `cargo tarpaulin` reporta >50%
- [ ] `cargo bench` corre en CI sin errores
- [ ] `cargo doc --no-deps` genera documentacion
- [ ] vttest categorias 1-4 pasan (verificacion manual)
- [ ] esctest subset critico pasa
- [ ] CI verde (fmt + clippy + test + bench + coverage)
- [ ] Sin dependencias nuevas fuera de proptest

---

### Sprint 10 (Semanas 25-26): Performance y packaging

| Tarea                                       | Estimacion | Dependencias | Criterio de aceptacion     |
| ------------------------------------------- | ---------- | ------------ | -------------------------- |
| Optimizar parser (>500 MB/s)                | M          | Parser       | Benchmark cumple           |
| Optimizar scroll (<1ms)                     | M          | Grid         | Benchmark cumple           |
| Panic hook custom (Fase 5 segun ADR-0007)   | M          | Logging      | Panic muestra notificacion |
| Script de build con `cargo build --release` | S          | -            | Binario produccion         |
| AppImage para distribucion Linux            | M          | Build        | AppImage funcional         |
| Documentacion de usuario completa           | M          | Todo         | README + manpage           |
| Portabilidad macOS (opcional)               | L          | Refactor     | Compila en macOS           |

**Demo al final del Sprint 10 (Produccion):**

```bash
$ cargo build --release
# Binario en target/release/baud, <15MB

$ ./target/release/baud
# 60fps en 200x50
# <100MB de memoria
# vttest categoria 1-4: 100% pass
# 0 panics en 1 hora de uso
# Panic hook: muestra notificacion si ocurre
```

## 10. Milestones

| Milestone         | Sprint        | Entregable visible  |
| ----------------- | ------------- | ------------------- |
| M0: Hello PTY     | Fin Sprint 1  | bash en ventana     |
| M1: ANSI basico   | Fin Sprint 2  | Colores en pantalla |
| M2: Render GPU    | Fin Sprint 3  | Grid 80x24 visible  |
| M3: MVP funcional | Fin Sprint 5  | vim/htop funcionan  |
| M4: Refinamiento  | Fin Sprint 7  | Mouse y reflow      |
| M5: Produccion    | Fin Sprint 10 | Release 0.0.1       |

## 11. Riesgos y Mitigaciones por Fase

| Fase | Riesgo                          | Mitigacion                               |
| ---- | ------------------------------- | ---------------------------------------- |
| 0    | nix API confusa                 | Documentar con ejemplos de Alacritty     |
| 1    | Secuencias ANSI no documentadas | Usar xterm ctlseqs como referencia       |
| 2    | Performance del render          | Empezar con mock, optimizar al final     |
| 3    | Bugs de integracion             | Testing continuo, no dejar para el final |
| 4    | Reflow complejo                 | Copiar implementación de Alacritty       |
| 5    | CI ruidoso                      | Threshold del 15% en benchmarks          |

## 12. Limitaciones

1. **El timeline asume desarrollador solo.** Con 2+
   desarrolladores, se puede paralelizar Fase 2 y 3.
2. **Las estimaciones son optimistas.** Agregar 30%
   de buffer para imprevistos.
3. **Fase 5 incluye portabilidad macOS opcional.** Si
   se requiere Windows, agregar 2-3 sprints mas.
4. **Los benchmarks en CI son ruidosos.** Se acepta
   varianza del 15%.
5. **No incluye plan de contribucion externa.** Si
   llegan PRs, agregar 1 sprint de code review.

## 13. Referencias

- docs/decisions/ADR-0008-roadmap-mvp.md (decision  
  de alto nivel).
- docs/specs/requisitos.md (RF y RNF detallados).
- Mitchell Hashimoto. Ghostty development blog.
  [https://mitchellh.com/ghostty](https://mitchellh.com/ghostty)
- Joe Wilm. "Life of a Terminal Emulator".
  [https://jwilm.io/blog/](https://jwilm.io/blog/)

## Cambios

| Version | Fecha      | Cambios                                                                                                  |
| ------- | ---------- | -------------------------------------------------------------------------------------------------------- |
| 0.1.0   | 2026-06-14 | Primer borrador. 9 sprints detallados, dependencias, criterios.                                          |
| 0.2.0   | 2026-06-24 | Agregada Fase 4.5 con Sprint A1 (Config+Color) y A2 (Fuente+Transparencia). Roadmap renumerado.          |
| 0.3.0   | 2026-06-24 | Sprint 9 actualizado con detalle completo: proptest, vttest, esctest, tarpaulin, benchmarks CI, rustdoc. |
