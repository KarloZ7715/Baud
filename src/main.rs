use tracing_subscriber::EnvFilter;

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("baud=info,baud::watchdog=warn,wgpu_core=warn,winit=warn")
    });
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    tracing::info!("baud starting");

    // ponytail: el event loop es bloqueante. No hace falta join explicito.
    baud::event_loop::run()?;

    Ok(())
}
