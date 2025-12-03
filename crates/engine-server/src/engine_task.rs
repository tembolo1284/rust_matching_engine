//! Central engine task that processes all trading requests.

use std::sync::Arc;

use engine_core::{InputMessage, MatchingEngine, OutputMessage};
use tokio::sync::mpsc;

use crate::metrics::Metrics;
use crate::router;
use crate::types::{ClientRegistry, EngineRx, MulticastTx};

/// Run the central matching engine loop.
pub async fn run_engine_loop(
    mut engine_rx: EngineRx,
    clients: Arc<ClientRegistry>,
    multicast_tx: Option<MulticastTx>,
    metrics: Arc<Metrics>,
) {
    // Create engine with pre-allocation
    let mut engine = MatchingEngine::new();
    
    // Pre-allocate output buffer (reused across all messages)
    let mut outputs: Vec<OutputMessage> = Vec::with_capacity(64);

    eprintln!("Engine task: started");

    while let Some(request) = engine_rx.recv().await {
        Metrics::inc(&metrics.messages_received);

        // Track user → client mapping for response routing
        clients.set_user_id(request.client_id, request.user_id).await;

        // Update specific metrics based on message type
        match &request.msg {
            InputMessage::NewOrder(_) => Metrics::inc(&metrics.orders_received),
            InputMessage::Cancel(_) => Metrics::inc(&metrics.cancels_received),
            _ => {}
        }

        // Process in engine (reuse output buffer)
        outputs.clear();
        engine.process_message(request.msg, &mut outputs);

        Metrics::inc(&metrics.messages_processed);

        // Route each output message
        for msg in &outputs {
            // Update trade count
            if matches!(msg, OutputMessage::Trade(_)) {
                Metrics::inc(&metrics.trades_executed);
            }

            // Route to appropriate recipients
            let (unicast_targets, should_multicast) = 
                router::route_to_originator(msg, request.client_id);

            // Send unicast messages
            for (target_id, target_msg) in unicast_targets {
                if clients.send_to_client(target_id, target_msg).await {
                    Metrics::inc(&metrics.messages_sent);
                } else {
                    Metrics::inc(&metrics.send_errors);
                }
            }

            // Send to multicast if applicable
            if should_multicast {
                if let Some(ref mcast_tx) = multicast_tx {
                    match mcast_tx.try_send(msg.clone()) {
                        Ok(_) => Metrics::inc(&metrics.multicast_messages),
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            Metrics::inc(&metrics.channel_full_drops);
                        }
                        Err(_) => {}
                    }
                }
            }
        }
    }

    eprintln!("Engine task: shutting down");
}
