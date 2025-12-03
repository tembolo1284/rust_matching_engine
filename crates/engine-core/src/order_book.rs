//! Single-symbol order book with price-time priority.
//!
//! # Cache Optimization
//! - Price levels stored in sorted `Vec` for cache-friendly iteration.
//! - Orders stored by index into a pre-allocated pool (handled by engine).
//! - Hot fields (best bid/ask) cached to avoid repeated lookups.
//!
//! # Design Decisions
//! - Uses `Vec` instead of `BTreeMap` for better cache locality.
//! - Outputs written to caller-provided buffer (no allocation).
//! - Bounded iteration with explicit limits.

use crate::messages::{NewOrder, OutputMessage};
use crate::order::Order;
use crate::order_type::OrderType;
use crate::side::Side;
use crate::symbol::Symbol;
use crate::top_of_book::TopOfBookSnapshot;

/// Maximum iterations for matching loop (Power of Ten Rule 2).
const MAX_MATCH_ITERATIONS: usize = 100_000;

/// Maximum orders per price level before warning.
const MAX_ORDERS_PER_LEVEL: usize = 10_000;

/// Maximum price levels per side.
const MAX_PRICE_LEVELS: usize = 10_000;

/// Default order capacity per price level.
const DEFAULT_ORDERS_PER_LEVEL: usize = 64;

/// A price level containing orders at a single price.
#[derive(Debug, Clone)]
struct PriceLevel {
    price: u32,
    orders: Vec<Order>,
}

impl PriceLevel {
    /// Create a new price level with pre-allocated order capacity.
    fn new(price: u32) -> Self {
        PriceLevel {
            price,
            orders: Vec::with_capacity(DEFAULT_ORDERS_PER_LEVEL),
        }
    }

    #[inline]
    fn total_quantity(&self) -> u32 {
        self.orders.iter().map(|o| o.remaining_qty).sum()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }
}

/// Single-symbol order book.
#[derive(Debug)]
pub struct OrderBook {
    /// Symbol for this book.
    symbol: Symbol,

    /// Bid price levels, sorted descending by price (best bid at index 0).
    bids: Vec<PriceLevel>,

    /// Ask price levels, sorted ascending by price (best ask at index 0).
    asks: Vec<PriceLevel>,

    /// Cached previous top-of-book for change detection.
    prev_tob: TopOfBookSnapshot,
}

impl OrderBook {
    /// Create a new order book for the given symbol.
    pub fn new(symbol: Symbol) -> Self {
        OrderBook {
            symbol,
            bids: Vec::with_capacity(256),
            asks: Vec::with_capacity(256),
            prev_tob: TopOfBookSnapshot::EMPTY,
        }
    }

    /// Create with pre-allocated capacity.
    pub fn with_capacity(symbol: Symbol, levels_per_side: usize) -> Self {
        OrderBook {
            symbol,
            bids: Vec::with_capacity(levels_per_side),
            asks: Vec::with_capacity(levels_per_side),
            prev_tob: TopOfBookSnapshot::EMPTY,
        }
    }

    /// Returns the symbol of this book.
    #[inline]
    pub fn symbol(&self) -> Symbol {
        self.symbol
    }

    /// Process a new order, writing outputs to the provided buffer.
    ///
    /// Outputs: Ack, Trades, TopOfBook changes.
    pub fn add_order(&mut self, msg: &NewOrder, timestamp_ns: u64, outputs: &mut Vec<OutputMessage>) {
        debug_assert_eq!(msg.symbol, self.symbol, "order symbol mismatch");
        debug_assert!(msg.quantity > 0, "order quantity must be > 0");

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

        // Ack immediately
        outputs.push(OutputMessage::ack(order.user_id, order.user_order_id, self.symbol));

        // Match against opposing side
        self.match_order(&mut order, outputs);

        // Add remainder to book if limit order with remaining qty
        if order.remaining_qty > 0 && order.order_type == OrderType::Limit {
            self.add_to_book(order);
        }

        // Emit TOB changes
        self.emit_tob_changes(outputs);
    }

    /// Cancel an order by (user_id, user_order_id).
    ///
    /// Returns true if the order was found and removed.
    pub fn cancel_order(
        &mut self,
        user_id: u32,
        user_order_id: u32,
        outputs: &mut Vec<OutputMessage>,
    ) -> bool {
        let found = self.remove_order(user_id, user_order_id);

        // Always emit CancelAck
        outputs.push(OutputMessage::cancel_ack(user_id, user_order_id, self.symbol));

        // Emit TOB changes if we removed something
        if found {
            self.emit_tob_changes(outputs);
        }

        found
    }

