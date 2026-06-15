```yaml
titulo: "Error Handling , Detalle Operativo"
tipo: especificacion
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-14"
fecha_modificacion: "2026-06-14"
version: "0.1.0"
estado: borrador
tags: [errores, robustez, panic, recovery, anyhow, thiserror, operativo]
```

# Error Handling , Detalle Operativo

## 1. Resumen

Este documento complementa
`docs/decisions/ADR-0007-error-handling.md` con el
detalle operativo de la estrategia de manejo de
errores: jerarquia de tipos `Error`, pseudocodigo de
los puntos de error mas criticos, configuracion de
tracing, escenarios de recovery, y secuencia de
shutdown.

La decision de alto nivel (anyhow + thiserror, sin
catch_unwind, tracing para logging) vive en el
ADR-0007. Este doc se enfoca en la implementación.

## 2. Principios

Cuatro principios guian la implementación:

1. **Fail fast en domain.** Si el parser recibe una
   secuencia invalida, retorna `ParserError` y el
   caller decide. No se traga el error.
2. **Fail safe en bordes.** Si el modulo de display
   no puede inicializar wgpu, se sale con mensaje
   claro al usuario. No se reintenta automaticamente.
3. **Contexto util.** Cada error en un `Result` que
   cruza capas debe tener `.context("...")` con
   información accionable.
4. **Panic solo en casos irrecuperables.** Un panic
   indica un bug del programa, no un fallo del
   entorno. Los panics se loguean con backtrace y se
   abortan.

## 3. Jerarquia de Tipos de Error

### 3.1 Tipos en Domain (thiserror)

