//! Configuration for the engine TCP server.
//!
//! You can configure the server either via environment variables
//! or a simple CLI flag:
//!
//! Environment variables (all optional):
//! - `ENGINE_BIND_ADDR`   (default: "0.0.0.0")
//! - `ENGINE_PORT`        (default: "9000")
//! - `ENGINE_MAX_CLIENTS` (default: "1024")
//!
//! CLI override (takes precedence over env):
//! - `--addr HOST:PORT`
//!
//! Examples:
//!   cargo run -p engine-server
//!   cargo run -p engine-server -- --addr 127.0.0.1:7001

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

    /// Construct a `Config` from env + CLI args.
    ///
    /// CLI overrides env where provided. Currently supports:
    ///   --addr HOST:PORT
    pub fn from_env_and_args() -> Result<Self, Box<dyn std::error::Error>> {
        let mut cfg = Self::from_env()?;

        let mut args = env::args().skip(1); // skip program name
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--addr" => {
                    let val = args.next().ok_or_else(|| {
                        "Missing value for --addr (expected HOST:PORT)".to_string()
                    })?;

                    let parts: Vec<_> = val.split(':').collect();
                    if parts.len() != 2 {
                        return Err(format!("Invalid --addr '{}', expected HOST:PORT", val).into());
                    }

                    cfg.bind_addr = parts[0].to_string();
                    cfg.port = parts[1]
                        .parse::<u16>()
                        .map_err(|e| format!("Invalid port in --addr '{}': {}", val, e))?;
                }
                // Ignore unknown args for now (lets you extend later).
                _ => {}
            }
        }

        Ok(cfg)
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
