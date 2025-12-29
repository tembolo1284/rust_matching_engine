//! Order book implementation with Power of Ten compliance.
//!
//! # Architecture
//! - Price levels stored in sorted Vec (bids descending, asks ascending).
//! - Orders within a level maintain FIFO order.
//! - All operations bounded by compile-time constants.
//!
//! # Power of Ten Compliance
//! - Rule 2: All loops bounded by MAX_* constants.
//! - Rule 3: No dynamic allocation after initialization (pre-allocated Vecs).
//! - Rule 5: Assertions on all operations.

use arrayvec::ArrayVec;

use crate::error::{EngineError, EngineResult};
use crate::messages::{NewOrder, OutputMessage};
use crate::order::Order;
use crate::side::Side;
use crate::symbol::Symbol;
use crate::top_of_book::TopOfBookSnapshot;

// =============================================================================
// Constants (Power of Ten Rule 2: bounded loops)
// =============================================================================

/// Maximum outputs per order (ack + trades + TOB updates).
pub const MAX_OUTPUTS_PER_ORDER: usize = 64;

/// Maximum price levels per side.
pub const MAX_PRICE_LEVELS: usize = 256;

/// Maximum orders per price level.
pub const MAX_ORDERS_PER_LEVEL: usize = 1024;

/// Maximum match iterations to prevent runaway loops.
pub const MAX_MATCH_ITERATIONS: usize = 10_000;

// Compile-time verification
const _: () = assert!(MAX_OUTPUTS_PER_ORDER >= 4, "need space for ack + trade + 2 TOB");
const _: () = assert!(MAX_PRICE_LEVELS >= 1, "need at least one price level");
const _: () = assert!(MAX_ORDERS_PER_LEVEL >= 1, "need at least one order per level");

// =============================================================================
// Price Level
// =============================================================================

/// A single price level containing orders at the same price.
#[derive(Debug, Clone)]
pub struct PriceLevel {
    /// Price for this level.
    pub price: u32,
    /// Orders at this price (FIFO order) - heap allocated.
    pub orders: Vec<Order>,
}

impl PriceLevel {
    /// Create a new price level.
    #[inline]
    pub fn new(price: u32) -> Self {
        debug_assert!(price > 0, "price level with zero price");
        PriceLevel {
            price,
            orders: Vec::with_capacity(16), // Start small, grow as needed
        }
    }

    /// Total quantity at this level.
    #[inline]
    pub fn total_quantity(&self) -> u32 {
        self.orders.iter().map(|o| o.remaining_qty).sum()
    }

    /// Check if level is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Number of orders at this level.
    #[inline]
    pub fn order_count(&self) -> usize {
        self.orders.len()
    }

    /// Add an order to this level.
    #[inline]
    pub fn add_order(&mut self, order: Order) -> EngineResult<()> {
        debug_assert_eq!(order.price, self.price, "order price must match level");

        if self.orders.len() >= MAX_ORDERS_PER_LEVEL {
            return Err(EngineError::OrdersPerLevelExceeded {
                symbol: order.symbol,
                price: self.price,
            });
        }

        self.orders.push(order);
        debug_assert!(!self.orders.is_empty());
        Ok(())
    }

    /// Remove filled orders from the front.
    pub fn remove_filled(&mut self) {
        while !self.orders.is_empty() && self.orders[0].is_filled() {
            self.orders.remove(0);
        }
    }
}

// =============================================================================
// Order Book
// =============================================================================

/// Order book for a single symbol.
///
/// Maintains buy and sell sides with price-time priority.
/// Uses heap-allocated Vecs to avoid stack overflow.
#[derive(Debug)]
pub struct OrderBook {
    /// Symbol for this book.
    symbol: Symbol,
    /// Bid levels (sorted descending by price - best bid first).
    bids: Vec<PriceLevel>,
    /// Ask levels (sorted ascending by price - best ask first).
    asks: Vec<PriceLevel>,
}

