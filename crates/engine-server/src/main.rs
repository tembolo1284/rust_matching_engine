//! Binary TCP server for the matching engine.

use engine_server::config::Config;
use engine_server::server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;

    eprintln!(
        "Starting engine-server on {}:{} (max_clients = {})",
        config.bind_addr,
        config.port,
        config.max_clients
    );

    server::run(config).await
}

