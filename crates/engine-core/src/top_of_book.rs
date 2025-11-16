//! Helper types for representing top-of-book state.
//!
//! This is separate from the [`OutputMessage::TopOfBook`](crate::messages::TopOfBook)
//! event type so that the engine can use a small, internal snapshot
//! type for queries and comparisons.

/// A simple snapshot of top-of-book for a single symbol.
///
/// This is useful for:
/// - answering `QueryTopOfBook` requests,
/// - comparing against previous state to decide if we should emit
///   a `TopOfBook` output event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TopOfBookSnapshot {
    /// Best bid price (0 if no bid).
    pub bid_price: u32,
    /// Total quantity at best bid (0 if no bid).
    pub bid_quantity: u32,

    /// Best ask price (0 if no ask).
    pub ask_price: u32,
    /// Total quantity at best ask (0 if no ask).
    pub ask_quantity: u32,
}

impl TopOfBookSnapshot {
    pub fn new(bid_price: u32, bid_quantity: u32, ask_price: u32, ask_quantity: u32) -> Self {
        TopOfBookSnapshot {
            bid_price,
            bid_quantity,
            ask_price,
            ask_quantity,
        }
    }

    /// Returns `true` if there is *no* bid and *no* ask.
    pub fn is_empty(&self) -> bool {
        self.bid_price == 0 && self.ask_price == 0
    }
}

