//! TCP listener and top-level server wiring.
//!
//! This module:
//! - Listens on the configured address/port.
//! - Accepts new TCP connections.
//! - Assigns each connection a `ClientId`.
//! - Spawns:
//!   - a per-client task to handle I/O,
//!   - a single central engine task that owns `MatchingEngine`.
//!
//! The actual per-client logic and engine loop live in `client`
//! and `engine_task` modules respectively.

mod client;
mod engine_task;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::mpsc;

use engine_core::OutputMessage;

use crate::config::Config;
use crate::types::{
    ClientId, ClientRegistry, EngineRequest, EngineRx, EngineTx, OutboundRx, OutboundTx,
};

/// Global-ish counter for assigning unique `ClientId`s.
///
/// In a more elaborate setup you might encapsulate this in a struct,
/// but this is sufficient and threadsafe for our server.
static NEXT_CLIENT_ID: AtomicU64 = AtomicU64::new(1);

fn next_client_id() -> ClientId {
    let id = NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed);
    ClientId(id)
}

/// Run the TCP server with the given configuration.
pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let addr = config.socket_addr_string();
    let listener = TcpListener::bind(&addr).await?;
    eprintln!("Listening on {}", addr);

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
            if let Err(e) =
                client::run_client(client_id, stream, engine_tx_clone, out_rx, clients_clone).await
            {
                eprintln!("Client {} error: {:?}", client_id.0, e);
            } else {
                eprintln!("Client {} disconnected", client_id.0);
            }
        });
    }
}

