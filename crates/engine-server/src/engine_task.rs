//! Central engine task that processes all trading requests.
//!
//! # Architecture
//! Single async task that:
//! 1. Receives requests from all client handlers
//! 2. Processes through matching engine
//! 3. Routes outputs to appropriate recipients
//!
//! # Power of Ten Compliance
//! - Rule 2: Bounded channel backpressure
//! - Rule 3: Pre-allocated output buffer (ArrayVec)
//! - Rule 5: Assertions on critical state
//!
//! # Performance Notes
//! - Output buffer reused across all messages
//! - try_send for multicast (non-blocking)
//! - Batch sends to reduce await overhead

use std::sync::Arc;

use arrayvec::ArrayVec;
use engine_core::{
    EngineConfig, InputMessage, MatchingEngine, OutputMessage, Symbol,
    MAX_OUTPUTS_PER_ORDER,
};
use tokio::sync::mpsc;

use crate::metrics::Metrics;
use crate::types::{ClientId, ClientRegistry, EngineRx, MulticastTx};

/// Maximum symbols to pre-register.
const DEFAULT_SYMBOLS: &[&str] = &[
    "AAPL", "AMZN", "GOOG", "META", "MSFT", "NVDA", "TSLA", "IBM",
    "JPM", "BAC", "GS", "MS", "C", "WFC", "USB", "PNC",
];

/// Run the central matching engine loop.
///
/// This is the heart of the server - all orders flow through here.
pub async fn run_engine_loop(
    mut engine_rx: EngineRx,
    clients: Arc<ClientRegistry>,
    multicast_tx: Option<MulticastTx>,
    metrics: Arc<Metrics>,
) {
    // Create engine with production config
    let config = EngineConfig {
        max_symbols: 1024,
        max_orders: 1_000_000,
        levels_per_side: 256,
        strict_mode: false, // Allow dynamic symbol creation for flexibility
    };
    let mut engine = MatchingEngine::with_config(config);

    // Pre-register common symbols
    for sym_str in DEFAULT_SYMBOLS {
        let _ = engine.register_symbol(Symbol::from_str(sym_str));
    }

    // Pre-allocated output buffer - ZERO ALLOCATION in hot path
    let mut outputs: ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER> = ArrayVec::new();

    // Batch buffer for sends (reduces await overhead)
    let mut send_batch: ArrayVec<(ClientId, OutputMessage), 16> = ArrayVec::new();

    eprintln!("Engine task: started with {} pre-registered symbols", DEFAULT_SYMBOLS.len());
    eprintln!("Engine task: max_orders={}, strict_mode={}", 
        engine.config().max_orders, engine.config().strict_mode);

    // Main processing loop
    while let Some(request) = engine_rx.recv().await {
        debug_assert!(outputs.is_empty(), "outputs should be cleared");

        // Track user → client mapping for response routing
        if request.user_id != 0 {
            clients.set_user_id(request.client_id, request.user_id).await;
        }

        // Update metrics based on message type
        match &request.msg {
            InputMessage::NewOrder(_) => metrics.record_order(),
            InputMessage::Cancel(_) => metrics.record_cancel(),
            _ => {}
        }

        // Process through matching engine
        // This writes to our pre-allocated ArrayVec - NO ALLOCATION
        let result = engine.process_message(request.msg, &mut outputs);

        metrics.record_message_processed();

        // Handle engine errors (capacity exceeded, etc.)
        if let Err(e) = result {
            eprintln!("Engine error: {:?}", e);
            metrics.record_reject();
            outputs.clear();
            continue;
        }

        // Route outputs
        send_batch.clear();

        for msg in outputs.iter() {
            // Update trade metrics
            if msg.is_trade() {
                metrics.record_trade();
            }
            if msg.is_reject() {
                metrics.record_reject();
            }

            // Determine routing
            let (target, should_multicast) = route_output(msg, request.client_id);

            // Queue for unicast send
            if let Some(target_id) = target {
                if !send_batch.is_full() {
                    send_batch.push((target_id, *msg));
                }
            }

            // Non-blocking multicast
            if should_multicast {
                if let Some(ref mcast_tx) = multicast_tx {
                    match mcast_tx.try_send(*msg) {
                        Ok(_) => Metrics::inc(&metrics.multicast_messages),
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            metrics.record_channel_drop();
                        }
                        Err(_) => {} // Channel closed
                    }
                }
            }
        }

        // Send batched unicast messages
        for (target_id, msg) in send_batch.iter() {
            if clients.send_to_client(*target_id, *msg).await {
                metrics.record_message_sent();
            } else {
                metrics.record_send_error();
            }
        }

        // Clear for next iteration
        outputs.clear();
    }

    eprintln!("Engine task: shutting down");
    eprintln!("Engine task: final state - {} symbols, {} orders tracked",
        engine.num_symbols(), engine.num_orders());
}

