use tokio::sync::mpsc::UnboundedReceiver;

use engine_core::{MatchingEngine, OutputMessage};

use crate::types::{ClientRegistry, EngineRequest};

/// Run the central engine loop.
///
/// - Receives `EngineRequest`s from all clients.
/// - Feeds them into `MatchingEngine`.
/// - Broadcasts resulting `OutputMessage`s to all connected clients.
/// - Tracks simple statistics and logs them when shutting down.
pub async fn run_engine_loop(
    mut engine_rx: UnboundedReceiver<EngineRequest>,
    clients: ClientRegistry,
) {
    let mut engine = MatchingEngine::new();

    // Simple stats — dev-focused, like your C++ OutputPublisher counters.
    let mut requests_received: u64 = 0;
    let mut outputs_generated: u64 = 0;

    eprintln!("Engine task: started");

    while let Some(EngineRequest { client_id: _client_id, msg }) = engine_rx.recv().await {
        requests_received += 1;

        // Process message in the matching engine.
        let outputs: Vec<OutputMessage> = engine.process_message(msg);
        outputs_generated += outputs.len() as u64;

        // For now: broadcast every output to every client.
        //
        // If you ever want selective routing later, we'd change this
        // to use some subscription or per-client filter.
        let guard = clients.read().await;
        for (target_id, tx) in guard.iter() {
            for out in &outputs {
                // If a send fails, that client is probably gone — we just log.
                if tx.send(out.clone()).is_err() {
                    eprintln!(
                        "Engine: failed to send message to client {}; \
                         dropping from registry may be needed",
                        target_id.0
                    );
                }
            }
        }

        // Optional: very light debug trace (commented out for perf)
        // eprintln!(
        //     "Engine: processed request from client {}, fanout {} outputs",
        //     client_id.0,
        //     outputs.len()
        // );
    }

    // Channel closed (server is shutting down).
    eprintln!("==============================================================");
    eprintln!("Engine task shutting down.");
    eprintln!("  Requests received:  {}", requests_received);
    eprintln!("  Outputs generated:  {}", outputs_generated);
    eprintln!("==============================================================");
}

