use tracing_subscriber::EnvFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("baud starting");

    // ponytail: el event loop es bloqueante. No hace falta join explicito.
    baud::event_loop::run()?;

    Ok(())
}
