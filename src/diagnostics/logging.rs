//! Inicialización de `tracing`: capa de archivo rotatorio siempre activa,
//! capa de stdout solo cuando hay una consola real adjunta, y un filtro
//! recargable para que `diagnostics.log_level` se aplique tras cargar la
//! config y en cada hot-reload, sin reiniciar el proceso.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::OnceLock;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{reload, EnvFilter, Layer};

/// Filtro usado cuando no hay `RUST_LOG` en el entorno ni nivel en la config.
const DEFAULT_FILTER: &str = "baud=warn,wgpu_core=warn,winit=warn";

static FILTER_HANDLE: OnceLock<reload::Handle<EnvFilter, tracing_subscriber::Registry>> =
    OnceLock::new();

/// Mantiene vivos los recursos de logging (el worker no bloqueante de
/// escritura a archivo). Debe mantenerse en el scope de `main()` hasta el
/// final del proceso o se pierden los últimos registros al salir.
pub struct LoggingGuard {
    _file_guard: tracing_appender::non_blocking::WorkerGuard,
}

/// Inicializa el registry de `tracing` con capa de archivo (siempre) y capa
/// de stdout (solo si `has_console` es `true`).
///
/// `has_console` debe ser `true` en Unix (siempre hay consola) y en Windows
/// solo cuando [`crate::console::attach_parent_console`] tuvo éxito.
pub fn init(has_console: bool) -> LoggingGuard {
    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("baud.log")
        .max_log_files(3)
        .build(log_dir())
        .expect("no se pudo inicializar el log de archivo");
    let (non_blocking, file_guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false);

    let stdout_layer = has_console
        .then(|| tracing_subscriber::fmt::layer().with_ansi(std::io::stdout().is_terminal()));

    let (reload_filter, handle) = reload::Layer::new(build_filter(None));
    let _ = FILTER_HANDLE.set(handle);

    tracing_subscriber::registry()
        .with(file_layer.and_then(stdout_layer).with_filter(reload_filter))
        .with(crate::diagnostics::hooks::ReporterLayer)
        .init();

    LoggingGuard {
        _file_guard: file_guard,
    }
}

/// Aplica `diagnostics.log_level` de la config al filtro activo. `RUST_LOG`
/// en el entorno siempre tiene prioridad y deja este llamado sin efecto.
pub fn apply_log_level(config_level: Option<&str>) {
    if std::env::var_os("RUST_LOG").is_some() {
        return;
    }
    let Some(handle) = FILTER_HANDLE.get() else {
        return;
    };
    if let Err(e) = handle.reload(build_filter(config_level)) {
        tracing::warn!("no se pudo recargar el nivel de log: {e}");
    }
}

fn build_filter(config_level: Option<&str>) -> EnvFilter {
    if let Ok(from_env) = EnvFilter::try_from_default_env() {
        return from_env;
    }
    if let Some(level) = config_level {
        if let Ok(filter) = EnvFilter::try_new(format!("baud={level},wgpu_core=warn,winit=warn")) {
            return filter;
        }
    }
    EnvFilter::new(DEFAULT_FILTER)
}

/// Directorio de logs: `state_dir()` (Linux: `~/.local/state/baud/logs/`),
/// con fallback a `data_local_dir()` (Windows: `%LOCALAPPDATA%\baud\logs\`).
/// Deliberadamente *local*, no roaming: `dirs::data_dir()` en Windows apunta
/// a `%APPDATA%`, que se sincroniza entre equipos de un dominio.
fn log_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("baud")
        .join("logs")
}
