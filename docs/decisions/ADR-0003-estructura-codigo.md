```yaml
titulo: "ADR-0003: Estructura del Codigo en 3 Capas"
tipo: decision
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-14"
fecha_modificacion: "2026-06-14"
version: "0.1.0"
estado: aceptado
tags: [decision, arquitectura, capas, modulos, separacion]
```

# ADR-0003: Estructura del Codigo en 3 Capas

## Contexto

El proyecto necesita una division clara de responsabilidades
entre la captura de eventos, la lógica del terminal, y la
representacion visual. Sin esta division, el código se vuelve
monolitico y dificil de testear.

Los cinco terminales de referencia analizados (Alacritty,
WezTerm, Warp, Rio, Ghostty) adoptan una separacion en capas
con variantes. La pregunta es: cuantas capas y como se
definen.

Las iteraciones 0-5 del proyecto investigaron cada componente
por separado, pero sin consolidar como se integran. Esta
iteracion define la integracion.

## Decision

El código se organiza en tres capas con dependencias
unidireccionales:

- **Presentation:** winit (ventana, eventos), wgpu + glyphon
(render), input (clasificacion de teclas y mouse).
- **Domain:** grid (ring buffer + celdas), cursor, parser ANSI
(vte), selection, term (estado central que implementa
`Handler`).
- **Infrastructure:** pty (openpty + child process), tty
(abstraccion Unix/Windows), config (TOML + serde).

Las dependencias entre capas son unidireccionales:
Presentation importa Domain, Domain importa Infrastructure.
Infrastructure no conoce las capas superiores.

Cada capa se traduce a un modulo (o conjunto de modulos) en
el crate. El modulo raiz `term` actua como mediador entre
las tres capas y mantiene el estado compartido bajo
`Arc<FairMutex<...>>`.

## Justificacion

Tres razones principales, todas con evidencia verificada en
código de proyectos en produccion:

1. **Testabilidad aislada.** Alacritty separa
  `alacritty_terminal/` (lógica) de `alacritty/`
   (presentacion) precisamente para testear la lógica sin
   iniciar ventana grafica. WezTerm hace lo mismo con
   `term/` vs `wezterm-gui/`. Esta separacion reduce el
   tiempo de test y elimina dependencias fragiles en
   plataforma.
2. **Cambio de backend sin tocar la lógica.** Migrar de
  OpenGL a WebGPU, o de wgpu a glow, no debe requerir
   reescribir la lógica del terminal. La separacion
   presentation/domain hace que el cambio se limite a
   los archivos de render. Warp demostro esto al migrar
   de OpenGL (fork de Alacritty) a wgpu (su rama actual)
   sin tocar el grid ni el parser.
3. **Desarrollo paralelo.** Multiples contribuidores pueden
  trabajar en modulos de capas distintas sin conflictos
   de merge frecuentes. El proyecto se beneficia desde
   el inicio de esta caracteristica, aunque en MVP sera
   desarrollado por una sola persona.

## Alternativas Consideradas


| Alternativa                                             | Pros                                                  | Contras                                                                               | Veredicto                                 |
| ------------------------------------------------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------------- | ----------------------------------------- |
| Monolito (sin capas)                                    | Simple, menos archivos                                | Imposible testear sin GUI, refactor costoso a futuro                                  | Rechazada                                 |
| 2 capas (Core + GUI)                                    | Menos capas, archivos mas cohesivos                   | Mezcla domain con infra, lógica del parser acoplada a I/O                             | Rechazada                                 |
| 5+ capas (Clean Architecture estricta)                  | Maxima pureza teorica                                 | Sobre-ingenieria para un MVP, no usado por terminales de referencia                   | Rechazada                                 |
| **3 capas (Presentation/Domain/Infra)**                 | Balance, validado por 5 proyectos, testable, portable | Requiere disciplina, definir interfaces claras entre capas                            | **Seleccionada**                          |
| 4 capas (con "Application" entre Presentation y Domain) | Capa extra para casos de uso                          | Los 5 terminales de referencia no la usan; agrega complejidad sin beneficio inmediato | Descartada para MVP, considerar en Fase 5 |


## Consecuencias

### Positivas

- Testabilidad aislada: la capa de domain se testea con
mocks de PTY, sin necesidad de GUI.
- Cambio de render sin tocar lógica: migrar de wgpu a
glow (o viceversa) afecta solo `src/renderer/`.
- Paralelismo entre contribuidores (cuando los haya).
- Reutilizacion: la capa de domain podria usarse para un
frontend alternativo (TUI, web) sin reescribir.

### Negativas

- Requiere definir interfaces claras entre capas (trait
`Handler` en vte, trait `Drawable` en renderer, trait
`PtyBackend` en infraestructura).
- Overhead de indireccion: cada llamada cruza al menos
un trait boundary. El compilador de Rust puede
inlinear en release, pero el costo de diseno es real.
- Mas archivos: el proyecto tendra ~25 archivos .rs
desde el inicio, vs ~10 con monolitico.

### Mitigacion

- Las interfaces entre capas se documentan en el doc
maestro 06-arquitectura.md, sección "Modulos
Principales" y "Implementacion Detallada por
Componente".
- Se usan traits minimalistas (1-2 métodos) para
minimizar el overhead.
- La estructura de directorios se valida en CI con
`cargo test --no-run` que falla si hay ciclos de
import.

## Referencias

- docs/research/00-fundamentos.md (vision general de
arquitectura en 3 capas).
- docs/research/05-terminal-grid.md (capa de domain
detallada).
- docs/research/04-ansi-parser.md (capa de domain:
parser).
- docs/research/01-pty-shell.md (capa de
infrastructure: PTY).
- docs/prompts/iter-06-investigacion-A.md (investigacion
especifica para esta decision, 559 lineas).
- [https://github.com/alacritty/alacritty](https://github.com/alacritty/alacritty) (separacion
alacritty_terminal/ vs alacritty/).
- [https://github.com/wez/wezterm](https://github.com/wez/wezterm) (separacion term/ vs
wezterm-gui/).
- [https://github.com/ghostty-org/ghostty](https://github.com/ghostty-org/ghostty) (separacion
language-agnostic: terminal.zig, input.zig, Surface.zig).

## Cambios


| Version | Fecha      | Cambios                             |
| ------- | ---------- | ----------------------------------- |
| 0.1.0   | 2026-06-14 | Primer borrador. Decision adoptada. |