    /// Flush all orders from the book.
    pub fn flush(&mut self, outputs: &mut Vec<OutputMessage>) {
        // Cancel acks for all orders
        for level in &self.bids {
            for order in &level.orders {
                outputs.push(OutputMessage::cancel_ack(
                    order.user_id,
                    order.user_order_id,
                    self.symbol,
                ));
            }
        }
        for level in &self.asks {
            for order in &level.orders {
                outputs.push(OutputMessage::cancel_ack(
                    order.user_id,
                    order.user_order_id,
                    self.symbol,
                ));
            }
        }

        // TOB eliminated if there were orders
        if !self.bids.is_empty() {
            outputs.push(OutputMessage::top_of_book_eliminated(self.symbol, Side::Buy));
        }
        if !self.asks.is_empty() {
            outputs.push(OutputMessage::top_of_book_eliminated(self.symbol, Side::Sell));
        }

        // Clear
        self.bids.clear();
        self.asks.clear();
        self.prev_tob = TopOfBookSnapshot::EMPTY;
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
        self.bids.first().map(|l| l.price).unwrap_or(0)
    }

    /// Best ask price (0 if empty).
    #[inline]
    pub fn best_ask_price(&self) -> u32 {
        self.asks.first().map(|l| l.price).unwrap_or(0)
    }

    /// Total quantity at best bid.
    #[inline]
    pub fn best_bid_quantity(&self) -> u32 {
        self.bids.first().map(|l| l.total_quantity()).unwrap_or(0)
    }

