```yaml
titulo: "ADR-0008: Roadmap de Implementacion y MVP"
tipo: decision
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-14"
fecha_modificacion: "2026-06-14"
version: "0.1.0"
estado: aceptado
tags: [decision, mvp, roadmap, fases, implementacion, rf, rnf]
```

# ADR-0008: Roadmap de Implementacion y MVP

## Contexto

El proyecto necesita definir el alcance del MVP (Minimum
Viable Product) y como se subdivide la implementación en
fases incrementales.

Los 5 terminales de referencia (Alacritty, WezTerm, Warp,
Rio, Ghostty) se desarrollaron en multiples anos con
distintos enfoques. Alacritty empezo con un POC
(`eduterm` de Joe Wilm) y crecio en ~7 anos a 40k
lineas. WezTerm empezo como multiplexer y crecio a
~200k lineas. Warp es un fork de Alacritty con
modificaciones.

La pregunta para el MVP es: que debe poder hacer el
emulador al final de la primera fase de desarrollo?

## Decision

El MVP se subdivide en **6 fases incrementales** con
dependencias lineales (cada fase depende de la anterior):

### Fase 0: Esqueleto + PTY (Sprint 1)

**Objetivo:** ventana winit que se abre, PTY creado, bash
arranca, output basico se ve (puede ser texto sin render
elaborado).

**Criterios de exito verificables:**

- `cargo run` abre una ventana de ~800x600.
- En la ventana se ve el prompt de bash (puede ser
  caracteres sin color).
- Escribir `echo hola` y presionar Enter muestra `hola`.
- `Ctrl+C` mata el comando sin cerrar la ventana.
- Cerrar la ventana (X button) termina el proceso.

**RF/RNF cubiertos:** RF-01, RF-02.

**Complejidad:** M. (1-2 semanas para un desarrollador
solo)

**Dependencias:** Ninguna (fase inicial).

### Fase 1: Parser ANSI basico (Sprint 2)

**Objetivo:** el parser vte reconoce SGR (color), cursor
movement, clear screen/line, y escribe al grid.

**Criterios de exito verificables:**

- `echo -e "\e[31mrojo\e[0m"` muestra texto en rojo.
- `echo -e "\e[2J"` limpia la pantalla.
- `echo -e "\e[5;10H"` mueve el cursor a (5, 10).
- `ls --color=auto` muestra colores basicos.

**RF/RNF cubiertos:** RF-03, parte de RF-04.

**Complejidad:** M.

**Dependencias:** Fase 0.

### Fase 2: Grid basico + Render (Sprint 3)

**Objetivo:** grid de 80x24 se renderiza en pantalla con
wgpu y glyphon. Texto monospace, 16 colores, SGR basico
(bold, italic, underline, reverse).

**Criterios de exito verificables:**

- 80 columnas x 24 filas exactas.
- Cada celda con su glyph correspondiente al carácter.
- Color de foreground y background aplicados.
- Bold se renderiza con variante bold de la fuente.
- El grid se actualiza en <16ms por frame.

**RF/RNF cubiertos:** RF-05, RF-06, RNF-01 (parcial).

**Complejidad:** M.

**Dependencias:** Fase 1.

### Fase 3: MVP funcional (Sprint 4)

**Objetivo:** integracion completa. El usuario puede
ejecutar comandos basicos, ver output con colores, hacer
clear, resize, y abrir apps TUI simples (vim, htop).

**Criterios de exito verificables:**

- `vim` abre correctamente, se puede editar y salir.
- `htop` muestra su interfaz correctamente.
- Resize de ventana recalcula grid y envia SIGWINCH.
- `clear` (Ctrl+L) limpia pantalla.
- `Ctrl+Shift+C` copia selección, `Ctrl+Shift+V` pega.
- 0 panics en uso normal de 10 minutos con comandos
  variados.

**RF/RNF cubiertos:** RF-07, RF-08, RF-09, RNF-04
(parcial).

**Complejidad:** L. (3-4 semanas, fase mas dificil del
proyecto)

**Dependencias:** Fase 2.

### Fase 4: Refinamiento (Sprint 5-6)

**Objetivo:** reflow de lineas al resize, selección de
texto con mouse, mouse reporting, scrollback 100 lineas.

**Criterios de exito verificables:**

- Resize a ventana mas pequena re-divide las lineas
  largas en multiples.
- Click izquierdo posiciona cursor, drag selecciona.
- Triple-click selecciona linea completa.
- PageUp/PageDown navega el scrollback.
- Scrollback retiene al menos 100 lineas.

**RF/RNF cubiertos:** RF-10, RF-11, RF-12, parte de
RNF-04.

**Complejidad:** L.

**Dependencias:** Fase 3.

### Fase 5: Produccion (Sprint 7-8)

**Objetivo:** testing exhaustivo, performance 60fps en
200x50, benchmarks, packaging, portabilidad.

**Criterios de exito verificables:**

- vttest categorías 1-4 pasan al 100%.
- 60fps en grid 200x50 con scroll activo.
- <16ms de latencia input a display.
- <100MB de uso de memoria.
- Binario distribuible para Linux (AppImage, .deb).
- CI verde en cada PR.
- Documentacion de usuario completa.

**RF/RNF cubiertos:** RNF-01, RNF-02, RNF-03, RNF-05,
RNF-06, refinamiento de RNF-04.

**Complejidad:** L.

**Dependencias:** Fase 4.

### Alcance del MVP

