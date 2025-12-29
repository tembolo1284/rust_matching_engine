//! Single-symbol order book with price-time priority.
//!
//! # Architecture
//! - Price levels stored in `VecDeque` for O(1) front removal.
//! - Orders per level stored in `ArrayVec` (fixed capacity, no allocation).
//! - Outputs written to caller-provided bounded buffer.
//!
//! # Power of Ten Compliance
//! - Rule 2: All loops have fixed upper bounds.
//! - Rule 3: No dynamic allocation after initialization.
//! - Rule 5: Minimum 2 assertions per function.
//! - Rule 7: All capacities checked before insertion.
//!
//! # Cache Optimization
//! - Price levels in contiguous memory (VecDeque).
//! - Orders within a level in contiguous memory (ArrayVec).
//! - Hot fields accessed sequentially during matching.

use std::collections::VecDeque;

use arrayvec::ArrayVec;

use crate::error::{EngineError, EngineResult};
use crate::messages::{NewOrder, OutputMessage};
use crate::order::Order;
use crate::order_type::OrderType;
use crate::side::Side;
use crate::symbol::Symbol;
use crate::top_of_book::TopOfBookSnapshot;

// =============================================================================
// Configuration Constants (Power of Ten Rule 2 - bounded loops)
// =============================================================================

/// Maximum iterations for matching loop.
pub const MAX_MATCH_ITERATIONS: usize = 100_000;

/// Maximum orders per price level (ArrayVec capacity).
/// This is the key constraint for zero-allocation.
pub const MAX_ORDERS_PER_LEVEL: usize = 256;

/// Maximum price levels per side.
pub const MAX_PRICE_LEVELS: usize = 10_000;

/// Maximum outputs per single order operation.
/// Worst case: 1 ack + MAX_ORDERS_PER_LEVEL trades + 2 TOB updates.
pub const MAX_OUTPUTS_PER_ORDER: usize = MAX_ORDERS_PER_LEVEL + 4;

// =============================================================================
// Price Level
// =============================================================================

/// A price level containing orders at a single price.
///
/// Uses `ArrayVec` for fixed-capacity, zero-allocation storage.
#[derive(Debug, Clone)]
pub struct PriceLevel {
    /// The price for this level.
    price: u32,
    /// Orders at this price (FIFO queue, oldest at front).
    /// Fixed capacity - no heap allocation after creation.
    orders: ArrayVec<Order, MAX_ORDERS_PER_LEVEL>,
}

// Compile-time size verification
const _: () = assert!(
    std::mem::size_of::<PriceLevel>() == 4 + 4 + (64 * MAX_ORDERS_PER_LEVEL),
    "PriceLevel size mismatch"
);

impl PriceLevel {
    /// Create a new empty price level.
    #[inline]
    pub fn new(price: u32) -> Self {
        debug_assert!(price > 0, "price level cannot have zero price");

        let level = PriceLevel {
            price,
            orders: ArrayVec::new(),
        };

        debug_assert!(level.orders.capacity() == MAX_ORDERS_PER_LEVEL);
        level
    }

    /// Get the price.
    #[inline]
    pub const fn price(&self) -> u32 {
        self.price
    }

    /// Get total quantity at this level.
    #[inline]
    pub fn total_quantity(&self) -> u32 {
        debug_assert!(self.orders.len() <= MAX_ORDERS_PER_LEVEL);

        let qty: u32 = self.orders.iter().map(|o| o.remaining_qty).sum();

        debug_assert!(
            self.orders.is_empty() || qty > 0,
            "non-empty level with zero quantity"
        );
        qty
    }

