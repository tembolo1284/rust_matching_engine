//! Message routing logic.
//!
//! Determines which clients should receive each output message.

use engine_core::OutputMessage;
use crate::types::{ClientId, ClientRegistry};
use std::sync::Arc;

/// Route an output message to the appropriate recipients.
///
/// Returns:
/// - `unicast_targets`: List of (client_id, message) to send directly.
/// - `should_multicast`: Whether to publish to multicast.
#[allow(dead_code)]
pub async fn route_message(
    msg: &OutputMessage,
    originating_client: ClientId,
    registry: &Arc<ClientRegistry>,
) -> (Vec<(ClientId, OutputMessage)>, bool) {
    let mut unicast = Vec::new();
    let should_multicast: bool;

    match msg {
        OutputMessage::Ack(_ack) => {
            // Ack goes only to originating client
            unicast.push((originating_client, msg.clone()));
            should_multicast = false;
        }

        OutputMessage::CancelAck(_cancel_ack) => {
            // CancelAck goes only to originating client
            unicast.push((originating_client, msg.clone()));
            should_multicast = false;
        }

        OutputMessage::Trade(trade) => {
            // Trade goes to both buyer and seller + multicast
            
            // Send to buyer
            if let Some(buyer_client) = registry.get_client_for_user(trade.user_id_buy).await {
                unicast.push((buyer_client, msg.clone()));
            }
            
            // Send to seller (if different from buyer)
            if trade.user_id_buy != trade.user_id_sell {
                if let Some(seller_client) = registry.get_client_for_user(trade.user_id_sell).await {
                    unicast.push((seller_client, msg.clone()));
                }
            }
            
            // Also multicast for market data
            should_multicast = true;
        }

        OutputMessage::TopOfBook(_) => {
            // TopOfBook is market data - multicast only
            unicast.clear();
            should_multicast = true;
        }
    }

    (unicast, should_multicast)
}

/// Simplified routing: send to originating client only.
/// Used when we don't have user ID tracking set up.
pub fn route_to_originator(
    msg: &OutputMessage,
    originating_client: ClientId,
) -> (Vec<(ClientId, OutputMessage)>, bool) {
    let should_multicast = matches!(msg, OutputMessage::Trade(_) | OutputMessage::TopOfBook(_));
    
    (vec![(originating_client, msg.clone())], should_multicast)
}