**Incluido en el MVP (Fases 0-3):**

- Arranque del shell (bash) del usuario.
- Input basico de teclado (caracteres, Enter, Backspace,
  Ctrl+C, Ctrl+D, flechas).
- Parser ANSI basico (SGR, cursor, clear, scroll).
- Grid 80x24 con 16 colores.
- Render GPU con wgpu y glyphon.
- Resize de ventana.
- Copy/Paste basico.
- Alternate screen (necesario para vim, htop).

**Excluido del MVP (se aborda en Fases 4-5):**

- Reflow al resize.
- Seleccion de texto con mouse.
- Mouse reporting (SGR, etc.).
- Scrollback extenso (>100 lineas).
- Tabs (HTS).
- Multiple ventanas.
- Configuracion de usuario.
- True color (24-bit).
- Temas.
- IME para CJK.
- macOS/Windows.
- Performance optimizada para grids grandes.
- Kitty keyboard protocol.
- Sixel, iTerm2 image protocol, Kitty graphics protocol.

## Justificacion

1. **Las fases son lineal-dependientes.** Cada fase
   produce un demo verificable, lo que permite
   detectar problemas temprano. Si Fase 0 no funciona,
   no tiene sentido continuar.

2. **Fase 3 es el MVP verdadero.** Las primeras 3
   fases construyen las piezas; Fase 3 las integra.
   El usuario puede usar el emulador para trabajo
   basico al final de Fase 3.

3. **Complejidad L en Fase 3 es esperada.** Es la
   fase donde se descubren los bugs de integracion.
   El proyecto reserva mas tiempo aqui.

4. **Fase 4-5 son mejoras incrementales.** No son
   bloqueantes para el MVP. Se priorizan segun
   necesidad del usuario.

5. **Excluir configuracion en MVP simplifica.** El
   MVP usa valores hardcodeados. La configuracion
   TOML se agrega cuando el comportamiento base
   esta estable.

6. **Linux primero es pragmatico.** macOS y Windows
   requieren testing adicional que no aporta valor
   al MVP. Se aborda cuando el proyecto este
   maduro.

## Alternativas Consideradas

| Alternativa                         | Pros                            | Contras                                                 | Veredicto                                 |
| :---------------------------------- | :------------------------------ | :------------------------------------------------------ | :---------------------------------------- |
| 3 fases (Esqueleto, Parser, Render) | Menos planeacion                | No cubre resize, TUI apps, copy/paste                   | Rechazada (insuficiente)                  |
| **6 fases (0-5)**                   | Balance, cada fase produce demo | Mas planeacion                                          | **Seleccionada**                          |
| 10+ fases (granular)                | Mas checkpoints                 | Overhead administrativo, fases de 1-2 dias son fragiles | Descartado                                |
| MVP = todo lo de Fase 5             | Sin refinamiento posterior      | Riesgo de nunca terminar                                | Rechazada (Fase 5 incluye optimizaciones) |
| Empezar por render sin PTY          | Ver algo rápido                 | No es un terminal sin PTY                               | Descartado                                |
| Empezar por parser sin GUI          | Logica primero                  | Requiere mocks complejos                                | Descartado                                |
| Cross-platform desde el inicio      | Alcance amplio                  | 3x el trabajo, debugging en 3 SOs                       | Descartado para MVP                       |

## Consecuencias

### Positivas

- Plan claro: el desarrollador sabe que hacer cada
  sprint.
- Demo verificable cada fase: el usuario ve
  progreso tangible.
- MVP alcanzable: al final de Fase 3 (4 sprints)
  el emulador es usable.
- Refinamiento aislado: Fases 4-5 no bloquean el
  MVP.

### Negativas

- **8 sprints (~4 meses) para Fase 5.** Es un
  compromiso de tiempo significativo.
- **Fase 3 es la mas riesgosa.** Si los problemas
  de integracion son mas severos de lo estimado, el
  MVP se retrasa.
- **Linux-only en MVP.** Usuarios de macOS/Windows
  deben esperar a Fase 5.
- **No cubre features avanzadas en MVP.** Sixel,
  true color, imagenes, etc. quedan fuera.

### Mitigacion

- Si Fase 3 se atrasa, se evalua partirla en dos
  (3a: resize + alternate screen, 3b: copy/paste).
- El proyecto acepta contribuciones para acortar
  tiempos.
- La documentacion cubre claramente que esta en
  MVP y que no, para evitar expectativas
  incorrectas.

## Referencias

- docs/prompts/iter-06-investigacion-F.md
  (investigacion completa, 834 lineas, 11 URLs
  verificadas HTTP 200).
- docs/specs/requisitos.md (12 RF + 6 RNF
  detallados).
- docs/specs/roadmap.md (spec técnica con detalle
  operativo por fase).
- docs/research/01-pty-shell.md (Fase 0).
- docs/research/02-rendering.md (Fase 2).
- docs/research/04-ansi-parser.md (Fase 1).
- docs/research/05-terminal-grid.md (Fases 2-4).
- Mitchell Hashimoto. Ghostty development blog.
  https://mitchellh.com/ghostty
- Joe Wilm. "Life of a Terminal Emulator". Serie de
  articulos sobre Alacritty.

## Cambios

| Version | Fecha      | Cambios                                                     |
| :------ | :--------- | :---------------------------------------------------------- |
| 0.1.0   | 2026-06-14 | Primer borrador. Decision adoptada. 6 fases, MVP en Fase 3. |
