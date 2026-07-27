#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Con el subsistema GUI no hay consola por defecto en Windows: sin esto,
    // `--version`/`--help` no imprimirian nada visible al lanzar desde cmd o
    // PowerShell.
    #[cfg(windows)]
    let attached_console = baud::console::attach_parent_console();
    #[cfg(not(windows))]
    let attached_console = true;

    // Los comandos CLI informativos y de actualizacion se ejecutan antes de
    // inicializar cualquier subsistema grafico o de diagnostico.
    match baud::cli::run()? {
        baud::cli::CliOutcome::Exit(code) => {
            // Sin esto la salida queda pegada al prompt de la consola adjunta.
            #[cfg(windows)]
            if attached_console {
                println!();
            }
            std::process::exit(code)
        }
        baud::cli::CliOutcome::LaunchGui(opts) => {
            baud::diagnostics::hooks::install_panic_hook();
            let _log_guard = baud::diagnostics::logging::init(attached_console);

            tracing::info!("baud starting");

            // ponytail: el event loop es bloqueante. No hace falta join explicito.
            baud::event_loop::run(opts)?;

            Ok(())
        }
    }
}