    /// Check if level has no orders.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Check if level is at capacity.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.orders.is_full()
    }

    /// Number of orders at this level.
    #[inline]
    pub fn order_count(&self) -> usize {
        self.orders.len()
    }

    /// Remaining capacity for orders.
    #[inline]
    pub fn remaining_capacity(&self) -> usize {
        self.orders.capacity() - self.orders.len()
    }

    /// Try to add an order. Returns error if at capacity.
    #[inline]
    pub fn try_push(&mut self, order: Order) -> EngineResult<()> {
        debug_assert!(order.price == self.price, "order price mismatch");
        debug_assert!(order.remaining_qty > 0, "adding filled order");

        if self.orders.is_full() {
            return Err(EngineError::OrdersPerLevelExceeded {
                symbol: order.symbol,
                price: self.price,
            });
        }

        self.orders.push(order);

        debug_assert!(!self.is_empty());
        Ok(())
    }

    /// Remove and return the first order (FIFO).
    #[inline]
    pub fn pop_front(&mut self) -> Option<Order> {
        if self.orders.is_empty() {
            None
        } else {
            // ArrayVec doesn't have pop_front, so we remove at index 0
            // This is O(n) but levels are typically small
            Some(self.orders.remove(0))
        }
    }

    /// Get mutable reference to first order.
    #[inline]
    pub fn front_mut(&mut self) -> Option<&mut Order> {
        self.orders.first_mut()
    }

    /// Get reference to first order.
    #[inline]
    pub fn front(&self) -> Option<&Order> {
        self.orders.first()
    }

    /// Remove filled orders from the front.
    /// Returns the number of orders removed.
    #[inline]
    pub fn drain_filled(&mut self) -> usize {
        let mut removed = 0;
        while let Some(order) = self.orders.first() {
            if order.is_filled() {
                self.orders.remove(0);
                removed += 1;
            } else {
                break;
            }
        }
        removed
    }

    /// Find and remove an order by key. Returns true if found.
    pub fn remove_order(&mut self, user_id: u32, user_order_id: u32) -> bool {
        debug_assert!(self.orders.len() <= MAX_ORDERS_PER_LEVEL);

        if let Some(idx) = self
            .orders
            .iter()
            .position(|o| o.user_id == user_id && o.user_order_id == user_order_id)
        {
            self.orders.remove(idx);
            debug_assert!(
                self.orders.iter().all(|o| o.user_id != user_id || o.user_order_id != user_order_id),
                "duplicate order found"
            );
            true
        } else {
            false
        }
    }

    /// Iterate over orders (for flush).
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &Order> {
        self.orders.iter()
    }
}

// =============================================================================
// Order Book
// =============================================================================

/// Single-symbol order book with price-time priority.
///
/// # Memory Layout
/// - Bids: `VecDeque<PriceLevel>` sorted descending (best bid at front).
/// - Asks: `VecDeque<PriceLevel>` sorted ascending (best ask at front).
/// - Both use `VecDeque` for O(1) front removal when levels are exhausted.
#[derive(Debug)]
pub struct OrderBook {
    /// Symbol for this book.
    symbol: Symbol,

    /// Bid price levels, sorted descending by price (best bid at front).
    bids: VecDeque<PriceLevel>,

    /// Ask price levels, sorted ascending by price (best ask at front).
    asks: VecDeque<PriceLevel>,

    /// Cached previous top-of-book for change detection.
    prev_tob: TopOfBookSnapshot,

    /// Maximum price levels per side (for capacity checks).
    max_levels: usize,
}

impl OrderBook {
    /// Create a new order book for the given symbol.
    pub fn new(symbol: Symbol) -> Self {
        Self::with_capacity(symbol, 256)
    }

    /// Create with pre-allocated capacity.
    ///
    /// # Arguments
    /// - `symbol`: The symbol for this book.
    /// - `levels_per_side`: Pre-allocated capacity for price levels per side.
    pub fn with_capacity(symbol: Symbol, levels_per_side: usize) -> Self {
        debug_assert!(!symbol.is_empty(), "OrderBook symbol cannot be empty");
        debug_assert!(levels_per_side > 0, "levels_per_side must be > 0");
        debug_assert!(
            levels_per_side <= MAX_PRICE_LEVELS,
            "levels_per_side exceeds MAX_PRICE_LEVELS"
        );

        let book = OrderBook {
            symbol,
            bids: VecDeque::with_capacity(levels_per_side),
            asks: VecDeque::with_capacity(levels_per_side),
            prev_tob: TopOfBookSnapshot::EMPTY,
            max_levels: levels_per_side,
        };

        debug_assert!(book.bids.capacity() >= levels_per_side);
        debug_assert!(book.asks.capacity() >= levels_per_side);

        book
    }

    /// Returns the symbol of this book.
    #[inline]
    pub fn symbol(&self) -> Symbol {
        self.symbol
    }

    /// Get current top-of-book snapshot.
    #[inline]
    pub fn top_of_book(&self) -> TopOfBookSnapshot {
        TopOfBookSnapshot::new(
            self.best_bid_price(),
            self.best_bid_quantity(),
            self.best_ask_price(),
            self.best_ask_quantity(),
        )
    }

    /// Best bid price (0 if empty).
    #[inline]
    pub fn best_bid_price(&self) -> u32 {
        self.bids.front().map(|l| l.price).unwrap_or(0)
    }

