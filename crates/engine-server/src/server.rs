//! TCP listener and top-level server wiring.
//!
//! This module:
//! - Binds to a TCP address/port (with port bumping on AddrInUse).
//! - Accepts new connections and assigns `ClientId`s.
//! - Spawns:
//!     - a central engine task that owns `MatchingEngine`;
//!     - a per-client task for TCP I/O.
//! - Handles Ctrl+C for graceful shutdown and prints a summary.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time;

use crate::config::Config;
use crate::engine_task;
use crate::types::{
    ClientId, ClientRegistry, EngineRx, EngineTx, OutboundRx, OutboundTx,
};

/// Global-ish counter for assigning unique `ClientId`s.
static NEXT_CLIENT_ID: AtomicU64 = AtomicU64::new(1);

fn next_client_id() -> ClientId {
    ClientId(NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed))
}

/// Try to bind a TCP listener, with simple "port bumping" on AddrInUse.
///
/// Tries up to 3 ports: `port`, `port+1`, `port+2`.
async fn bind_with_port_bump(bind_addr: String, mut port: u16) -> std::io::Result<(TcpListener, String, u16, u8)> {
    let mut attempts: u8 = 0;

    loop {
        attempts += 1;
        let addr_string = format!("{}:{}", bind_addr, port);
        match TcpListener::bind(&addr_string).await {
            Ok(listener) => {
                return Ok((listener, bind_addr, port, attempts));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && attempts < 3 => {
                eprintln!(
                    "Port {} is already in use on {} (attempt {}/3), trying {}...",
                    port,
                    bind_addr,
                    attempts,
                    port + 1
                );
                port = port + 1;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Run the TCP server with the given configuration.
pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    // Try to bind listener with port bumping.
    let (listener, bind_addr, bound_port, attempts) =
        bind_with_port_bump(config.bind_addr.clone(), config.port).await?;

    // Shared registry of clients → outbound channels.
    let clients: ClientRegistry = Arc::new(tokio::sync::RwLock::new(Default::default()));

    // Channel from clients → engine task.
    let (engine_tx, engine_rx): (EngineTx, EngineRx) = mpsc::unbounded_channel();

    // Spawn the central engine task.
    {
        let clients_clone = clients.clone();
        tokio::spawn(async move {
            engine_task::run_engine_loop(engine_rx, clients_clone).await;
        });
    }

    // Pretty banner (Rust version of your C++ startup logs).
    eprintln!("==============================================================");
    eprintln!("Order Book - TCP Matching Engine");
    eprintln!("==============================================================");
    eprintln!("Bind address: {}", bind_addr);
    eprintln!("TCP Port:     {}", bound_port);
    eprintln!("Max clients:  {}", config.max_clients);
    if attempts > 1 {
        eprintln!(
            "Note: bound after {} attempts (port bumped due to AddrInUse).",
            attempts
        );
    }
    eprintln!("==============================================================");
    eprintln!("Queue Configuration:");
    eprintln!("  Engine request queue:  Tokio mpsc::unbounded_channel()");
    eprintln!("  Client outbound queues: Tokio mpsc::unbounded_channel() per client");
    eprintln!("==============================================================");
    eprintln!("Starting tasks...");
    eprintln!("  Engine task: started");
    eprintln!(
        "  TCP listener: starting on {}:{}",
        bind_addr, bound_port
    );
    eprintln!("==============================================================");
    eprintln!(
        "TCP listener ready on {}:{} (press Ctrl+C to shutdown gracefully)",
        bind_addr, bound_port
    );
    eprintln!("==============================================================");

    // Main accept loop + Ctrl+C handling.
    let listener = Arc::new(listener);

    loop {
        tokio::select! {
            // Accept new clients.
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, peer_addr)) => {
                        let current_clients = {
                            let guard = clients.read().await;
                            guard.len()
                        };

                        if current_clients >= config.max_clients {
                            eprintln!(
                                "Rejecting connection from {}: max_clients ({}) reached",
                                peer_addr, config.max_clients
                            );
                            continue;
                        }

                        let client_id = next_client_id();
                        eprintln!("Accepted connection {} from {}", client_id.0, peer_addr);

                        // Outbound channel for this client.
                        let (out_tx, out_rx): (OutboundTx, OutboundRx) = mpsc::unbounded_channel();

                        // Register client.
                        {
                            let mut guard = clients.write().await;
                            guard.insert(client_id, out_tx.clone());
                        }

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
                    Err(e) => {
                        eprintln!("Listener accept error: {:?}", e);
                        // Small delay before retrying accept.
                        time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }

            // Handle Ctrl+C for graceful shutdown.
            _ = tokio::signal::ctrl_c() => {
                eprintln!();
                eprintln!("==============================================================");
                eprintln!("Ctrl+C received, initiating graceful shutdown...");
                eprintln!("==============================================================");
                break;
            }
        }
    }

    // Drop engine_tx so engine loop can finish and print stats.
    drop(engine_tx);

    // Give clients a moment to drain outbound messages and disconnect.
    {
        let mut guard = clients.write().await;
        guard.clear();
    }

    eprintln!("Waiting briefly for engine task to finish...");
    // Not strictly necessary, but a small delay helps in practice.
    time::sleep(Duration::from_millis(200)).await;

    eprintln!("==============================================================");
    eprintln!("Shutdown complete. Goodbye!");
    eprintln!("==============================================================");

    Ok(())
}

