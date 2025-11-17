//! Binary TCP server for the matching engine.

use engine_server::config::Config;
use engine_server::server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read config from env + CLI. CLI (e.g. --addr 127.0.0.1:7001) wins.
    let config = Config::from_env_and_args()?;

    // All the pretty banner / retry logic lives inside server::run.
    server::run(config).await
}
