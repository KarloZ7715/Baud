```yaml
titulo: "ADR-0004: Seleccion Final de Crates"
tipo: decision
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-14"
fecha_modificacion: "2026-06-25"
version: "0.4.0"
estado: aceptado
tags: [decision, crates, dependencias, mvp, msrc, wgpu, config, serde, toml]
```

# ADR-0004: Seleccion Final de Crates

## Contexto

El proyecto necesita definir la lista completa de crates
para la primera version implementable. Las iteraciones
previas investigaron componentes individuales pero sin
consolidar la seleccion de dependencias.

La MSRV objetivo del proyecto es 1.75 (release estable
de hace ~6 meses) o superior. Sin embargo, la seleccion
de wgpu como backend de render impone una MSRV mayor.

## Decision

Se seleccionan los siguientes 11 crates para `[dependencies]`
y 1 para `[dev-dependencies]`:

| Categoria          | Crate              | Version | MSRV               |
| ------------------ | ------------------ | ------- | ------------------ |
| Parser ANSI        | vte                | 0.15    | 1.65               |
| PTY                | nix                | 0.31    | 1.65               |
| Ventana            | winit              | 0.30    | 1.70               |
| Render GPU         | wgpu               | 29      | 1.87               |
| Texto              | glyphon            | 0.11    | (heredada de wgpu) |
| Unicode            | unicode-width      | 0.2     | 1.66               |
| Logging            | tracing            | 0.1     | 1.65               |
| Logging subscriber | tracing-subscriber | 0.3     | 1.65               |
| Serializacion      | serde              | 1       | 1.65               |
| Config TOML        | toml               | 1       | 1.70               |
| Directorios        | dirs               | 6       | 1.65               |
| Benchmarks (dev)   | criterion          | 0.8     | 1.86               |

La MSRV efectiva del proyecto queda en **1.87.0**,
determinada por wgpu. El `rust-version` en Cargo.toml se
fija en 1.87.0.

## Justificacion

1. **vte 0.15 sobre wezterm-escape-parser.** vte es
   mantenido por la comunidad GNOME (jamesnichols,
   christianpoveda) con releases frecuentes y 58M de
   downloads. wezterm-escape-parser es interno al monorepo
   de WezTerm y no se publica a crates.io. vte cubre
   el 100% de las secuencias necesarias para el MVP
   (verificado en iter 4).
2. **nix 0.31 sobre portable-pty.** nix es Unix-only
   pero mas simple, bien documentado, y usado por Warp
   en produccion. portable-pty es cross-platform pero
   agrega complejidad para soporte Windows que el MVP
   no requiere. Cuando se aborde macOS/Windows en Fase
   5, se evaluara migrar a portable-pty.
3. **winit 0.30 (la unica opcion realista).** winit es
   el estandar de facto para GUI en Rust, mantenido por
   la organizacion `rust-windowing` con multiples
   maintainers. No hay alternativa realista.
4. **wgpu 29 sobre glow (OpenGL).** wgpu es mas moderno,
   cross-platform via WebGPU, y mantenido por gfx-rs
   (mismo grupo que Vulkan y WebGPU en Firefox). El
   trade-off es la MSRV alta (1.87). glow (OpenGL)
   tendria MSRV mas baja (~1.70) pero requiere escribir
   codigo OpenGL tradicional, que es menos portable
   (especialmente a macOS donde OpenGL esta deprecado).
5. **glyphon 0.11 sobre crossfont.** glyphon se integra
   nativamente con wgpu (es la unica opcion mantenida
   para wgpu). crossfont (Alacritty) usa glutin (OpenGL).
   Si se hubiera elegido glow, la seleccion seria
   crossfont. La decision esta acoplada a la de render.
   Desde ADR-0009, Baud pinta el grid con `CustomGlyph`
   posicionados por celda (`left`/`top` = `col * cell_w`,
   `row * cell_h`), no con `TextArea` por fila.
6. **unicode-width 0.2.** Estandar de facto para calcular
   el ancho de caracteres Unicode (CJK de ancho 2,
   emojis ZWJ, etc.). Usado por Alacritty, WezTerm y
   Kitty. Sprint 6 lo agrega para reflow correcto de
   lineas con caracteres de ancho variable.
7. **tracing + tracing-subscriber.** Logging estructurado
   con spans y filtros por env. tracing-subscriber provee
   el subscriber con env-filter.
8. **criterion como dev-dependency.** criterion es el
   estandar de benchmarks en Rust. Solo se compila en
   `cargo bench`, no afecta el build de produccion.
9. **serde 1 con feature derive.** Estandar de facto para
   serializacion/deserializacion en Rust. Se usa exclusivamente
   para deserializar el archivo de configuracion TOML via
   `#[derive(Deserialize)]`. La feature `derive` habilita
   las macros de derivacion en tiempo de compilacion.
10. **toml 1.** Parseo de archivos TOML con la API
    `toml::from_str()`. Sigue el estándar TOML v1.0.
11. **dirs 6.** Provee `dirs::config_dir()` para resolver
    rutas estandar del sistema (`~/.config` en Linux,
    `~/Library/Application Support` en macOS,
    `%APPDATA%` en Windows).

