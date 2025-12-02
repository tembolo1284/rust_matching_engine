//! Multi-symbol matching engine.
//!
//! # Architecture
//! - Pre-registered symbols with pre-created order books.
//! - Order-to-symbol tracking via fast hash map.
//! - Output buffer passed by caller (no allocation).

use std::collections::HashMap;

use crate::messages::{Cancel, InputMessage, NewOrder, OutputMessage, TopOfBookQuery};
use crate::order_book::OrderBook;
use crate::side::Side;
use crate::symbol::Symbol;
use crate::top_of_book::TopOfBookSnapshot;

/// Configuration for engine initialization.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Expected number of symbols.
    pub num_symbols: usize,
    /// Expected max orders (for order-to-symbol map capacity).
    pub max_orders: usize,
    /// Price levels per side per book.
    pub levels_per_side: usize,
    /// Pre-allocate output buffer capacity.
    pub output_buffer_capacity: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            num_symbols: 1024,
            max_orders: 1_000_000,
            levels_per_side: 256,
            output_buffer_capacity: 1024,
        }
    }
}

/// Symbol used when cancel target is unknown.
const UNKNOWN_SYMBOL: Symbol = Symbol([b'<', b'U', b'N', b'K', b'>', 0, 0, 0]);

/// Multi-symbol matching engine.
#[derive(Debug)]
pub struct MatchingEngine {
    /// Symbol -> OrderBook.
    /// Using standard HashMap; for ultra-low latency, consider FxHashMap or a
    /// symbol-index lookup table.
    order_books: HashMap<Symbol, OrderBook>,

    /// (user_id, user_order_id) -> Symbol for cancel routing.
    order_to_symbol: HashMap<(u32, u32), Symbol>,

    /// Configuration.
    config: EngineConfig,

    /// Timestamp counter for ordering (can be replaced with external clock).
    timestamp_counter: u64,
}

impl MatchingEngine {
    /// Create a new engine with default configuration.
    pub fn new() -> Self {
        Self::with_config(EngineConfig::default())
    }

    /// Create a new engine with custom configuration.
    pub fn with_config(config: EngineConfig) -> Self {
        MatchingEngine {
            order_books: HashMap::with_capacity(config.num_symbols),
            order_to_symbol: HashMap::with_capacity(config.max_orders),
            config,
            timestamp_counter: 0,
        }
    }

    /// Pre-register a symbol (creates order book upfront).
    ///
    /// Call this at startup for all known symbols to avoid allocation
    /// during trading.
    pub fn register_symbol(&mut self, symbol: Symbol) {
        if !self.order_books.contains_key(&symbol) {
            let book = OrderBook::with_capacity(symbol, self.config.levels_per_side);
            self.order_books.insert(symbol, book);
        }
    }

    /// Pre-register multiple symbols.
    pub fn register_symbols(&mut self, symbols: impl IntoIterator<Item = Symbol>) {
        for sym in symbols {
            self.register_symbol(sym);
        }
    }

    /// Process a single input message, writing outputs to the provided buffer.
    ///
    /// # Performance
    /// Caller should reuse the output buffer across calls to avoid allocation.
    pub fn process_message(&mut self, msg: InputMessage, outputs: &mut Vec<OutputMessage>) {
        match msg {
            InputMessage::NewOrder(new_order) => {
                self.process_new_order(&new_order, outputs);
            }
            InputMessage::Cancel(cancel) => {
                self.process_cancel(cancel, outputs);
            }
            InputMessage::Flush => {
                self.process_flush(outputs);
            }
            InputMessage::QueryTopOfBook(query) => {
                self.process_query_top_of_book(query, outputs);
            }
        }
    }

    /// Convenience: process message and return new Vec.
    /// 
    /// Prefer `process_message` with reusable buffer for hot path.
    pub fn process(&mut self, msg: InputMessage) -> Vec<OutputMessage> {
        let mut outputs = Vec::with_capacity(self.config.output_buffer_capacity);
        self.process_message(msg, &mut outputs);
        outputs
    }