/// Route an output message to its target.
///
/// Returns (target_client, should_multicast).
///
/// Routing rules:
/// - Ack/CancelAck/Reject: originating client only
/// - Trade: both parties (simplified to originator here) + multicast
/// - TopOfBook: multicast only
#[inline]
fn route_output(msg: &OutputMessage, originator: ClientId) -> (Option<ClientId>, bool) {
    match msg {
        OutputMessage::Ack(_) | OutputMessage::CancelAck(_) | OutputMessage::Reject(_) => {
            (Some(originator), false)
        }
        OutputMessage::Trade(_) => {
            // In production, route to both buyer and seller
            // Simplified: just send to originator + multicast
            (Some(originator), true)
        }
        OutputMessage::TopOfBook(_) => {
            // Market data: multicast only, no unicast
            (None, true)
        }
    }
}

/// Alternative: Run with dual-processor mode (A-M / N-Z symbol partitioning).
///
/// This matches your C implementation's architecture for maximum throughput.
#[allow(dead_code)]
pub async fn run_engine_loop_dual_processor(
    mut engine_rx: EngineRx,
    clients: Arc<ClientRegistry>,
    multicast_tx: Option<MulticastTx>,
    metrics: Arc<Metrics>,
) {
    // Two engines: A-M symbols and N-Z symbols
    let config = EngineConfig {
        max_symbols: 512, // Split across two engines
        max_orders: 500_000,
        levels_per_side: 256,
        strict_mode: false,
    };

    let mut engine_a_to_m = MatchingEngine::with_config(config.clone());
    let mut engine_n_to_z = MatchingEngine::with_config(config);

    let mut outputs: ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER> = ArrayVec::new();

    eprintln!("Engine task: dual-processor mode (A-M / N-Z)");

    while let Some(request) = engine_rx.recv().await {
        if request.user_id != 0 {
            clients.set_user_id(request.client_id, request.user_id).await;
        }

        // Route to appropriate engine based on symbol
        let symbol = extract_symbol(&request.msg);
        let engine = if symbol.is_a_to_m() {
            &mut engine_a_to_m
        } else {
            &mut engine_n_to_z
        };

        outputs.clear();
        let _ = engine.process_message(request.msg, &mut outputs);
        metrics.record_message_processed();

        // Route outputs (same as single processor)
        for msg in outputs.iter() {
            if msg.is_trade() {
                metrics.record_trade();
            }

            let (target, should_multicast) = route_output(msg, request.client_id);

            if let Some(target_id) = target {
                if clients.send_to_client(target_id, *msg).await {
                    metrics.record_message_sent();
                }
            }

            if should_multicast {
                if let Some(ref mcast_tx) = multicast_tx {
                    let _ = mcast_tx.try_send(*msg);
                }
            }
        }
    }
}

/// Extract symbol from input message.
#[inline]
fn extract_symbol(msg: &InputMessage) -> Symbol {
    match msg {
        InputMessage::NewOrder(o) => o.symbol,
        InputMessage::Cancel(_) => Symbol::from_str(""), // Cancel doesn't have symbol
        InputMessage::Flush => Symbol::from_str(""),
        InputMessage::QueryTopOfBook(q) => q.symbol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::Side;

    #[test]
    fn test_route_output_ack() {
        let ack = OutputMessage::ack(1, 100, Symbol::from_str("IBM"));
        let (target, multicast) = route_output(&ack, ClientId(42));
        assert_eq!(target, Some(ClientId(42)));
        assert!(!multicast);
    }

    #[test]
    fn test_route_output_trade() {
        let trade = OutputMessage::trade(Symbol::from_str("X"), 1, 1, 2, 2, 100, 10);
        let (target, multicast) = route_output(&trade, ClientId(42));
        assert_eq!(target, Some(ClientId(42)));
        assert!(multicast);
    }

    #[test]
    fn test_route_output_tob() {
        let tob = OutputMessage::top_of_book(Symbol::from_str("X"), Side::Buy, 100, 50);
        let (target, multicast) = route_output(&tob, ClientId(42));
        assert_eq!(target, None);
        assert!(multicast);
    }
}
