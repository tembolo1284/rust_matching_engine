// crates/engine-server/src/engine_task.rs

//! Central engine task.
//!
//! Listens for `EngineRequest`s from clients, passes each
//! `InputMessage` into the `MatchingEngine`, then **broadcasts**
//! every `OutputMessage` to all connected clients.

use engine_core::{MatchingEngine, OutputMessage};

use crate::types::{ClientRegistry, EngineRx};

/// Main engine loop.
///
/// - Owns a single `MatchingEngine`.
/// - Receives `EngineRequest { client_id, msg }` from all clients.
/// - For each message:
///     - Runs the engine.
///     - Broadcasts all resulting `OutputMessage`s to **all** clients.
pub async fn run_engine_loop(mut engine_rx: EngineRx, clients: ClientRegistry) {
    let mut engine = MatchingEngine::new();

    while let Some(req) = engine_rx.recv().await {
        let outputs = engine.process_message(req.msg);

        if outputs.is_empty() {
            continue;
        }

        // Snapshot current client senders under a read-lock.
        let client_senders = {
            use std::collections::HashMap;
            use tokio::sync::RwLockReadGuard;

            let guard: RwLockReadGuard<HashMap<_, _>> = clients.read().await;
            guard.values().cloned().collect::<Vec<_>>()
        };

        // Broadcast each output to all clients.
        for msg in outputs {
            for tx in &client_senders {
                // Ignore send errors (client may have disconnected).
                let _ = tx.send(msg.clone());
            }
        }
    }
}

