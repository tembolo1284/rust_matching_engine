//! Internal order representation used inside the order book.
//!
//! # Cache Optimization
//! - `repr(C, align(64))`: One order per cache line prevents false sharing.
//! - Hot fields (remaining_qty, price) grouped at start.
//! - Total size: 64 bytes (one cache line).
//!
//! # Power of Ten Compliance
//! - Rule 3: No dynamic allocation.
//! - Rule 5: Minimum 2 assertions per function.
//! - Rule 10: All warnings enabled.

use crate::order_type::OrderType;
use crate::side::Side;
use crate::symbol::Symbol;

/// A single order in the book.
///
/// # Memory Layout (64 bytes, cache-line aligned)
///
/// ```text
/// Offset  Size  Field
/// ------  ----  -----
///   0      4    remaining_qty (hot - checked every match)
///   4      4    price         (hot - compared every match)
///   8      4    quantity      (original, for fill reporting)
///  12      4    user_id
///  16      4    user_order_id
///  20      1    side
///  21      1    order_type
///  22      2    _pad1
///  24      8    timestamp_ns
///  32      8    symbol
///  40     24    _pad2         (pad to 64 bytes)
/// ```
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct Order {
    // === Hot fields (accessed every match iteration) ===
    /// Remaining unfilled quantity. Decremented on each fill.
    pub remaining_qty: u32,
    /// Price in ticks. 0 = market order.
    pub price: u32,

    // === Warm fields (accessed on fill) ===
    /// Original quantity (for fill calculation).
    pub quantity: u32,
    /// User/session identifier.
    pub user_id: u32,
    /// User-assigned order identifier (for cancel/fill reporting).
    pub user_order_id: u32,

    // === Cold fields (rarely accessed after creation) ===
    /// Buy or Sell.
    pub side: Side,
    /// Market or Limit.
    pub order_type: OrderType,
    /// Padding for alignment.
    _pad1: [u8; 2],
    /// Timestamp in nanoseconds since epoch (for time priority).
    pub timestamp_ns: u64,
    /// Symbol (fixed 8 bytes, no allocation).
    pub symbol: Symbol,

    /// Padding to reach exactly 64 bytes (one cache line).
    _pad2: [u8; 24],
}

// =============================================================================
// Compile-time verification (Power of Ten Rule 2 & 10)
// =============================================================================
const _: () = assert!(std::mem::size_of::<Order>() == 64, "Order must be exactly 64 bytes");
const _: () = assert!(std::mem::align_of::<Order>() == 64, "Order must be 64-byte aligned");

// Field offset verification (matches C version)
const _: () = assert!(std::mem::offset_of!(Order, remaining_qty) == 0);
const _: () = assert!(std::mem::offset_of!(Order, price) == 4);
const _: () = assert!(std::mem::offset_of!(Order, quantity) == 8);
const _: () = assert!(std::mem::offset_of!(Order, user_id) == 12);
const _: () = assert!(std::mem::offset_of!(Order, user_order_id) == 16);
const _: () = assert!(std::mem::offset_of!(Order, side) == 20);
const _: () = assert!(std::mem::offset_of!(Order, order_type) == 21);
const _: () = assert!(std::mem::offset_of!(Order, timestamp_ns) == 24);
const _: () = assert!(std::mem::offset_of!(Order, symbol) == 32);

impl Order {
    /// Create a new order.
    ///
    /// # Panics (debug only)
    /// - If `quantity` is zero.
    /// - If `user_order_id` is zero (invalid ID).
    #[inline]
    pub fn new(
        user_id: u32,
        user_order_id: u32,
        symbol: Symbol,
        price: u32,
        quantity: u32,
        side: Side,
        timestamp_ns: u64,
    ) -> Self {
        // Rule 5: Precondition assertions
        debug_assert!(quantity > 0, "Order quantity must be > 0");
        debug_assert!(user_order_id > 0, "Order ID must be > 0");
        debug_assert!(!symbol.is_empty(), "Symbol cannot be empty");

        let order_type = OrderType::from_price(price);

        let order = Order {
            remaining_qty: quantity,
            price,
            quantity,
            user_id,
            user_order_id,
            side,
            order_type,
            _pad1: [0; 2],
            timestamp_ns,
            symbol,
            _pad2: [0; 24],
        };

        // Rule 5: Postcondition assertions
        debug_assert!(order.remaining_qty == order.quantity, "Initial remaining must equal quantity");
        debug_assert!(order.remaining_qty > 0, "Order must have positive quantity");

        order
    }

    /// Create an empty/invalid order (for pre-allocation in pools).
    ///
    /// # Safety
    /// Empty orders must not be used in matching. Check `is_valid()` before use.
    #[inline]
    pub const fn empty() -> Self {
        Order {
            remaining_qty: 0,
            price: 0,
            quantity: 0,
            user_id: 0,
            user_order_id: 0,
            side: Side::Buy,
            order_type: OrderType::Market,
            _pad1: [0; 2],
            timestamp_ns: 0,
            symbol: Symbol::EMPTY,
            _pad2: [0; 24],
        }
    }

