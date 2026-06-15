```yaml
titulo: "ADR-0004: Seleccion Final de Crates"
tipo: decision
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-14"
fecha_modificacion: "2026-06-14"
version: "0.1.0"
estado: aceptado
tags: [decision, crates, dependencias, mvp, msrc, wgpu]
```

# ADR-0004: Seleccion Final de Crates

## Contexto

El proyecto necesita definir la lista completa de crates
para la primera versión implementable. Las iteraciones
previas investigaron componentes individuales pero sin
consolidar la selección de dependencias.

La MSRV objetivo del proyecto es 1.75 (release estable  
de hace ~6 meses) o superior. Sin embargo, la selección  
de wgpu como backend de render impone una MSRV mayor.

## Decision

Se seleccionan los siguientes 14 crates para `[dependencies]`
y 1 para `[dev-dependencies]`:

| Categoria        | Crate         | Version | MSRV               |
| ---------------- | ------------- | ------- | ------------------ |
| Parser ANSI      | vte           | 0.15    | 1.65               |
| PTY              | nix           | 0.31    | 1.65               |
| Ventana          | winit         | 0.30    | 1.70               |
| Render GPU       | wgpu          | 29      | 1.87               |
| Texto            | glyphon       | 0.11    | (heredada de wgpu) |
| Unicode          | unicode-width | 0.2     | 1.66               |
| Flags            | bitflags      | 2       | 1.56               |
| Logging          | tracing       | 0.1     | 1.65               |
| Errores entry    | anyhow        | 1       | 1.68               |
| Errores domain   | thiserror     | 2       | 1.68               |
| Paths            | dirs          | 6       | N/A                |
| Serializacion    | serde         | 1       | 1.56               |
| CLI              | clap          | 4       | 1.85               |
| Lock             | parking_lot   | 0.12    | 1.65               |
| Benchmarks (dev) | criterion     | 0.8     | 1.86               |

La MSRV efectiva del proyecto queda en **1.87.0**,
determinada por wgpu. El `rust-versión` en Cargo.toml se
fija en 1.87.0.

## Justificacion

1. **vte 0.15 sobre wezterm-escape-parser.** vte es
   mantenido por la comunidad GNOME (jamesnichols,
   christianpoveda) con releases frecuentes y 58M de
   downloads. wezterm-escape-parser es interno al monorepo
   de WezTerm y no se pública a crates.io. vte cubre
   el 100% de las secuencias necesarias para el MVP
   (verificado en iter 4).
2. **nix 0.31 sobre portable-pty.** nix es Unix-only
   pero mas simple, bien documentado, y usado por Warp
   en produccion. portable-pty es cross-platform pero
   agrega complejidad para soporte Windows que el MVP
   no requiere. Cuando se aborde macOS/Windows en Fase
   5, se evaluara migrar a portable-pty.
3. **winit 0.30 (la unica opcion realista).** winit es
   el estándar de facto para GUI en Rust, mantenido por
   la organizacion `rust-windowing` con multiples
   maintainers. No hay alternativa realista.
4. **wgpu 29 sobre glow (OpenGL).** wgpu es mas moderno,
   cross-platform vía WebGPU, y mantenido por gfx-rs
   (mismo grupo que Vulkan y WebGPU en Firefox). El
   trade-off es la MSRV alta (1.87). glow (OpenGL)
   tendria MSRV mas baja (~1.70) pero requiere escribir
   código OpenGL tradicional, que es menos portable
   (especialmente a macOS donde OpenGL esta deprecado).
5. **glyphon 0.11 sobre crossfont.** glyphon se integra
   nativamente con wgpu (es la unica opcion mantenida
   para wgpu). crossfont (Alacritty) usa glutin (OpenGL).
   Si se hubiera elegido glow, la selección seria
   crossfont. La decision esta acoplada a la de render.
6. **anyhow en bordes + thiserror en domain.** Warp
   documenta este patron y la comunidad Rust lo
   recomienda: anyhow permite contexto dinamico
   (`.context("...")`) sin definir tipos, ideal para
   main.rs y handlers de UI. thiserror permite definir
   tipos `Error` propios con `#[derive(Error)]`,
   ideal para modulos de domain que necesitan propagar
   errores tipados.
7. **parking_lot sobre std::sync::Mutex.** parking_lot
   es ~3x mas rápido en benchmarks, tiene FairMutex
   (que evita starvation), y es usado por Alacritty.
   Costo: una dependencia mas, pero es de las mas
   descargadas de Rust (580M downloads).
8. **criterion como dev-dependency.** criterion es el
   estándar de benchmarks en Rust. Solo se compila en
   `cargo bench`, no afecta el build de produccion.

## Alternativas Consideradas

| Decision | Alternativa           | Pros                         | Contras                                   | Veredicto              |
| -------- | --------------------- | ---------------------------- | ----------------------------------------- | ---------------------- |
| Parser   | wezterm-escape-parser | Control total                | Interno a WezTerm, no en crates.io        | **vte 0.15**           |
| Parser   | vtparse               | Bajo nivel, WezTerm lo usa   | Requiere implementar Handler propio       | vte es mas práctico    |
| PTY      | portable-pty          | Cross-platform               | Mas complejo que nix, dependencias extras | **nix 0.31** (MVP)     |
| PTY      | rustix-openpty        | Bajo nivel, Alacritty lo usa | rustix tiene API compleja, peor docs      | nix es mas claro       |
| Render   | glow (OpenGL)         | MSRV ~1.70                   | OpenGL deprecado en macOS, requiere FFI   | **wgpu 29**            |
| Render   | wgpu 27               | Version anterior             | Algunos bugs conocidos ya fixed en 28/29  | wgpu 29 (latest)       |
| Texto    | crossfont             | Maduro, Alacritty lo usa     | Requiere OpenGL, no compatible con wgpu   | **glyphon 0.11**       |
| Texto    | rusttype              | Bajo nivel, no GPU           | Lento, requiere atlas manual              | glyphon es superior    |
| Errores  | solo anyhow           | Simple                       | Pierde tipos en domain                    | **anyhow + thiserror** |
| Errores  | solo thiserror        | Tipos en todos lados         | Verbose en main.rs                        | igual                  |
| Logging  | log crate             | Maduro                       | Sin spans, limitado                       | **tracing 0.1**        |
| Lock     | std::sync::Mutex      | Sin deps                     | Mas lento, no FairMutex                   | **parking_lot**        |

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
- **14 dependencias en `[dependencies]`.** Aceptable
  pero requiere disciplina para mantenerlas
  actualizadas. Se usa `cargo update` y `cargo audit`
  en CI.

### Mitigacion

- La MSRV alta se documenta explicitamente en
  `README.md` y en el mensaje de error si la
  compilacion falla.
- Si glyphon se discontinua, la alternativa es
  cosmy-text directo con un shim propio (~200
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
- [https://crates.io/crates/anyhow](https://crates.io/crates/anyhow)
- [https://crates.io/crates/thiserror](https://crates.io/crates/thiserror)
- [https://crates.io/crates/tracing](https://crates.io/crates/tracing)
- [https://crates.io/crates/criterion](https://crates.io/crates/criterion)

## Cambios

| Version | Fecha      | Cambios                                                               |
| ------- | ---------- | --------------------------------------------------------------------- |
| 0.1.0   | 2026-06-14 | Primer borrador. Decision adoptada. 14 deps + 1 dev-dep, MSRV 1.87.0. |
