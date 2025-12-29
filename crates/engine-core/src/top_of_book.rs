//! Top-of-book snapshot type for internal state tracking.
//!
//! # Design
//! This is a compact, `Copy` type used by the order book to:
//! - Track previous state for change detection.
//! - Answer top-of-book queries efficiently.
//!
//! # Power of Ten Compliance
//! - Rule 3: No dynamic allocation.
//! - Rule 5: Assertions in non-trivial functions.

/// Snapshot of top-of-book for a single symbol.
///
/// Size: 16 bytes, `Copy`, cache-friendly.
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

// Compile-time size verification
const _: () = assert!(std::mem::size_of::<TopOfBookSnapshot>() == 16, "TopOfBookSnapshot must be 16 bytes");

impl TopOfBookSnapshot {
    /// Empty snapshot (no bids, no asks).
    pub const EMPTY: Self = Self::new(0, 0, 0, 0);

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

    /// Create snapshot for bid side only.
    #[inline]
    pub const fn bid_only(price: u32, quantity: u32) -> Self {
        Self::new(price, quantity, 0, 0)
    }

    /// Create snapshot for ask side only.
    #[inline]
    pub const fn ask_only(price: u32, quantity: u32) -> Self {
        Self::new(0, 0, price, quantity)
    }

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

    /// Check if book is two-sided (has both bid and ask).
    #[inline]
    pub const fn is_two_sided(&self) -> bool {
        self.has_bid() && self.has_ask()
    }

    /// Calculate the spread (ask - bid). Returns None if either side is empty.
    #[inline]
    pub fn spread(&self) -> Option<u32> {
        if self.is_two_sided() {
            debug_assert!(
                self.ask_price >= self.bid_price,
                "crossed book: ask {} < bid {}",
                self.ask_price,
                self.bid_price
            );
            Some(self.ask_price.saturating_sub(self.bid_price))
        } else {
            None
        }
    }

    /// Calculate the mid price. Returns None if either side is empty.
    #[inline]
    pub fn mid_price(&self) -> Option<u32> {
        if self.is_two_sided() {
            debug_assert!(
                self.ask_price >= self.bid_price,
                "crossed book in mid_price"
            );
            Some((self.bid_price + self.ask_price) / 2)
        } else {
            None
        }
    }

    /// Check if this snapshot differs from another on the bid side.
    #[inline]
    pub const fn bid_changed(&self, other: &Self) -> bool {
        self.bid_price != other.bid_price || self.bid_quantity != other.bid_quantity
    }

    /// Check if this snapshot differs from another on the ask side.
    #[inline]
    pub const fn ask_changed(&self, other: &Self) -> bool {
        self.ask_price != other.ask_price || self.ask_quantity != other.ask_quantity
    }

    /// Check if any side has changed.
    #[inline]
    pub const fn changed(&self, other: &Self) -> bool {
        self.bid_changed(other) || self.ask_changed(other)
    }

    /// Validate invariants (debug only).
    #[inline]
    pub fn validate(&self) -> bool {
        // If price is 0, quantity should also be 0
        let bid_valid = (self.bid_price == 0) == (self.bid_quantity == 0) || self.bid_price > 0;
        let ask_valid = (self.ask_price == 0) == (self.ask_quantity == 0) || self.ask_price > 0;
        // If two-sided, ask should be >= bid (not crossed)
        let not_crossed = !self.is_two_sided() || self.ask_price >= self.bid_price;

        bid_valid && ask_valid && not_crossed
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
        assert!(!snap.is_two_sided());
        assert_eq!(snap.spread(), None);
        assert_eq!(snap.mid_price(), None);
        assert!(snap.validate());
    }

    #[test]
    fn test_snapshot_bid_only() {
        let snap = TopOfBookSnapshot::bid_only(100, 50);
        assert!(!snap.is_empty());
        assert!(snap.has_bid());
        assert!(!snap.has_ask());
        assert!(!snap.is_two_sided());
        assert_eq!(snap.spread(), None);
        assert!(snap.validate());
    }

    #[test]
    fn test_snapshot_two_sided() {
        let snap = TopOfBookSnapshot::new(100, 10, 102, 20);
        assert!(!snap.is_empty());
        assert!(snap.is_two_sided());
        assert_eq!(snap.spread(), Some(2));
        assert_eq!(snap.mid_price(), Some(101));
        assert!(snap.validate());
    }

    #[test]
    fn test_snapshot_change_detection() {
        let snap1 = TopOfBookSnapshot::new(100, 10, 102, 20);
        let snap2 = TopOfBookSnapshot::new(100, 15, 102, 20); // bid qty changed
        let snap3 = TopOfBookSnapshot::new(100, 10, 103, 20); // ask price changed

        assert!(!snap1.changed(&snap1));
        assert!(snap1.changed(&snap2));
        assert!(snap1.bid_changed(&snap2));
        assert!(!snap1.ask_changed(&snap2));
        assert!(snap1.ask_changed(&snap3));
    }
}
