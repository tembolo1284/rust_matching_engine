//! Top-of-book snapshot type for internal state tracking.
//!
//! This is a compact, `Copy` type used by the order book to:
//! - Track previous state for change detection.
//! - Answer top-of-book queries.
//!
//! Separate from `OutputMessage::TopOfBook` which is the wire format.

/// Snapshot of top-of-book for a single symbol.
///
/// Size: 16 bytes, `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct TopOfBookSnapshot {
    /// Best bid price (0 if no bids).
    pub bid_price: u32,
    /// Total quantity at best bid.
    pub bid_quantity: u32,
    /// Best ask price (0 if no asks).
    pub ask_price: u32,
    /// Total quantity at best ask.
    pub ask_quantity: u32,
}

impl TopOfBookSnapshot {
    /// Create a new snapshot.
    #[inline]
    pub const fn new(
        bid_price: u32,
        bid_quantity: u32,
        ask_price: u32,
        ask_quantity: u32,
    ) -> Self {
        TopOfBookSnapshot {
            bid_price,
            bid_quantity,
            ask_price,
            ask_quantity,
        }
    }

    /// Empty snapshot (no bids, no asks).
    pub const EMPTY: Self = Self::new(0, 0, 0, 0);

    /// Check if both sides are empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.bid_price == 0 && self.ask_price == 0
    }

    /// Check if bid side has orders.
    #[inline]
    pub const fn has_bid(&self) -> bool {
        self.bid_price > 0
    }

    /// Check if ask side has orders.
    #[inline]
    pub const fn has_ask(&self) -> bool {
        self.ask_price > 0
    }

    /// Calculate the spread (ask - bid). Returns None if either side is empty.
    #[inline]
    pub fn spread(&self) -> Option<u32> {
        if self.has_bid() && self.has_ask() {
            Some(self.ask_price.saturating_sub(self.bid_price))
        } else {
            None
        }
    }

    /// Calculate the mid price. Returns None if either side is empty.
    #[inline]
    pub fn mid_price(&self) -> Option<u32> {
        if self.has_bid() && self.has_ask() {
            Some((self.bid_price + self.ask_price) / 2)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_size() {
        assert_eq!(std::mem::size_of::<TopOfBookSnapshot>(), 16);
    }

    #[test]
    fn test_snapshot_empty() {
        let snap = TopOfBookSnapshot::EMPTY;
        assert!(snap.is_empty());
        assert!(!snap.has_bid());
        assert!(!snap.has_ask());
        assert_eq!(snap.spread(), None);
    }

    #[test]
    fn test_snapshot_spread() {
        let snap = TopOfBookSnapshot::new(100, 10, 102, 20);
        assert!(!snap.is_empty());
        assert_eq!(snap.spread(), Some(2));
        assert_eq!(snap.mid_price(), Some(101));
    }
}