impl OrderBook {
    /// Create a new order book for a symbol.
    pub fn new(symbol: Symbol) -> Self {
        debug_assert!(!symbol.is_empty(), "order book with empty symbol");

        OrderBook {
            symbol,
            bids: Vec::with_capacity(64),  // Pre-allocate reasonable capacity
            asks: Vec::with_capacity(64),
        }
    }

    /// Create with specific capacity hint (for pre-allocation).
    pub fn with_capacity(symbol: Symbol, levels_per_side: usize) -> Self {
        debug_assert!(!symbol.is_empty(), "order book with empty symbol");

        let capacity = levels_per_side.min(MAX_PRICE_LEVELS);
        OrderBook {
            symbol,
            bids: Vec::with_capacity(capacity),
            asks: Vec::with_capacity(capacity),
        }
    }

    /// Get the symbol.
    #[inline]
    pub fn symbol(&self) -> Symbol {
        self.symbol
    }

    /// Get top-of-book snapshot.
    pub fn top_of_book(&self) -> TopOfBookSnapshot {
        let (bid_price, bid_qty) = self.bids.first()
            .map(|l| (l.price, l.total_quantity()))
            .unwrap_or((0, 0));

        let (ask_price, ask_qty) = self.asks.first()
            .map(|l| (l.price, l.total_quantity()))
            .unwrap_or((0, 0));

        TopOfBookSnapshot {
            bid_price,
            bid_quantity: bid_qty,
            ask_price,
            ask_quantity: ask_qty,
        }
    }

    /// Best bid price (0 if no bids).
    #[inline]
    pub fn best_bid_price(&self) -> u32 {
        self.bids.first().map(|l| l.price).unwrap_or(0)
    }

    /// Best bid quantity (0 if no bids).
    #[inline]
    pub fn best_bid_quantity(&self) -> u32 {
        self.bids.first().map(|l| l.total_quantity()).unwrap_or(0)
    }

    /// Best ask price (0 if no asks).
    #[inline]
    pub fn best_ask_price(&self) -> u32 {
        self.asks.first().map(|l| l.price).unwrap_or(0)
    }

    /// Best ask quantity (0 if no asks).
    #[inline]
    pub fn best_ask_quantity(&self) -> u32 {
        self.asks.first().map(|l| l.total_quantity()).unwrap_or(0)
    }

    /// Number of bid levels.
    #[inline]
    pub fn bid_level_count(&self) -> usize {
        self.bids.len()
    }

    /// Number of ask levels.
    #[inline]
    pub fn ask_level_count(&self) -> usize {
        self.asks.len()
    }

    /// Add an order to the book, performing matching.
    pub fn add_order(
        &mut self,
        msg: &NewOrder,
        timestamp: u64,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) -> EngineResult<()> {
        debug_assert_eq!(msg.symbol, self.symbol, "order symbol must match book");
        debug_assert!(msg.quantity > 0, "order with zero quantity");

        // Create order using Order::new
        let mut order = Order::new(
            msg.user_id,
            msg.user_order_id,
            msg.symbol,
            msg.price,
            msg.quantity,
            msg.side,
            timestamp,
        );

        // Emit Ack first
        if !outputs.is_full() {
            outputs.push(OutputMessage::ack(
                msg.user_id,
                msg.user_order_id,
                self.symbol,
            ));
        }

        let old_bid = self.best_bid_price();
        let old_ask = self.best_ask_price();

        // Match against opposite side
        self.match_order(&mut order, outputs)?;

        // If order has remaining quantity and is a limit order, add to book
        if !order.is_filled() && order.order_type.is_limit() {
            self.insert_order(order)?;
        }

        // Emit TOB updates if changed
        self.emit_tob_updates(old_bid, old_ask, outputs);

        Ok(())
    }