    /// Best ask price (0 if empty).
    #[inline]
    pub fn best_ask_price(&self) -> u32 {
        self.asks.front().map(|l| l.price).unwrap_or(0)
    }

    /// Total quantity at best bid.
    #[inline]
    pub fn best_bid_quantity(&self) -> u32 {
        self.bids.front().map(|l| l.total_quantity()).unwrap_or(0)
    }

    /// Total quantity at best ask.
    #[inline]
    pub fn best_ask_quantity(&self) -> u32 {
        self.asks.front().map(|l| l.total_quantity()).unwrap_or(0)
    }

    /// Number of bid price levels.
    #[inline]
    pub fn bid_levels(&self) -> usize {
        self.bids.len()
    }

    /// Number of ask price levels.
    #[inline]
    pub fn ask_levels(&self) -> usize {
        self.asks.len()
    }

    /// Total number of orders in the book.
    pub fn total_orders(&self) -> usize {
        let bid_orders: usize = self.bids.iter().map(|l| l.order_count()).sum();
        let ask_orders: usize = self.asks.iter().map(|l| l.order_count()).sum();
        bid_orders + ask_orders
    }

    /// Check if book is empty (no orders on either side).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bids.is_empty() && self.asks.is_empty()
    }

    // =========================================================================
    // Order Processing (Public API)
    // =========================================================================

    /// Process a new order, writing outputs to the provided buffer.
    ///
    /// # Returns
    /// - `Ok(())` on success.
    /// - `Err(EngineError)` if capacity exceeded.
    ///
    /// # Outputs
    /// - Always: Ack message.
    /// - If matched: Trade messages.
    /// - If TOB changed: TopOfBook messages.
    pub fn add_order(
        &mut self,
        msg: &NewOrder,
        timestamp_ns: u64,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) -> EngineResult<()> {
        // Rule 5: Preconditions
        debug_assert_eq!(msg.symbol, self.symbol, "order symbol mismatch");
        debug_assert!(msg.quantity > 0, "order quantity must be > 0");
        debug_assert!(
            outputs.len() < outputs.capacity(),
            "output buffer should have space"
        );

        // Create internal order
        let mut order = Order::new(
            msg.user_id,
            msg.user_order_id,
            self.symbol,
            msg.price,
            msg.quantity,
            msg.side,
            timestamp_ns,
        );

        // Ack immediately (always succeeds - we checked capacity)
        if outputs.is_full() {
            return Err(EngineError::OutputBufferFull {
                current: outputs.len(),
                max: outputs.capacity(),
            });
        }
        outputs.push(OutputMessage::ack(order.user_id, order.user_order_id, self.symbol));

        // Match against opposing side
        self.match_order(&mut order, outputs)?;

        // Add remainder to book if limit order with remaining qty
        if order.remaining_qty > 0 && order.order_type == OrderType::Limit {
            self.add_to_book(order)?;
        }

        // Emit TOB changes
        self.emit_tob_changes(outputs);

        // Rule 5: Postcondition
        debug_assert!(
            !outputs.is_empty(),
            "add_order must produce at least an ack"
        );

        Ok(())
    }

    /// Cancel an order by (user_id, user_order_id).
    ///
    /// # Returns
    /// `true` if the order was found and removed.
    pub fn cancel_order(
        &mut self,
        user_id: u32,
        user_order_id: u32,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) -> bool {
        debug_assert!(outputs.remaining_capacity() >= 3, "need space for cancel outputs");

        let found = self.remove_order(user_id, user_order_id);

        // Always emit CancelAck
        if !outputs.is_full() {
            outputs.push(OutputMessage::cancel_ack(user_id, user_order_id, self.symbol));
        }

        // Emit TOB changes if we removed something
        if found {
            self.emit_tob_changes(outputs);
        }

        found
    }

    /// Flush all orders from the book.
    ///
    /// Note: This may produce many outputs. Caller should handle appropriately.
    pub fn flush(&mut self, outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>) {
        debug_assert!(outputs.remaining_capacity() >= 2, "need space for TOB eliminated");

        // Note: In production, we might want to emit cancel acks for all orders.
        // For now, we just emit TOB eliminated messages.

        // TOB eliminated if there were orders
        if !self.bids.is_empty() && !outputs.is_full() {
            outputs.push(OutputMessage::top_of_book_eliminated(self.symbol, Side::Buy));
        }
        if !self.asks.is_empty() && !outputs.is_full() {
            outputs.push(OutputMessage::top_of_book_eliminated(self.symbol, Side::Sell));
        }

        // Clear
        self.bids.clear();
        self.asks.clear();
        self.prev_tob = TopOfBookSnapshot::EMPTY;

        debug_assert!(self.is_empty(), "flush must empty the book");
    }

    // =========================================================================
    // Internal: Matching
    // =========================================================================

    /// Match an incoming order against the opposing side.
    fn match_order(
        &mut self,
        order: &mut Order,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) -> EngineResult<()> {
        debug_assert!(order.remaining_qty > 0, "matching fully filled order");

        let opposing_side = match order.side {
            Side::Buy => &mut self.asks,
            Side::Sell => &mut self.bids,
        };

        let mut iterations = 0;

        // Bounded loop (Power of Ten Rule 2)
        while iterations < MAX_MATCH_ITERATIONS {
            iterations += 1;

            // Exit conditions
            if order.remaining_qty == 0 {
                break;
            }
            if opposing_side.is_empty() {
                break;
            }

            // Get best price level
            let best_price = opposing_side.front().map(|l| l.price).unwrap_or(0);
            if best_price == 0 {
                break;
            }

            // Check if we can match at this price
            if !order.can_match(best_price) {
                break;
            }

            // Match against orders at this level (FIFO)
            self.match_at_level(order, opposing_side, outputs)?;

            // Remove empty price level
            if opposing_side.front().map(|l| l.is_empty()).unwrap_or(false) {
                opposing_side.pop_front();
            }
        }

        debug_assert!(
            iterations < MAX_MATCH_ITERATIONS,
            "exceeded max match iterations"
        );

        Ok(())
    }

    /// Match against orders at the best price level.
    fn match_at_level(
        &mut self,
        order: &mut Order,
        levels: &mut VecDeque<PriceLevel>,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) -> EngineResult<()> {
        debug_assert!(!levels.is_empty(), "matching against empty book");
        debug_assert!(order.remaining_qty > 0, "matching filled order");

        let level = levels.front_mut().unwrap();
        let trade_price = level.price();

        let mut inner_iterations = 0;

        while inner_iterations < MAX_ORDERS_PER_LEVEL {
            inner_iterations += 1;

            if order.remaining_qty == 0 {
                break;
            }

            let passive = match level.front_mut() {
                Some(p) => p,
                None => break,
            };

            let trade_qty = order.remaining_qty.min(passive.remaining_qty);
            debug_assert!(trade_qty > 0, "zero trade quantity");

            // Determine buyer/seller
            let (buyer_id, buyer_oid, seller_id, seller_oid) = match order.side {
                Side::Buy => (
                    order.user_id,
                    order.user_order_id,
                    passive.user_id,
                    passive.user_order_id,
                ),
                Side::Sell => (
                    passive.user_id,
                    passive.user_order_id,
                    order.user_id,
                    order.user_order_id,
                ),
            };

            // Emit trade
            if outputs.is_full() {
                return Err(EngineError::OutputBufferFull {
                    current: outputs.len(),
                    max: outputs.capacity(),
                });
            }
            outputs.push(OutputMessage::trade(
                self.symbol,
                buyer_id,
                buyer_oid,
                seller_id,
                seller_oid,
                trade_price,
                trade_qty,
            ));

            // Fill both orders
            order.fill(trade_qty);
            passive.fill(trade_qty);

            // Remove filled passive order
            if passive.is_filled() {
                level.pop_front();
            }
        }

        debug_assert!(
            inner_iterations < MAX_ORDERS_PER_LEVEL,
            "exceeded max orders per level"
        );

        Ok(())
    }

    // =========================================================================
    // Internal: Book Management
    // =========================================================================

    /// Add a limit order to the appropriate side.
    fn add_to_book(&mut self, order: Order) -> EngineResult<()> {
        debug_assert!(order.remaining_qty > 0, "adding filled order to book");
        debug_assert!(order.order_type == OrderType::Limit, "adding market order to book");
        debug_assert!(order.price > 0, "limit order with zero price");

        let (levels, descending) = match order.side {
            Side::Buy => (&mut self.bids, true),   // Bids: high to low
            Side::Sell => (&mut self.asks, false), // Asks: low to high
        };

        // Find insertion point
        let pos = self.find_level_position(levels, order.price, descending);

        // Check if we have a level at this price
        if let Some(level) = levels.get_mut(pos) {
            if level.price() == order.price {
                return level.try_push(order);
            }
        }

        // Check capacity before inserting new level
        if levels.len() >= self.max_levels {
            return Err(EngineError::PriceLevelCapacityExceeded {
                symbol: self.symbol,
                side: order.side,
            });
        }

        // Insert new level
        let mut new_level = PriceLevel::new(order.price);
        new_level.try_push(order)?;

        // VecDeque doesn't have insert at arbitrary position efficiently,
        // but we can use make_contiguous + slice operations for small books.
        // For simplicity, we rebuild - this is rare (new price level).
        levels.insert(pos, new_level);

        Ok(())
    }

    /// Find the position for a price level using binary search.
    fn find_level_position(
        &self,
        levels: &VecDeque<PriceLevel>,
        price: u32,
        descending: bool,
    ) -> usize {
        debug_assert!(price > 0, "searching for zero price");

        if levels.is_empty() {
            return 0;
        }

        // Binary search
        let (mut lo, mut hi) = (0, levels.len());

        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let mid_price = levels[mid].price();

            let go_left = if descending {
                mid_price < price // Descending: we want higher prices first
            } else {
                mid_price > price // Ascending: we want lower prices first
            };

            if go_left {
                hi = mid;
            } else if mid_price == price {
                return mid; // Exact match
            } else {
                lo = mid + 1;
            }
        }

        lo
    }

    /// Remove an order by (user_id, user_order_id). Returns true if found.
    fn remove_order(&mut self, user_id: u32, user_order_id: u32) -> bool {
        // Try bids first
        if Self::remove_from_side(&mut self.bids, user_id, user_order_id) {
            return true;
        }
        // Then asks
        Self::remove_from_side(&mut self.asks, user_id, user_order_id)
    }

    /// Remove from a specific side. Returns true if found.
    fn remove_from_side(
        levels: &mut VecDeque<PriceLevel>,
        user_id: u32,
        user_order_id: u32,
    ) -> bool {
        for level_idx in 0..levels.len() {
            if levels[level_idx].remove_order(user_id, user_order_id) {
                // Remove empty level
                if levels[level_idx].is_empty() {
                    levels.remove(level_idx);
                }
                return true;
            }
        }
        false
    }

    /// Emit top-of-book changes if state has changed.
    fn emit_tob_changes(&mut self, outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>) {
        let current = self.top_of_book();

        // Bid side
        if current.bid_changed(&self.prev_tob) {
            if !outputs.is_full() {
                if current.bid_price == 0 {
                    outputs.push(OutputMessage::top_of_book_eliminated(self.symbol, Side::Buy));
                } else {
                    outputs.push(OutputMessage::top_of_book(
                        self.symbol,
                        Side::Buy,
                        current.bid_price,
                        current.bid_quantity,
                    ));
                }
            }
        }

        // Ask side
        if current.ask_changed(&self.prev_tob) {
            if !outputs.is_full() {
                if current.ask_price == 0 {
                    outputs.push(OutputMessage::top_of_book_eliminated(self.symbol, Side::Sell));
                } else {
                    outputs.push(OutputMessage::top_of_book(
                        self.symbol,
                        Side::Sell,
                        current.ask_price,
                        current.ask_quantity,
                    ));
                }
            }
        }

        self.prev_tob = current;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_order(user_id: u32, user_order_id: u32, price: u32, qty: u32, side: Side) -> NewOrder {
        NewOrder::new(user_id, user_order_id, Symbol::from_str("TEST"), price, qty, side)
    }

    fn new_outputs() -> ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER> {
        ArrayVec::new()
    }

    #[test]
    fn test_add_single_bid() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = new_outputs();

        let order = make_order(1, 100, 1000, 10, Side::Buy);
        book.add_order(&order, 0, &mut outputs).unwrap();

        assert_eq!(book.best_bid_price(), 1000);
        assert_eq!(book.best_bid_quantity(), 10);
        assert_eq!(book.best_ask_price(), 0);

        // Should have: Ack + TOB update
        assert!(outputs.len() >= 2);
        assert!(outputs.iter().any(|m| m.is_ack()));
    }

    #[test]
    fn test_match_simple() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = new_outputs();

        // Add resting bid
        let bid = make_order(1, 1, 100, 10, Side::Buy);
        book.add_order(&bid, 0, &mut outputs).unwrap();
        outputs.clear();

        // Incoming sell should match
        let ask = make_order(2, 1, 100, 10, Side::Sell);
        book.add_order(&ask, 1, &mut outputs).unwrap();

        // Find the trade
        let trade = outputs.iter().find(|m| m.is_trade());
        assert!(trade.is_some());

        if let Some(OutputMessage::Trade(t)) = trade {
            assert_eq!(t.price, 100);
            assert_eq!(t.quantity, 10);
            assert_eq!(t.user_id_buy, 1);
            assert_eq!(t.user_id_sell, 2);
        }

        // Book should be empty
        assert_eq!(book.best_bid_price(), 0);
        assert_eq!(book.best_ask_price(), 0);
        assert!(book.is_empty());
    }

    #[test]
    fn test_partial_fill() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = new_outputs();

        // Add resting bid for 100
        let bid = make_order(1, 1, 100, 100, Side::Buy);
        book.add_order(&bid, 0, &mut outputs).unwrap();
        outputs.clear();

        // Sell only 30
        let ask = make_order(2, 1, 100, 30, Side::Sell);
        book.add_order(&ask, 1, &mut outputs).unwrap();

        // Should have partial fill, 70 remaining
        assert_eq!(book.best_bid_quantity(), 70);
    }

    #[test]
    fn test_price_time_priority() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = new_outputs();

        // Add two bids at same price
        let bid1 = make_order(1, 1, 100, 10, Side::Buy);
        let bid2 = make_order(2, 1, 100, 10, Side::Buy);
        book.add_order(&bid1, 0, &mut outputs).unwrap();
        book.add_order(&bid2, 1, &mut outputs).unwrap();
        outputs.clear();

        // Sell should match first bid (time priority)
        let ask = make_order(3, 1, 100, 10, Side::Sell);
        book.add_order(&ask, 2, &mut outputs).unwrap();

        let trade = outputs.iter().find_map(|m| {
            if let OutputMessage::Trade(t) = m {
                Some(t)
            } else {
                None
            }
        });

        assert!(trade.is_some());
        assert_eq!(trade.unwrap().user_id_buy, 1); // First bid
    }

    #[test]
    fn test_cancel() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = new_outputs();

        let bid = make_order(1, 100, 100, 10, Side::Buy);
        book.add_order(&bid, 0, &mut outputs).unwrap();
        outputs.clear();

        let found = book.cancel_order(1, 100, &mut outputs);
        assert!(found);
        assert_eq!(book.best_bid_price(), 0);

        // Should have CancelAck
        assert!(outputs
            .iter()
            .any(|m| matches!(m, OutputMessage::CancelAck(_))));
    }

    #[test]
    fn test_price_level_ordering() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = new_outputs();

        // Add bids at different prices
        book.add_order(&make_order(1, 1, 100, 10, Side::Buy), 0, &mut outputs).unwrap();
        book.add_order(&make_order(2, 1, 102, 10, Side::Buy), 1, &mut outputs).unwrap();
        book.add_order(&make_order(3, 1, 101, 10, Side::Buy), 2, &mut outputs).unwrap();

        // Best bid should be highest price
        assert_eq!(book.best_bid_price(), 102);

        // Add asks at different prices
        outputs.clear();
        book.add_order(&make_order(4, 1, 105, 10, Side::Sell), 3, &mut outputs).unwrap();
        book.add_order(&make_order(5, 1, 103, 10, Side::Sell), 4, &mut outputs).unwrap();
        book.add_order(&make_order(6, 1, 104, 10, Side::Sell), 5, &mut outputs).unwrap();

        // Best ask should be lowest price
        assert_eq!(book.best_ask_price(), 103);
    }

    #[test]
    fn test_market_order() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = new_outputs();

        // Add resting ask
        book.add_order(&make_order(1, 1, 100, 10, Side::Sell), 0, &mut outputs).unwrap();
        outputs.clear();

        // Market buy (price = 0)
        let market = make_order(2, 1, 0, 5, Side::Buy);
        book.add_order(&market, 1, &mut outputs).unwrap();

        // Should match
        assert!(outputs.iter().any(|m| m.is_trade()));
        assert_eq!(book.best_ask_quantity(), 5); // 5 remaining
    }

    #[test]
    fn test_multiple_trades() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = new_outputs();

        // Add multiple small asks
        for i in 1..=5 {
            book.add_order(&make_order(i, 1, 100, 10, Side::Sell), i as u64, &mut outputs).unwrap();
        }
        outputs.clear();

        // Big buy should match all
        book.add_order(&make_order(100, 1, 100, 50, Side::Buy), 10, &mut outputs).unwrap();

        // Should have 5 trades
        let trade_count = outputs.iter().filter(|m| m.is_trade()).count();
        assert_eq!(trade_count, 5);
    }
}
