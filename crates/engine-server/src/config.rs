//! Configuration for the engine TCP server.
//!
//! For now this is intentionally simple: you can either use defaults
//! or override via a few environment variables:
//!
//! - `ENGINE_BIND_ADDR`   (default: "0.0.0.0")
//! - `ENGINE_PORT`        (default: "9000")
//! - `ENGINE_MAX_CLIENTS` (default: "1024")

use std::env;
use std::str::FromStr;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// IP address / interface to bind to (e.g. "0.0.0.0" or "127.0.0.1").
    pub bind_addr: String,

    /// TCP port to listen on.
    pub port: u16,

    /// Maximum number of simultaneously connected clients.
    pub max_clients: usize,
}

impl Config {
    /// Construct a `Config` from environment variables, falling back
    /// to reasonable defaults.
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let bind_addr = env::var("ENGINE_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = read_env_or_default("ENGINE_PORT", 9000u16)?;
        let max_clients = read_env_or_default("ENGINE_MAX_CLIENTS", 1024usize)?;

        Ok(Config {
            bind_addr,
            port,
            max_clients,
        })
    }

    /// Convenience: `addr:port` socket string.
    pub fn socket_addr_string(&self) -> String {
        format!("{}:{}", self.bind_addr, self.port)
    }
}

fn read_env_or_default<T>(key: &str, default: T) -> Result<T, Box<dyn std::error::Error>>
where
    T: FromStr,
    T::Err: std::error::Error + 'static,
{
    match env::var(key) {
        Ok(val) => Ok(val.parse::<T>()?),
        Err(_) => Ok(default),
    }
}

