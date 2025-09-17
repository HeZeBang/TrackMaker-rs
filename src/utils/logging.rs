use tracing_subscriber::{EnvFilter, fmt};

pub fn init_logging() {
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(crate::utils::consts::LOG_LEVEL))
        .unwrap();

    fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .with_level(true)
        .compact()
        .with_writer(std::io::stdout)
        .init();
}