**`src/pty/error.rs`:**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PtyError {
    #[error("no se pudo abrir el PTY: {0}")]
    Open(#[source] nix::Error),

    #[error("no se pudo hacer fork: {0}")]
    Fork(#[source] nix::Error),

    #[error("no se pudo ejecutar el shell: {0}")]
    Exec(#[source] std::io::Error),

    #[error("error de I/O en el PTY: {0}")]
    Io(#[source] std::io::Error),

    #[error("el PTY ya esta cerrado")]
    Closed,
}

impl From<nix::Error> for PtyError {
    fn from(e: nix::Error) -> Self {
        PtyError::Open(e)
    }
}
```

**`src/parser/error.rs`:**

```rust
#[derive(Debug, Error)]
pub enum ParserError {
    #[error("secuencia CSI invalida: {0}")]
    InvalidCsi(String),

    #[error("secuencia OSC sin ST: {0}")]
    UnterminatedOsc(String),

    #[error("parametro fuera de rango: {0}")]
    OutOfRange(i64),
}
```

**`src/grid/error.rs`:**

```rust
#[derive(Debug, Error)]
pub enum GridError {
    #[error("dimension invalida: {0}x{1}")]
    InvalidDimension(usize, usize),

    #[error("indice fuera de rango: linea {0}, col {1}")]
    OutOfBounds(i32, usize),

    #[error("resize no soportado en alt screen con reflow")]
    ReflowNotSupported,
}
```

### 3.2 Tipo de Borde (anyhow)

`src/error.rs` (modulo raiz):

```rust
pub use anyhow::{Context, Result};

pub type AppResult<T> = anyhow::Result<T>;
```

### 3.3 Conversion Automatica

`#[from]` en las variantes permite conversion
automatica con `?`:

```rust
fn read_pty(pty: &mut MasterPty) -> AppResult<Vec<u8>> {
    let mut buf = [0u8; 4096];
    let n = pty.read(&mut buf)
        .context("leyendo del PTY master")?;  // PtyError -> anyhow::Error
    Ok(buf[..n].to_vec())
}
```

## 4. Estrategia por Capa

### 4.1 main.rs (Borde)

```rust
use baud::error::AppResult;

fn main() -> AppResult<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into())
        )
        .init();

    let config = Config::load().context("cargando configuracion")?;
    let mut app = App::new(config).context("inicializando aplicacion")?;
    app.run().context("ejecutando event loop")?;
    Ok(())
}
```

### 4.2 Modulos de Domain

```rust
// src/grid/mod.rs
use crate::pty::error::GridError;

impl Grid {
    pub fn resize(&mut self, new_cols: usize, new_rows: usize)
        -> Result<(), GridError>
    {
        if new_cols == 0 || new_rows == 0 {
            return Err(GridError::InvalidDimension(new_cols, new_rows));
        }
        // ... logica de resize ...
        Ok(())
    }
}
```

### 4.3 Handlers de UI (Borde)

```rust
// src/input/mod.rs
fn handle_key(&self, event: KeyEvent) -> AppResult<()> {
    let bytes = match event.key_code {
        KeyCode::Char(c) => vec![c as u8],
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7F],
        // ...
    };
    self.pty_tx.send(Msg::Input(bytes))
        .context("enviando input al hilo PTY")?;
    Ok(())
}
```

## 5. Puntos de Error mas Criticos

Pseudocodigo de los 5 puntos donde el proyecto puede
fallar. Cada uno se valida con unit test o
integration test.

### 5.1 main() Arranque

```rust
fn main() -> AppResult<()> {
    // 1. Cargar configuracion (archivo invalido -> anyhow)
    let config = Config::load()
        .context("cargando configuracion desde disco")?;

    // 2. Inicializar tracing
    tracing_subscriber::fmt()
        .with_env_filter(env_filter_from(&config))
        .init();

    // 3. Verificar display server
    if !display_available() {
        return Err(anyhow::anyhow!(
            "no se detecta un display server (X11/Wayland)"
        ));
    }

    // 4. Spawn del child
    let pty = Pty::spawn(&config.shell, &config.shell_args)
        .context("arrancando el shell del usuario")?;

    // 5. Crear ventana
    let event_loop = EventLoop::new()
        .context("creando event loop de winit")?;
    let window = WindowBuilder::new()
        .with_title(&config.window.title)
        .build(&event_loop)
        .context("creando ventana")?;

    // 6. Inicializar wgpu
    let wgpu = WgpuContext::new(&window)
        .await
        .context("inicializando contexto wgpu")?;

    Ok(())
}
```

Errores tipicos en arranque y mensaje al usuario:

| Error                 | Mensaje al usuario                                                                           |
| :-------------------- | :------------------------------------------------------------------------------------------- |
| Config invalida       | "Error cargando configuracion en ~/.config/baud/config.toml: <detalle>"                      |
| Sin display           | "Error: no se detecta un display server. Asegurate de estar en una sesión grafica."          |
| Spawn del shell falla | "Error arrancando el shell '<shell>': <detalle>"                                             |
| winit falla           | "Error creando ventana: <detalle>"                                                           |
| wgpu falla            | "Error inicializando render GPU: <detalle>. Tu hardware puede no ser compatible con WebGPU." |

### 5.2 PTY Spawn

```rust
impl Pty {
    pub fn spawn(shell: &str, args: &[&str]) -> Result<Self, PtyError> {
        // 1. Abrir PTY
        let pty = nix::pty::openpty()
            .map_err(PtyError::Open)?;

        // 2. Configurar tamaño
        let winsize = Winsize {
            ws_row: 24, ws_col: 80,
            ws_xpixel: 0, ws_ypixel: 0,
        };
        nix::libc::ioctl(pty.slave, TIOCSWINSZ, &winsize)
            .map_err(PtyError::Io)?;

        // 3. Fork
        match unsafe { nix::unistd::fork() } {
            Ok(ForkResult::Parent { child }) => {
                // Cerrar slave en el parent
                drop(pty.slave);
                Ok(Self {
                    master: pty.master,
                    child,
                })
            }
            Ok(ForkResult::Child) => {
                // 4. Child: setup completo antes de exec
                Self::child_setup(pty.slave, shell, args)
                    .expect("child setup must succeed");
                unreachable!()
            }
            Err(e) => Err(PtyError::Fork(e)),
        }
    }
}
```

### 5.3 Render Init (wgpu)

```rust
impl WgpuContext {
    pub async fn new(window: &Window) -> Result<Self, WgpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = unsafe {
            instance.create_surface(window)
                .map_err(WgpuError::SurfaceCreation)?
        };

        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }).await.ok_or(WgpuError::NoAdapter)?;

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("baud"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ).await.map_err(WgpuError::DeviceRequest)?;

        Ok(Self { instance, surface, adapter, device, queue })
    }
}
```

### 5.4 Grid Resize

```rust
impl Grid {
    pub fn resize(&mut self, new_cols: usize, new_rows: usize)
        -> Result<(), GridError>
    {
        if new_cols == 0 || new_rows == 0 {
            tracing::error!(
                "resize a ({}, {}) rechazado: dimensiones invalidas",
                new_cols, new_rows
            );
            return Err(GridError::InvalidDimension(new_cols, new_rows));
        }

        let old_cols = self.cols;
        let old_rows = self.rows;

        if old_cols == new_cols && old_rows == new_rows {
            return Ok(());
        }

        if self.in_alt_screen {
            // Alt screen: truncate or extend without reflow
            self.resize_no_reflow(new_cols, new_rows)?;
        } else {
            // Primary screen: reflow
            self.resize_with_reflow(new_cols, new_rows)?;
        }

        self.cols = new_cols;
        self.rows = new_rows;
        Ok(())
    }
}
```

### 5.5 Child Muerte (Recovery)

```rust
fn handle_child_exit(&mut self, exit_code: i32) {
    tracing::warn!("child proceso terminado con codigo {}", exit_code);

    if self.config.show_exit_message {
        // Escribir en el grid: "[Proceso terminado: codigo N]"
        self.write_to_grid(
            &format!("\n[Proceso terminado: codigo {}]\n", exit_code)
        );
    }

    // NO salir. Permitir al usuario escribir comandos nuevos
    // (el shell esta muerto, pero podemos relanzarlo o esperar)

    // Opcion A: relanzar el shell
    if self.config.restart_shell_on_exit {
        match Pty::spawn(&self.config.shell, &self.config.shell_args) {
            Ok(new_pty) => {
                self.pty = new_pty;
                tracing::info!("shell relanzado");
            }
            Err(e) => {
                tracing::error!("no se pudo relanzar el shell: {}", e);
            }
        }
    }

    // Opcion B: esperar input del usuario (no hacer nada)
}
```

## 6. Panic Handling

### 6.1 MVP: Default de Rust

En MVP, no se configura panic hook custom. El default
de Rust:

1. Imprime el mensaje del panic.
2. Imprime el backtrace (si `RUST_BACKTRACE=1`).
3. Aborta con código 101.

Esto es suficiente para desarrollo. Los crashes
quedan visibles en el log de CI.

### 6.2 Fase 5: Panic Hook Custom

Inspirado en WezTerm, en Fase 5 se agrega un panic
hook que:

1. Captura el mensaje y backtrace.
2. Muestra una notificacion GTK nativa (o
   equivalente) con el mensaje.
3. Ofrece copiar al clipboard.
4. Permite al usuario reportar el bug.

```rust
// src/panic_hook.rs (Fase 5)
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let message = format!("{:?}\n\n{}", panic_info, backtrace);

        // Mostrar notificacion nativa
        if let Err(e) = show_native_notification(
            "baud ha encontrado un error",
            &message,
        ) {
            eprintln!("{}", message);
            eprintln!("Error mostrando notificacion: {}", e);
        }

        // Loggear
        tracing::error!(target: "panic", "{}", message);
    }));
}
```

## 7. Logging con tracing

### 7.1 Configuracion

`src/logging.rs`:

```rust
pub fn init() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new("info,baud=debug")
        });

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(false)  // No mostrar archivo en produccion
        .with_line_number(false)
        .init();
}
```

### 7.2 Niveles por Modulo

| Modulo       | Nivel default | Notas                                  |
| :----------- | :------------ | :------------------------------------- |
| `main`       | info          | Arranque, configuracion, primer render |
| `event_loop` | debug         | Flujo PTY I/O                          |
| `pty`        | debug         | Spawn, read, write                     |
| `parser`     | trace         | Cada byte del PTY (solo en dev)        |
| `grid`       | warn          | Errores de resize, panic recovery      |
| `renderer`   | info          | Inicializacion, error                  |
| `input`      | debug         | Clasificacion de teclas                |
| `panic`      | error         | Solo panics                            |

Para activar nivel `trace` solo en el parser:

```bash
RUST_LOG=info,baud::parser=trace cargo run
```

### 7.3 Spans

Usar spans para estructurar el contexto de multihilo:

```rust
use tracing::{info, instrument};

#[instrument(skip(self))]
fn handle_resize(&mut self, new_cols: usize, new_rows: usize) -> AppResult<()> {
    info!("resize: {}x{} -> {}x{}", self.cols, self.rows, new_cols, new_rows);
    self.grid.resize(new_cols, new_rows)?;
    self.pty.resize(new_cols, new_rows)?;
    Ok(())
}
```

## 8. Escenarios de Recovery

| Escenario           | Deteccion                    | Estrategia                                | Test      |
| :------------------ | :--------------------------- | :---------------------------------------- | :-------- |
| Child muere         | SIGCHLD vía signal pipe      | Mostrar mensaje, esperar input (no salir) | IT-005    |
| PTY se cierra (EOF) | `read()` retorna 0           | Mostrar mensaje, intentar reabrir         | IT-006    |
| wgpu device lost    | `wgpu::DeviceLost` callback  | Re-inicializar wgpu, mantener estado      | manual    |
| Font no carga       | error al cargar con glyphon  | Usar font de sistema por defecto          | unit test |
| Config invalida     | serde::de::Error al parsear  | Mostrar error, usar defaults parciales    | unit test |
| Sin display         | winit falla al crear ventana | Salir con mensaje claro                   | manual    |
| Out of memory       | allocation retorna None      | Salir con mensaje, sin recovery           | N/A       |
| Child se cuelga     | timeout en `read()`          | Desconocido (MVP); Fase 5: enviar SIGHUP  | Fase 5    |

## 9. Shutdown Graceful

### 9.1 Triggers

Shutdown se activa por:

1. `WindowEvent::CloseRequested` de winit.
2. `SIGTERM` (signal_hook).
3. `SIGINT` (Ctrl+C, si la config lo permite).
4. EOF del child (Ctrl+D en shell vacio).

### 9.2 Secuencia

```rust
fn shutdown(&mut self) -> AppResult<()> {
    tracing::info!("iniciando shutdown graceful");

    // 1. Enviar SIGHUP al child
    if let Err(e) = self.pty.send_sighup() {
        tracing::warn!("no se pudo enviar SIGHUP al child: {}", e);
    }

    // 2. Esperar hasta 100ms para que el child termine
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(100) {
        match self.pty.child().try_wait() {
            Ok(Some(status)) => {
                tracing::info!("child terminado con {:?}", status);
                break;
            }
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }

    // 3. Si el child sigue vivo, SIGKILL
    if let Ok(None) = self.pty.child().try_wait() {
        tracing::warn!("child no respondio a SIGHUP, enviando SIGKILL");
        if let Err(e) = self.pty.child().kill() {
            tracing::error!("no se pudo enviar SIGKILL: {}", e);
        }
    }

    // 4. Drop del struct PTY (que envia SIGHUP en su Drop impl)
    drop(self.pty.take());

    // 5. Flush de logs
    tracing::info!("shutdown completo");

    Ok(())
}
```

### 9.3 Drop del Struct PTY

```rust
impl Drop for Pty {
    fn drop(&mut self) {
        // SIGHUP en Drop es el patron usado por Alacritty
        // y WezTerm. Garantiza que el child no quede huerfano.
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(self.child.id() as i32),
            nix::sys::signal::Signal::SIGHUP,
        );
    }
}
```

## 10. Limitaciones

1. **Sin panic hook en MVP.** Los panics se ven en
   stderr/abort. Se mejora en Fase 5.
2. **Sin fallback a software rendering.** Si wgpu
   falla, el emulador no arranca.
3. **Sin recovery automatico de OpenGL.** Si el
   contexto GPU se pierde, hay que reiniciar el
   emulador.
4. **Timeout de shutdown hardcodeado a 100ms.** No
   configurable en MVP.
5. **No hay deteccion de child colgado.** Si el
   child no responde a SIGHUP, se envia SIGKILL
   sin esperar.

## 11. Referencias

- docs/decisions/ADR-0007-error-handling.md (decision
  de alto nivel).
- docs/prompts/iter-06-investigacion-E.md
  (investigacion base, 526 lineas).
- https://docs.rs/anyhow/latest/anyhow/
- https://docs.rs/thiserror/latest/thiserror/
- https://docs.rs/tracing/latest/tracing/
- https://docs.rs/tracing-subscriber/latest/tracing_subscriber/
- Alacritty main: alacritty/src/main.rs.
- WezTerm panic hook: busqueda de `set_hook` en
  wezterm-gui/.

## Cambios

| Version | Fecha      | Cambios                                                                              |
| :------ | :--------- | :----------------------------------------------------------------------------------- |
| 0.1.0   | 2026-06-14 | Primer borrador. Detalle operativo de anyhow+thiserror, tracing, recovery, shutdown. |