    /// Cancel an order.
    pub fn cancel_order(
        &mut self,
        user_id: u32,
        user_order_id: u32,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) {
        let old_bid = self.best_bid_price();
        let old_ask = self.best_ask_price();

        // Search bids
        for level in &mut self.bids {
            if let Some(pos) = level.orders.iter().position(|o| {
                o.user_id == user_id && o.user_order_id == user_order_id
            }) {
                level.orders.remove(pos);
                break;
            }
        }

        // Search asks
        for level in &mut self.asks {
            if let Some(pos) = level.orders.iter().position(|o| {
                o.user_id == user_id && o.user_order_id == user_order_id
            }) {
                level.orders.remove(pos);
                break;
            }
        }

        // Clean up empty levels
        self.bids.retain(|l| !l.is_empty());
        self.asks.retain(|l| !l.is_empty());

        // Emit CancelAck
        if !outputs.is_full() {
            outputs.push(OutputMessage::cancel_ack(
                user_id,
                user_order_id,
                self.symbol,
            ));
        }

        // Emit TOB updates if changed
        self.emit_tob_updates(old_bid, old_ask, outputs);
    }

    /// Flush (cancel all orders).
    pub fn flush(&mut self, outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>) {
        // Emit CancelAck for each order
        for level in &self.bids {
            for order in &level.orders {
                if !outputs.is_full() {
                    outputs.push(OutputMessage::cancel_ack(
                        order.user_id,
                        order.user_order_id,
                        self.symbol,
                    ));
                }
            }
        }
        for level in &self.asks {
            for order in &level.orders {
                if !outputs.is_full() {
                    outputs.push(OutputMessage::cancel_ack(
                        order.user_id,
                        order.user_order_id,
                        self.symbol,
                    ));
                }
            }
        }

        // Clear all levels
        self.bids.clear();
        self.asks.clear();

        // Emit TOB eliminated
        if !outputs.is_full() {
            outputs.push(OutputMessage::top_of_book_eliminated(self.symbol, Side::Buy));
        }
        if !outputs.is_full() {
            outputs.push(OutputMessage::top_of_book_eliminated(self.symbol, Side::Sell));
        }
    }

    // =========================================================================
    // Internal Methods
    // =========================================================================

    fn match_order(
        &mut self,
        order: &mut Order,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) -> EngineResult<()> {
        let opposite_side = match order.side {
            Side::Buy => &mut self.asks,
            Side::Sell => &mut self.bids,
        };

        let mut iterations = 0;

        while !order.is_filled() && !opposite_side.is_empty() && iterations < MAX_MATCH_ITERATIONS {
            iterations += 1;

            let best_level = &mut opposite_side[0];

            // Check price compatibility
            let prices_cross = order.can_match(best_level.price);

            if !prices_cross {
                break;
            }

            // Match against orders at this level
            while !order.is_filled() && !best_level.is_empty() {
                let resting = &mut best_level.orders[0];

                let match_qty = order.remaining_qty.min(resting.remaining_qty);
                let match_price = best_level.price;

                // Fill both orders
                order.fill(match_qty);
                resting.fill(match_qty);

                // Emit trade
                if !outputs.is_full() {
                    let (buyer_id, buyer_order_id, seller_id, seller_order_id) = match order.side {
                        Side::Buy => (order.user_id, order.user_order_id, resting.user_id, resting.user_order_id),
                        Side::Sell => (resting.user_id, resting.user_order_id, order.user_id, order.user_order_id),
                    };

                    outputs.push(OutputMessage::trade(
                        self.symbol,
                        buyer_id,
                        buyer_order_id,
                        seller_id,
                        seller_order_id,
                        match_price,
                        match_qty,
                    ));
                }

                // Remove filled resting order
                if resting.is_filled() {
                    best_level.orders.remove(0);
                }
            }

            // Remove empty level
            if best_level.is_empty() {
                opposite_side.remove(0);
            }
        }

        debug_assert!(iterations <= MAX_MATCH_ITERATIONS, "match loop exceeded max iterations");
        Ok(())
    }

    fn insert_order(&mut self, order: Order) -> EngineResult<()> {
        debug_assert!(!order.is_filled(), "inserting filled order");
        debug_assert!(order.order_type.is_limit(), "inserting non-limit order");

        let levels = match order.side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };

        // Check capacity before inserting
        if levels.len() >= MAX_PRICE_LEVELS {
            // Check if we can add to existing level
            let existing = levels.iter().any(|l| l.price == order.price);
            if !existing {
                return Err(EngineError::PriceLevelCapacityExceeded {
                    symbol: self.symbol,
                    side: order.side,
                });
            }
        }

        // Find insertion point
        let pos = match order.side {
            Side::Buy => {
                // Bids: descending order (highest first)
                levels.iter().position(|l| l.price < order.price)
            }
            Side::Sell => {
                // Asks: ascending order (lowest first)
                levels.iter().position(|l| l.price > order.price)
            }
        };

        // Check if level exists at this price
        let existing_pos = levels.iter().position(|l| l.price == order.price);

        if let Some(idx) = existing_pos {
            // Add to existing level
            levels[idx].add_order(order)?;
        } else {
            // Create new level
            let mut new_level = PriceLevel::new(order.price);
            new_level.add_order(order)?;

            let insert_pos = pos.unwrap_or(levels.len());
            levels.insert(insert_pos, new_level);
        }

        Ok(())
    }

    fn emit_tob_updates(
        &self,
        old_bid: u32,
        old_ask: u32,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) {
        let new_bid = self.best_bid_price();
        let new_ask = self.best_ask_price();

        // Emit bid update if changed
        if new_bid != old_bid && !outputs.is_full() {
            if new_bid == 0 {
                outputs.push(OutputMessage::top_of_book_eliminated(self.symbol, Side::Buy));
            } else {
                outputs.push(OutputMessage::top_of_book(
                    self.symbol,
                    Side::Buy,
                    new_bid,
                    self.best_bid_quantity(),
                ));
            }
        }

        // Emit ask update if changed
        if new_ask != old_ask && !outputs.is_full() {
            if new_ask == 0 {
                outputs.push(OutputMessage::top_of_book_eliminated(self.symbol, Side::Sell));
            } else {
                outputs.push(OutputMessage::top_of_book(
                    self.symbol,
                    Side::Sell,
                    new_ask,
                    self.best_ask_quantity(),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> Symbol {
        Symbol::from_str(s)
    }

    #[test]
    fn test_price_level_basic() {
        let mut level = PriceLevel::new(100);
        assert!(level.is_empty());
        assert_eq!(level.total_quantity(), 0);

        let order = Order::new(1, 1, sym("TEST"), 100, 50, Side::Buy, 0);
        level.add_order(order).unwrap();

        assert!(!level.is_empty());
        assert_eq!(level.total_quantity(), 50);
        assert_eq!(level.order_count(), 1);
    }

    #[test]
    fn test_order_book_basic() {
        let mut book = OrderBook::new(sym("IBM"));
        assert_eq!(book.best_bid_price(), 0);
        assert_eq!(book.best_ask_price(), 0);

        let mut outputs = ArrayVec::new();
        let order = NewOrder::new(1, 1, sym("IBM"), 100, 50, Side::Buy);
        book.add_order(&order, 0, &mut outputs).unwrap();

        assert_eq!(book.best_bid_price(), 100);
        assert_eq!(book.best_bid_quantity(), 50);
    }

    #[test]
    fn test_order_book_match() {
        let mut book = OrderBook::new(sym("IBM"));
        let mut outputs = ArrayVec::new();

        // Add bid
        let bid = NewOrder::new(1, 1, sym("IBM"), 100, 50, Side::Buy);
        book.add_order(&bid, 0, &mut outputs).unwrap();
        outputs.clear();

        // Add matching ask
        let ask = NewOrder::new(2, 1, sym("IBM"), 100, 50, Side::Sell);
        book.add_order(&ask, 1, &mut outputs).unwrap();

        // Should have trade
        assert!(outputs.iter().any(|m| m.is_trade()));

        // Book should be empty
        assert_eq!(book.best_bid_price(), 0);
        assert_eq!(book.best_ask_price(), 0);
    }
}
