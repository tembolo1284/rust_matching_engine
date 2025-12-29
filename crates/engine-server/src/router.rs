//! Message routing logic.
//!
//! Note: Most routing is now inlined in engine_task.rs for performance.
//! This module provides advanced routing for trade messages.

use std::sync::Arc;

use arrayvec::ArrayVec;
use engine_core::OutputMessage;

use crate::types::{ClientId, ClientRegistry};

/// Maximum unicast targets per message.
/// Trade: buyer + seller = 2
/// With some margin for future use.
pub const MAX_UNICAST_TARGETS: usize = 4;

/// Routing result with zero allocation.
pub type UnicastTargets = ArrayVec<(ClientId, OutputMessage), MAX_UNICAST_TARGETS>;

/// Route a trade to both buyer and seller.
///
/// This is the full routing logic for trades when we have user->client mapping.
pub async fn route_trade(
    trade: &engine_core::Trade,
    msg: OutputMessage,
    registry: &Arc<ClientRegistry>,
) -> UnicastTargets {
    let mut targets = ArrayVec::new();

    // Send to buyer
    if let Some(buyer_client) = registry.get_client_for_user(trade.user_id_buy).await {
        if !targets.is_full() {
            targets.push((buyer_client, msg));
        }
    }

    // Send to seller (if different)
    if trade.user_id_buy != trade.user_id_sell {
        if let Some(seller_client) = registry.get_client_for_user(trade.user_id_sell).await {
            if !targets.is_full() {
                targets.push((seller_client, msg));
            }
        }
    }

    targets
}

/// Simple routing: always to originator.
#[inline]
pub fn route_to_originator(
    msg: &OutputMessage,
    originating_client: ClientId,
) -> (UnicastTargets, bool) {
    let should_multicast = matches!(msg, OutputMessage::Trade(_) | OutputMessage::TopOfBook(_));

    let mut targets = ArrayVec::new();
    
    // TopOfBook is multicast-only
    if !matches!(msg, OutputMessage::TopOfBook(_)) {
        targets.push((originating_client, *msg));
    }

    (targets, should_multicast)
}