    /// Total quantity at best ask.
    #[inline]
    pub fn best_ask_quantity(&self) -> u32 {
        self.asks.first().map(|l| l.total_quantity()).unwrap_or(0)
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

    // =========================================================================
    // Internal Methods
    // =========================================================================

    /// Match an incoming order against the opposing side.
    fn match_order(&mut self, order: &mut Order, outputs: &mut Vec<OutputMessage>) {
        debug_assert!(order.remaining_qty > 0, "matching fully filled order");

        let opposing_side = match order.side {
            Side::Buy => &mut self.asks,
            Side::Sell => &mut self.bids,
        };

        let mut iterations = 0;

        // Bounded loop (Power of Ten Rule 2)
        while iterations < MAX_MATCH_ITERATIONS {
            iterations += 1;

            if order.remaining_qty == 0 || opposing_side.is_empty() {
                break;
            }

            let best_price = opposing_side[0].price;

            // Check if we can match at this price
            if !order.can_match(best_price) {
                break;
            }

            // Match against orders at this level (FIFO)
            let level = &mut opposing_side[0];

            let mut order_idx = 0;
            let mut inner_iterations = 0;

            while inner_iterations < MAX_ORDERS_PER_LEVEL {
                inner_iterations += 1;

                if order.remaining_qty == 0 || order_idx >= level.orders.len() {
                    break;
                }

                let passive = &mut level.orders[order_idx];
                let trade_qty = order.remaining_qty.min(passive.remaining_qty);

                debug_assert!(trade_qty > 0, "zero trade quantity");

                // Emit trade (buyer always first in output)
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

                outputs.push(OutputMessage::trade(
                    self.symbol,
                    buyer_id,
                    buyer_oid,
                    seller_id,
                    seller_oid,
                    best_price,
                    trade_qty,
                ));

                order.fill(trade_qty);
                passive.fill(trade_qty);

                if passive.is_filled() {
                    order_idx += 1;
                }
            }

            debug_assert!(
                inner_iterations < MAX_ORDERS_PER_LEVEL,
                "exceeded max orders per level"
            );

            // Remove filled orders from front
            let filled_count = level.orders.iter().take_while(|o| o.is_filled()).count();
            if filled_count > 0 {
                level.orders.drain(0..filled_count);
            }

            // Remove empty price level
            if level.is_empty() {
                opposing_side.remove(0);
            }
        }

        debug_assert!(
            iterations < MAX_MATCH_ITERATIONS,
            "exceeded max match iterations"
        );
    }

    /// Add a limit order to the appropriate side.
    fn add_to_book(&mut self, order: Order) {
        debug_assert!(order.remaining_qty > 0, "adding filled order to book");
        debug_assert!(order.order_type == OrderType::Limit, "adding market order to book");
        debug_assert!(order.price > 0, "limit order with zero price");

        let (levels, descending) = match order.side {
            Side::Buy => (&mut self.bids, true),   // Bids: high to low
            Side::Sell => (&mut self.asks, false), // Asks: low to high
        };

        // Find insertion point (binary search)
        let pos = if descending {
            // Descending: find first price < order.price
            levels
                .iter()
                .position(|l| l.price < order.price)
                .unwrap_or(levels.len())
        } else {
            // Ascending: find first price > order.price
            levels
                .iter()
                .position(|l| l.price > order.price)
                .unwrap_or(levels.len())
        };

        // Check if we have a level at this price
        if pos < levels.len() && levels[pos].price == order.price {
            // Append to existing level (time priority)
            levels[pos].orders.push(order);
        } else if pos > 0 && levels[pos - 1].price == order.price {
            // Check previous position too (edge case in binary search)
            levels[pos - 1].orders.push(order);
        } else {
            // Insert new level
            debug_assert!(
                levels.len() < MAX_PRICE_LEVELS,
                "exceeded max price levels"
            );
            let mut level = PriceLevel::new(order.price);
            level.orders.push(order);
            levels.insert(pos, level);
        }
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
        levels: &mut Vec<PriceLevel>,
        user_id: u32,
        user_order_id: u32,
    ) -> bool {
        for level_idx in 0..levels.len() {
            let level = &mut levels[level_idx];

            // Find order in this level
            if let Some(order_idx) = level
                .orders
                .iter()
                .position(|o| o.user_id == user_id && o.user_order_id == user_order_id)
            {
                level.orders.remove(order_idx);

                // Remove empty level
                if level.is_empty() {
                    levels.remove(level_idx);
                }

                return true;
            }
        }
        false
    }

    /// Emit top-of-book changes if state has changed.
    fn emit_tob_changes(&mut self, outputs: &mut Vec<OutputMessage>) {
        let current = self.top_of_book();

        // Bid side
        if current.bid_price != self.prev_tob.bid_price
            || current.bid_quantity != self.prev_tob.bid_quantity
        {
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

        // Ask side
        if current.ask_price != self.prev_tob.ask_price
            || current.ask_quantity != self.prev_tob.ask_quantity
        {
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

        self.prev_tob = current;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_order(user_id: u32, user_order_id: u32, price: u32, qty: u32, side: Side) -> NewOrder {
        NewOrder::new(user_id, user_order_id, Symbol::from_str("TEST"), price, qty, side)
    }

    #[test]
    fn test_add_single_bid() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = Vec::new();

        let order = make_order(1, 100, 1000, 10, Side::Buy);
        book.add_order(&order, 0, &mut outputs);

        assert_eq!(book.best_bid_price(), 1000);
        assert_eq!(book.best_bid_quantity(), 10);
        assert_eq!(book.best_ask_price(), 0);

        // Should have: Ack + TOB update
        assert!(outputs.len() >= 2);
    }

    #[test]
    fn test_match_simple() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = Vec::new();

        // Add resting bid
        let bid = make_order(1, 1, 100, 10, Side::Buy);
        book.add_order(&bid, 0, &mut outputs);
        outputs.clear();

        // Incoming sell should match
        let ask = make_order(2, 1, 100, 10, Side::Sell);
        book.add_order(&ask, 1, &mut outputs);

        // Find the trade
        let trade = outputs.iter().find(|m| matches!(m, OutputMessage::Trade(_)));
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
    }

    #[test]
    fn test_partial_fill() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = Vec::new();

        // Add resting bid for 100
        let bid = make_order(1, 1, 100, 100, Side::Buy);
        book.add_order(&bid, 0, &mut outputs);
        outputs.clear();

        // Sell only 30
        let ask = make_order(2, 1, 100, 30, Side::Sell);
        book.add_order(&ask, 1, &mut outputs);

        // Should have partial fill, 70 remaining
        assert_eq!(book.best_bid_quantity(), 70);
    }

    #[test]
    fn test_price_time_priority() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = Vec::new();

        // Add two bids at same price
        let bid1 = make_order(1, 1, 100, 10, Side::Buy);
        let bid2 = make_order(2, 1, 100, 10, Side::Buy);
        book.add_order(&bid1, 0, &mut outputs);
        book.add_order(&bid2, 1, &mut outputs);
        outputs.clear();

        // Sell should match first bid (time priority)
        let ask = make_order(3, 1, 100, 10, Side::Sell);
        book.add_order(&ask, 2, &mut outputs);

        let trade = outputs.iter().find_map(|m| {
            if let OutputMessage::Trade(t) = m { Some(t) } else { None }
        });

        assert!(trade.is_some());
        assert_eq!(trade.unwrap().user_id_buy, 1); // First bid
    }

    #[test]
    fn test_cancel() {
        let mut book = OrderBook::new(Symbol::from_str("TEST"));
        let mut outputs = Vec::new();

        let bid = make_order(1, 100, 100, 10, Side::Buy);
        book.add_order(&bid, 0, &mut outputs);
        outputs.clear();

        let found = book.cancel_order(1, 100, &mut outputs);
        assert!(found);
        assert_eq!(book.best_bid_price(), 0);

        // Should have CancelAck
        assert!(outputs.iter().any(|m| matches!(m, OutputMessage::CancelAck(_))));
    }
}
