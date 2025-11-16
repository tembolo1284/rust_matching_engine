//! Internal order representation used inside the order book.
//!
//! Mirrors your C++ `Order` struct with:
//! - `user_id`, `user_order_id`, `symbol`
//! - `price`, `quantity`, `remaining_qty`
//! - `side`, `type` (market vs limit)
//! - `timestamp` in nanoseconds since epoch
//!
//! This type is **not** exposed over the wire; it's purely internal
//! to the engine-core crate.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::messages::NewOrder;
use crate::order_type::OrderType;
use crate::side::Side;

/// A single order in the book.
///
/// This is analogous to your C++:
/// ```cpp
/// struct Order {
///     uint32_t user_id;
///     uint32_t user_order_id;
///     std::string symbol;
///     uint32_t price;
///     uint32_t quantity;
///     uint32_t remaining_qty;
///     Side side;
///     OrderType type;
///     uint64_t timestamp;
/// };
/// ```
#[derive(Debug, Clone)]
pub struct Order {
    // Order identification
    pub user_id: u32,
    pub user_order_id: u32,
    pub symbol: String,

    // Order details
    pub price: u32,         // 0 = market, >0 = limit
    pub quantity: u32,      // original quantity
    pub remaining_qty: u32, // remaining unfilled quantity
    pub side: Side,
    pub order_type: OrderType,

    // Time priority (nanoseconds since epoch)
    pub timestamp_ns: u64,
}

impl Order {
    /// Construct an `Order` from a [`NewOrder`] message and a given timestamp.
    ///
    /// This mirrors your C++ constructor:
    /// ```cpp
    /// Order(const NewOrderMessage& msg, uint64_t ts)
    ///     : user_id(msg.user_id)
    ///     , user_order_id(msg.user_order_id)
    ///     , symbol(msg.symbol)
    ///     , price(msg.price)
    ///     , quantity(msg.quantity)
    ///     , remaining_qty(msg.quantity)
    ///     , side(msg.side)
    ///     , type(msg.price == 0 ? OrderType::MARKET : OrderType::LIMIT)
    ///     , timestamp(ts)
    /// {}
    /// ```
    pub fn from_new_order(msg: &NewOrder, timestamp_ns: u64) -> Self {
        let order_type = msg.order_type();
        Order {
            user_id: msg.user_id,
            user_order_id: msg.user_order_id,
            symbol: msg.symbol.clone(),
            price: msg.price,
            quantity: msg.quantity,
            remaining_qty: msg.quantity,
            side: msg.side,
            order_type,
            timestamp_ns,
        }
    }

    /// Helper to construct from a `NewOrder` using the current time
    /// as the timestamp (nanoseconds since epoch).
    pub fn from_new_order_now(msg: &NewOrder) -> Self {
        let ts = Self::current_timestamp_ns();
        Self::from_new_order(msg, ts)
    }

    /// Returns `true` if the order is fully filled.
    pub fn is_filled(&self) -> bool {
        self.remaining_qty == 0
    }

    /// Fill the order by up to `qty` units.
    ///
    /// Returns the quantity that was actually filled (which will be
    /// `<= qty` and `<= remaining_qty`).
    ///
    /// Mirrors your C++:
    /// ```cpp
    /// uint32_t fill(uint32_t qty) {
    ///     uint32_t filled = std::min(qty, remaining_qty);
    ///     remaining_qty -= filled;
    ///     return filled;
    /// }
    /// ```
    pub fn fill(&mut self, qty: u32) -> u32 {
        let filled = qty.min(self.remaining_qty);
        self.remaining_qty -= filled;
        filled
    }

    /// Get the current timestamp in nanoseconds since the Unix epoch.
    ///
    /// This is analogous to your C++ `getCurrentTimestamp()` which uses
    /// `std::chrono::high_resolution_clock`.
    pub fn current_timestamp_ns() -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        now.as_secs()
            .saturating_mul(1_000_000_000)
            .saturating_add(now.subsec_nanos() as u64)
    }
}

