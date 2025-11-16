//! Central engine loop.
//!
//! This task owns the `MatchingEngine` instance and processes
//! all `EngineRequest`s coming from clients.
//!
//! Routing policy (can be refined later):
//! - `Ack`, `CancelAck`: sent **only** to the originating client.
//! - `Trade`, `TopOfBook`: broadcast to **all** connected clients.
//!
//! This keeps the engine logic decoupled from who is listening;
//! symbol-based subscriptions can be added here later if needed.

use std::sync::Arc;

use engine_core::{MatchingEngine, OutputMessage};
use tokio::sync::RwLock;

use crate::types::{ClientId, ClientRegistry, EngineRequest, EngineRx};

/// Run the central engine processing loop.
///
/// - `engine_rx`: receives requests from all client tasks.
/// - `clients`: registry of connected clients and their outbound channels.
pub async fn run_engine_loop(mut engine_rx: EngineRx, clients: ClientRegistry) {
    let mut engine = MatchingEngine::new();

    while let Some(req) = engine_rx.recv().await {
        let EngineRequest { client_id, msg } = req;

        let outputs = engine.process_message(msg);

        if outputs.is_empty() {
            continue;
        }

        // Snapshot of current clients to minimize lock hold time.
        let current_clients = {
            let guard = clients.read().await;
            guard.clone()
        };

        for out in outputs {
            route_output(client_id, &out, &current_clients);
        }
    }

    eprintln!("Engine loop shutting down (engine_rx closed)");
}

/// Route a single `OutputMessage` to appropriate client(s).
///
/// Current policy:
/// - `Ack`, `CancelAck`   => unicast to `origin_client`.
/// - `Trade`, `TopOfBook` => broadcast to all clients.
fn route_output(origin_client: ClientId, msg: &OutputMessage, clients: &std::collections::HashMap<ClientId, crate::types::OutboundTx>) {
    match msg {
        OutputMessage::Ack(_) | OutputMessage::CancelAck(_) => {
            if let Some(tx) = clients.get(&origin_client) {
                let _ = tx.send(msg.clone());
            }
        }
        OutputMessage::Trade(_) | OutputMessage::TopOfBook(_) => {
            for (_cid, tx) in clients.iter() {
                let _ = tx.send(msg.clone());
            }
        }
    }
}