    /// Get a reference to an order book by symbol.
    pub fn get_book(&self, symbol: Symbol) -> Option<&OrderBook> {
        self.order_books.get(&symbol)
    }

    /// Get mutable reference to an order book.
    pub fn get_book_mut(&mut self, symbol: Symbol) -> Option<&mut OrderBook> {
        self.order_books.get_mut(&symbol)
    }

    /// Get top-of-book snapshot for a symbol.
    pub fn top_of_book(&self, symbol: Symbol) -> TopOfBookSnapshot {
        self.order_books
            .get(&symbol)
            .map(|b| b.top_of_book())
            .unwrap_or(TopOfBookSnapshot::EMPTY)
    }

    /// Number of registered symbols.
    pub fn num_symbols(&self) -> usize {
        self.order_books.len()
    }

    /// Number of tracked orders.
    pub fn num_orders(&self) -> usize {
        self.order_to_symbol.len()
    }

    /// Set the timestamp counter (for deterministic testing).
    pub fn set_timestamp(&mut self, ts: u64) {
        self.timestamp_counter = ts;
    }

    // =========================================================================
    // Internal Handlers
    // =========================================================================

    fn process_new_order(&mut self, msg: &NewOrder, outputs: &mut Vec<OutputMessage>) {
        debug_assert!(msg.quantity > 0, "new order with zero quantity");

        let symbol = msg.symbol;
        let key = (msg.user_id, msg.user_order_id);

        // Get or create order book
        // Note: In strict Power of Ten mode, we'd reject unknown symbols.
        // Here we auto-create for flexibility.
        let book = self
            .order_books
            .entry(symbol)
            .or_insert_with(|| OrderBook::with_capacity(symbol, self.config.levels_per_side));

        // Generate timestamp
        let timestamp = self.next_timestamp();

        // Process the order
        book.add_order(msg, timestamp, outputs);

        // Track order -> symbol mapping for cancels
        // Only track if it might rest in the book (limit orders)
        if msg.price > 0 {
            self.order_to_symbol.insert(key, symbol);
        }
    }

    fn process_cancel(&mut self, msg: Cancel, outputs: &mut Vec<OutputMessage>) {
        let key = (msg.user_id, msg.user_order_id);

        // Find which symbol this order belongs to
        let symbol_opt = self.order_to_symbol.get(&key).copied();

        match symbol_opt {
            Some(symbol) => {
                // Route to the correct book
                if let Some(book) = self.order_books.get_mut(&symbol) {
                    book.cancel_order(msg.user_id, msg.user_order_id, outputs);
                } else {
                    // Book doesn't exist (shouldn't happen)
                    outputs.push(OutputMessage::cancel_ack(
                        msg.user_id,
                        msg.user_order_id,
                        symbol,
                    ));
                }

                // Remove from tracking
                self.order_to_symbol.remove(&key);
            }
            None => {
                // Order not found - still emit CancelAck
                outputs.push(OutputMessage::cancel_ack(
                    msg.user_id,
                    msg.user_order_id,
                    UNKNOWN_SYMBOL,
                ));
            }
        }
    }

    fn process_flush(&mut self, outputs: &mut Vec<OutputMessage>) {
        // Flush each order book
        for (_symbol, book) in self.order_books.iter_mut() {
            book.flush(outputs);
        }

        // Clear tracking
        self.order_to_symbol.clear();

        // Note: We keep order_books (don't clear) so symbols stay registered.
        // The individual books are now empty.
    }

    fn process_query_top_of_book(&self, query: TopOfBookQuery, outputs: &mut Vec<OutputMessage>) {
        let symbol = query.symbol;

        let (bid_price, bid_qty, ask_price, ask_qty) =
            if let Some(book) = self.order_books.get(&symbol) {
                (
                    book.best_bid_price(),
                    book.best_bid_quantity(),
                    book.best_ask_price(),
                    book.best_ask_quantity(),
                )
            } else {
                (0, 0, 0, 0)
            };

        // Emit bid side
        if bid_price == 0 {
            outputs.push(OutputMessage::top_of_book_eliminated(symbol, Side::Buy));
        } else {
            outputs.push(OutputMessage::top_of_book(symbol, Side::Buy, bid_price, bid_qty));
        }

        // Emit ask side
        if ask_price == 0 {
            outputs.push(OutputMessage::top_of_book_eliminated(symbol, Side::Sell));
        } else {
            outputs.push(OutputMessage::top_of_book(symbol, Side::Sell, ask_price, ask_qty));
        }
    }

