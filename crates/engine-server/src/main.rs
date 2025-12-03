//! Multi-protocol matching engine server.

use engine_server::{Config, run};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging (optional)
    // tracing_subscriber::fmt::init();

    // Load configuration
    let config = Config::from_env_and_args()?;

    // Run server
    run(config).await
}
