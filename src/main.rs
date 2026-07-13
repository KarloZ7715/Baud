use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("baud=warn,wgpu_core=warn,winit=warn"));

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter))
        .with(baud::diagnostics::hooks::ReporterLayer)
        .init();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Los comandos CLI informativos y de actualizacion se ejecutan antes de
    // inicializar cualquier subsistema grafico o de diagnostico.
    match baud::cli::run()? {
        Some(code) => std::process::exit(code),
        None => {
            baud::diagnostics::hooks::install_panic_hook();
            init_tracing();

            tracing::info!("baud starting");

            // ponytail: el event loop es bloqueante. No hace falta join explicito.
            baud::event_loop::run()?;

            Ok(())
        }
    }
}