    fn next_timestamp(&mut self) -> u64 {
        let ts = self.timestamp_counter;
        self.timestamp_counter += 1;
        ts
    }
}

impl Default for MatchingEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> Symbol {
        Symbol::from_str(s)
    }

    fn new_order(user_id: u32, order_id: u32, symbol: &str, price: u32, qty: u32, side: Side) -> InputMessage {
        InputMessage::NewOrder(NewOrder::new(
            user_id,
            order_id,
            sym(symbol),
            price,
            qty,
            side,
        ))
    }

    fn cancel(user_id: u32, order_id: u32) -> InputMessage {
        InputMessage::Cancel(Cancel::new(user_id, order_id))
    }

    #[test]
    fn test_simple_order() {
        let mut engine = MatchingEngine::new();
        let outputs = engine.process(new_order(1, 1, "IBM", 100, 10, Side::Buy));

        // Should have Ack + TOB update
        assert!(outputs.iter().any(|m| matches!(m, OutputMessage::Ack(_))));
        assert_eq!(engine.top_of_book(sym("IBM")).bid_price, 100);
    }

    #[test]
    fn test_match() {
        let mut engine = MatchingEngine::new();

        // Add bid
        engine.process(new_order(1, 1, "IBM", 100, 10, Side::Buy));

        // Add matching ask
        let outputs = engine.process(new_order(2, 1, "IBM", 100, 10, Side::Sell));

        // Should have trade
        let trade = outputs.iter().find(|m| matches!(m, OutputMessage::Trade(_)));
        assert!(trade.is_some());

        // Book should be empty
        assert_eq!(engine.top_of_book(sym("IBM")).bid_price, 0);
        assert_eq!(engine.top_of_book(sym("IBM")).ask_price, 0);
    }

    #[test]
    fn test_cancel() {
        let mut engine = MatchingEngine::new();

        engine.process(new_order(1, 1, "IBM", 100, 10, Side::Buy));
        assert_eq!(engine.top_of_book(sym("IBM")).bid_price, 100);

        let outputs = engine.process(cancel(1, 1));
        assert!(outputs.iter().any(|m| matches!(m, OutputMessage::CancelAck(_))));
        assert_eq!(engine.top_of_book(sym("IBM")).bid_price, 0);
    }

    #[test]
    fn test_multi_symbol() {
        let mut engine = MatchingEngine::new();

        engine.process(new_order(1, 1, "IBM", 100, 10, Side::Buy));
        engine.process(new_order(2, 1, "AAPL", 200, 20, Side::Buy));

        assert_eq!(engine.top_of_book(sym("IBM")).bid_price, 100);
        assert_eq!(engine.top_of_book(sym("AAPL")).bid_price, 200);
        assert_eq!(engine.num_symbols(), 2);
    }

    #[test]
    fn test_pre_registration() {
        let mut engine = MatchingEngine::new();
        engine.register_symbols([sym("IBM"), sym("AAPL"), sym("GOOG")].into_iter());

        assert_eq!(engine.num_symbols(), 3);
        
        // Books exist but are empty
        assert_eq!(engine.top_of_book(sym("IBM")).bid_price, 0);
    }

    #[test]
    fn test_reuse_output_buffer() {
        let mut engine = MatchingEngine::new();
        let mut outputs = Vec::with_capacity(64);

        // Reuse buffer across multiple calls
        engine.process_message(new_order(1, 1, "IBM", 100, 10, Side::Buy), &mut outputs);
        assert!(!outputs.is_empty());

        outputs.clear();
        engine.process_message(new_order(2, 1, "IBM", 100, 10, Side::Sell), &mut outputs);
        
        // Should have trade in same buffer
        assert!(outputs.iter().any(|m| matches!(m, OutputMessage::Trade(_))));
    }
}