## Alternativas Consideradas

| Decision | Alternativa           | Pros                         | Contras                                   | Veredicto             |
| -------- | --------------------- | ---------------------------- | ----------------------------------------- | --------------------- |
| Parser   | wezterm-escape-parser | Control total                | Interno a WezTerm, no en crates.io        | **vte 0.15**          |
| Parser   | vtparse               | Bajo nivel, WezTerm lo usa   | Requiere implementar Handler propio       | vte es mas practico   |
| PTY      | portable-pty          | Cross-platform               | Mas complejo que nix, dependencias extras | **nix 0.31** (MVP)    |
| PTY      | rustix-openpty        | Bajo nivel, Alacritty lo usa | rustix tiene API compleja, peor docs      | nix es mas claro      |
| Render   | glow (OpenGL)         | MSRV ~1.70                   | OpenGL deprecado en macOS, requiere FFI   | **wgpu 29**           |
| Render   | wgpu 27               | Version anterior             | Algunos bugs conocidos ya fixed en 28/29  | wgpu 29 (latest)      |
| Texto    | crossfont             | Maduro, Alacritty lo usa     | Requiere OpenGL, no compatible con wgpu   | **glyphon 0.11**      |
| Texto    | rusttype              | Bajo nivel, no GPU           | Lento, requiere atlas manual              | glyphon es superior   |
| Unicode  | calcular ancho manual | Sin deps                     | Incorrecto para CJK/emojis                | **unicode-width 0.2** |

## Consecuencias

### Positivas

- Stack verificado: cada crate tiene al menos un
  proyecto de referencia (Alacritty, WezTerm, Warp)
  que lo usa en produccion.
- Versiones verificadas contra crates.io API con
  HTTP 200.
- Documentacion mantenida por comunidades activas
  (rust-windowing para winit, gfx-rs para wgpu,
  GNOME para vte, David Tolnay para anyhow/thiserror).
- Licencias permisivas (MIT, Apache-2.0): compatibles
  con distribucion binaria.

### Negativas

- **MSRV 1.87.0.** Usuarios con Rust < 1.87 no pueden
  compilar. Esto excluye releases LTS de algunas
  distribuciones (Debian stable, RHEL) hasta su
  siguiente actualizacion.
- **glyphon es relativamente nuevo.** 817K downloads
  (vs 632M de unicode-width). Depende de wgpu y
  cosmic-text. Si glyphon se discontinua, hay que
  migrar.
- **11 dependencias en `[dependencies]`.** Aceptable
  pero requiere disciplina para mantenerlas
  actualizadas. Se usa `cargo update` y `cargo audit`
  en CI.

### Mitigacion

- La MSRV alta se documenta explicitamente en
  `README.md` y en el mensaje de error si la
  compilacion falla.
- Si glyphon se discontinua, la alternativa es
  cosmic-text directo con un shim propio (~200
  lineas), o migrar a OpenGL con rusttype.
- Las dependencias se actualizan mensualmente con
  `cargo update` y se validan con CI antes de cada
  release.

## Referencias

- docs/prompts/iter-06-investigacion-C.md (investigacion
  completa, 519 lineas, 33 URLs verificadas HTTP 200).
- docs/research/01-pty-shell.md (analisis de PTY).
- docs/research/02-rendering.md (analisis de render).
- docs/research/04-ansi-parser.md (analisis de parser).
- [https://crates.io/crates/vte](https://crates.io/crates/vte)
- [https://crates.io/crates/nix](https://crates.io/crates/nix)
- [https://crates.io/crates/winit](https://crates.io/crates/winit)
- [https://crates.io/crates/wgpu](https://crates.io/crates/wgpu)
- [https://crates.io/crates/glyphon](https://crates.io/crates/glyphon)
- [https://crates.io/crates/unicode-width](https://crates.io/crates/unicode-width)
- [https://crates.io/crates/tracing](https://crates.io/crates/tracing)
- [https://crates.io/crates/criterion](https://crates.io/crates/criterion)
- [https://crates.io/crates/serde](https://crates.io/crates/serde)
- [https://crates.io/crates/toml](https://crates.io/crates/toml)
- [https://crates.io/crates/dirs](https://crates.io/crates/dirs)

## Cambios

| Version | Fecha      | Cambios                                                                                                               |
| ------- | ---------- | --------------------------------------------------------------------------------------------------------------------- |
| 0.1.0   | 2026-06-14 | Primer borrador. Decision adoptada. 14 deps + 1 dev-dep, MSRV 1.87.0.                                                 |
| 0.2.0   | 2026-06-20 | Add arboard 3.4 para clipboard (Sprint 5b). 15 deps + 1 dev-dep.                                                      |
| 0.3.0   | 2026-06-22 | Sincronizar con stack real del proyecto: 8 deps + 1 dev-dep. Agregar unicode-width 0.2 y criterion 0.8 para Sprint 6. |
| 0.4.0   | 2026-06-25 | Agregar serde 1, toml 1, dirs 6 para Sprint A1. 11 deps total.       |