    /// Check if this is a valid (non-empty) order.
    #[inline]
    pub const fn is_valid(&self) -> bool {
        self.quantity > 0 && self.user_order_id > 0
    }

    /// Returns `true` if the order is fully filled.
    #[inline]
    pub fn is_filled(&self) -> bool {
        debug_assert!(self.remaining_qty <= self.quantity, "remaining > quantity invariant");
        debug_assert!(self.is_valid() || self.remaining_qty == 0, "invalid order state");
        self.remaining_qty == 0
    }

    /// Fill the order by up to `qty` units.
    ///
    /// Returns the quantity actually filled (<= qty, <= remaining_qty).
    ///
    /// # Panics (debug only)
    /// - If `qty` is zero.
    /// - If invariants are violated.
    #[inline]
    pub fn fill(&mut self, qty: u32) -> u32 {
        // Rule 5: Precondition assertions
        debug_assert!(qty > 0, "fill() called with zero quantity");
        debug_assert!(
            self.remaining_qty <= self.quantity,
            "pre-fill invariant: remaining_qty ({}) > quantity ({})",
            self.remaining_qty,
            self.quantity
        );
        debug_assert!(self.remaining_qty > 0, "filling already-filled order");

        let filled = qty.min(self.remaining_qty);
        self.remaining_qty -= filled;

        // Rule 5: Postcondition assertions
        debug_assert!(
            self.remaining_qty <= self.quantity,
            "post-fill invariant violated"
        );
        debug_assert!(filled > 0, "fill() must fill at least 1 unit");

        filled
    }

    /// Get filled quantity (original - remaining).
    #[inline]
    pub fn filled_qty(&self) -> u32 {
        debug_assert!(self.remaining_qty <= self.quantity, "invariant violation");
        debug_assert!(self.quantity >= self.remaining_qty, "underflow check");
        self.quantity - self.remaining_qty
    }

    /// Check if this order can match against a passive order at `passive_price`.
    ///
    /// - Buy orders match if `passive_price <= self.price` (or market).
    /// - Sell orders match if `passive_price >= self.price` (or market).
    #[inline]
    pub fn can_match(&self, passive_price: u32) -> bool {
        debug_assert!(passive_price > 0, "passive price must be > 0 (resting orders are limit)");
        debug_assert!(self.remaining_qty > 0, "matching filled order");

        match self.order_type {
            OrderType::Market => true,
            OrderType::Limit => match self.side {
                Side::Buy => passive_price <= self.price,
                Side::Sell => passive_price >= self.price,
            },
        }
    }

    /// Get the order key (user_id, user_order_id) for tracking.
    #[inline]
    pub const fn key(&self) -> (u32, u32) {
        (self.user_id, self.user_order_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_size_and_alignment() {
        assert_eq!(std::mem::size_of::<Order>(), 64);
        assert_eq!(std::mem::align_of::<Order>(), 64);
    }

    #[test]
    fn test_order_field_offsets() {
        // Verify hot fields are at the start
        assert_eq!(std::mem::offset_of!(Order, remaining_qty), 0);
        assert_eq!(std::mem::offset_of!(Order, price), 4);
    }

    #[test]
    fn test_order_fill() {
        let mut order = Order::new(
            1,
            100,
            Symbol::from_str("IBM"),
            1000,
            50,
            Side::Buy,
            123456789,
        );

        assert_eq!(order.remaining_qty, 50);
        assert!(!order.is_filled());
        assert!(order.is_valid());

        let filled = order.fill(20);
        assert_eq!(filled, 20);
        assert_eq!(order.remaining_qty, 30);
        assert_eq!(order.filled_qty(), 20);

        let filled = order.fill(100); // Try to fill more than remaining
        assert_eq!(filled, 30);
        assert_eq!(order.remaining_qty, 0);
        assert!(order.is_filled());
    }

    #[test]
    fn test_order_can_match() {
        // Buy limit at 100
        let buy = Order::new(1, 1, Symbol::from_str("IBM"), 100, 10, Side::Buy, 0);
        assert!(buy.can_match(100)); // exact
        assert!(buy.can_match(90));  // better price
        assert!(!buy.can_match(110)); // worse price

        // Sell limit at 100
        let sell = Order::new(1, 1, Symbol::from_str("IBM"), 100, 10, Side::Sell, 0);
        assert!(sell.can_match(100)); // exact
        assert!(sell.can_match(110)); // better price
        assert!(!sell.can_match(90)); // worse price

        // Market orders match anything
        let market = Order::new(1, 1, Symbol::from_str("IBM"), 0, 10, Side::Buy, 0);
        assert!(market.can_match(1));
        assert!(market.can_match(1000000));
    }

    #[test]
    fn test_order_empty() {
        let empty = Order::empty();
        assert!(!empty.is_valid());
        assert!(empty.is_filled()); // remaining_qty == 0
    }

    #[test]
    fn test_order_key() {
        let order = Order::new(42, 100, Symbol::from_str("X"), 50, 10, Side::Buy, 0);
        assert_eq!(order.key(), (42, 100));
    }
}
