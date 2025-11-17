//! TCP listener and top-level server wiring.
//!
//! This module:
//! - Binds a TCP listener (with simple port retry).
//! - Prints a startup banner similar in spirit to the C++ version.
//! - Accepts new TCP connections.
//! - Assigns each connection a `ClientId`.
//! - Spawns:
//!   - a per-client task to handle I/O,
//!   - a single central engine task that owns `MatchingEngine`.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::types::{
    ClientId, ClientRegistry, EngineRequest, EngineRx, EngineTx, OutboundRx, OutboundTx,
};

/// Global-ish counter for assigning unique `ClientId`s.
static NEXT_CLIENT_ID: AtomicU64 = AtomicU64::new(1);

fn next_client_id() -> ClientId {
    let id = NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed);
    ClientId(id)
}

/// Max number of times we'll try to bump the port if it's in use.
const MAX_PORT_RETRIES: u16 = 3;

/// Run the TCP server with the given configuration.
///
/// - Tries to bind `bind_addr:port`.
/// - If the port is in use, increments the port and retries,
///   up to `MAX_PORT_RETRIES` times.
/// - Prints a banner once a port is successfully bound.
pub async fn run(mut config: Config) -> Result<(), Box<dyn std::error::Error>> {
    // Try binding with simple port bump on AddrInUse.
    let (listener, final_port, attempts) = bind_with_retry(&mut config).await?;

    // Update config.port to final one actually bound.
    config.port = final_port;

    print_startup_banner(&config, attempts);

    // Shared registry of clients → outbound channels.
    let clients: ClientRegistry = Arc::new(tokio::sync::RwLock::new(Default::default()));

    // Channel from clients → engine task.
    let (engine_tx, engine_rx): (EngineTx, EngineRx) = mpsc::unbounded_channel();

    // Spawn the central engine task.
    {
        let clients_clone = clients.clone();
        tokio::spawn(async move {
            crate::engine_task::run_engine_loop(engine_rx, clients_clone).await;
        });
    }

    eprintln!(
        "TCP listener ready on {} (press Ctrl+C to shutdown gracefully)",
        config.socket_addr_string()
    );
    eprintln!("==============================================================");

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let current_clients = {
            let guard = clients.read().await;
            guard.len()
        };

        if current_clients >= config.max_clients {
            eprintln!(
                "Rejecting connection from {}: max_clients ({}) reached",
                peer_addr, config.max_clients
            );
            // Just drop the stream; client will see connection refused/closed.
            continue;
        }

        let client_id = next_client_id();
        eprintln!("Accepted connection {} from {}", client_id.0, peer_addr);

        // Create outbound channel for this client.
        let (out_tx, out_rx): (OutboundTx, OutboundRx) = mpsc::unbounded_channel();

        // Register client.
        {
            let mut guard = clients.write().await;
            guard.insert(client_id, out_tx.clone());
        }

        // Clone handles to move into the client task.
        let clients_clone = clients.clone();
        let engine_tx_clone = engine_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = crate::client::run_client(
                client_id,
                stream,
                engine_tx_clone,
                out_rx,
                clients_clone,
            )
            .await
            {
                eprintln!("Client {} error: {:?}", client_id.0, e);
            } else {
                eprintln!("Client {} disconnected", client_id.0);
            }
        });
    }
}

/// Try to bind, bumping the port by +1 on `AddrInUse`, up to `MAX_PORT_RETRIES`.
async fn bind_with_retry(
    config: &mut Config,
) -> Result<(TcpListener, u16, u16), Box<dyn std::error::Error>> {
    let mut attempts: u16 = 0;
    let mut port = config.port;

    loop {
        attempts += 1;
        let addr = format!("{}:{}", config.bind_addr, port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                return Ok((listener, port, attempts));
            }
            Err(e) if e.kind() == io::ErrorKind::AddrInUse && attempts < MAX_PORT_RETRIES => {
                eprintln!(
                    "Port {} is already in use on {} (attempt {}/{}), trying {}...",
                    port,
                    config.bind_addr,
                    attempts,
                    MAX_PORT_RETRIES,
                    port + 1
                );
                port += 1;
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                return Err(format!(
                    "Failed to bind after {} attempts; last tried {}:{}",
                    attempts, config.bind_addr, port
                )
                .into());
            }
            Err(e) => {
                return Err(format!(
                    "Failed to bind to {}:{}: {}",
                    config.bind_addr, port, e
                )
                .into());
            }
        }
    }
}

/// Print a startup banner similar in spirit to your C++ version,
/// adapted for the TCP + Tokio/mpsc architecture.
fn print_startup_banner(config: &Config, attempts: u16) {
    eprintln!("==============================================================");
    eprintln!("Order Book - TCP Matching Engine");
    eprintln!("==============================================================");
    eprintln!("Bind address: {}", config.bind_addr);
    eprintln!("TCP Port:     {}", config.port);
    eprintln!("Max clients:  {}", config.max_clients);
    if attempts > 1 {
        eprintln!(
            "Note: bound after {} attempts (port bumped due to AddrInUse).",
            attempts
        );
    }
    eprintln!("==============================================================");
    eprintln!("Queue Configuration:");
    eprintln!("  Engine request queue:   Tokio mpsc::unbounded_channel()");
    eprintln!("  Client outbound queues: Tokio mpsc::unbounded_channel() per client");
    eprintln!("==============================================================");
    eprintln!("Starting tasks...");
    eprintln!("  Engine task:        started");
    eprintln!("  TCP listener:       starting on {}:{}",
              config.bind_addr, config.port);
    eprintln!("==============================================================");
}
