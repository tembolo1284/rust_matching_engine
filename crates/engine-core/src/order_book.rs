//! Multi-symbol matching engine with strict Power of Ten compliance.
//!
//! # Architecture
//! - Pre-registered symbols with pre-created order books.
//! - Fixed-capacity order tracking (no allocation after init).
//! - Unknown symbols are rejected (strict mode).
//! - All operations return `Result` for explicit error handling.
//!
//! # Power of Ten Compliance
//! - Rule 2: All loops bounded.
//! - Rule 3: No dynamic allocation after initialization.
//! - Rule 5: Minimum 2 assertions per function.
//! - Rule 7: All return values checked.
use rustc_hash::FxHashMap;
use crate::error::{EngineError, EngineResult};
use crate::messages::{Cancel, InputMessage, NewOrder, OutputMessage, RejectReason, TopOfBookQuery};
use crate::order_book::{OrderBook, MAX_OUTPUTS_PER_ORDER};
use crate::side::Side;
use crate::symbol::Symbol;
use crate::top_of_book::TopOfBookSnapshot;
use arrayvec::ArrayVec;

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for engine initialization.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Maximum number of symbols.
    pub max_symbols: usize,
    /// Maximum tracked orders (for order-to-symbol map).
    pub max_orders: usize,
    /// Price levels per side per book.
    pub levels_per_side: usize,
    /// Enable strict mode (reject unknown symbols).
    pub strict_mode: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            max_symbols: 1024,
            max_orders: 1_000_000,
            levels_per_side: 256,
            strict_mode: true, // Power of Ten compliant by default
        }
    }
}

impl EngineConfig {
    /// Create a config for testing (smaller capacities).
    pub fn for_testing() -> Self {
        EngineConfig {
            max_symbols: 64,
            max_orders: 10_000,
            levels_per_side: 64,
            strict_mode: true,
        }
    }

    /// Create a lenient config (auto-creates symbols).
    pub fn lenient() -> Self {
        EngineConfig {
            strict_mode: false,
            ..Default::default()
        }
    }
}

// =============================================================================
// Symbol Constants
// =============================================================================

/// Symbol used when cancel target is unknown.
pub const UNKNOWN_SYMBOL: Symbol = Symbol([b'<', b'U', b'N', b'K', b'>', 0, 0, 0]);

// =============================================================================
// Matching Engine
// =============================================================================

/// Multi-symbol matching engine.
///
/// # Memory Model
/// All memory is pre-allocated at construction:
/// - Order books: `FxHashMap<Symbol, OrderBook>` with reserved capacity.
/// - Order tracking: `FxHashMap<(u32, u32), Symbol>` with reserved capacity.
///
/// After `new()` or `with_config()`, no further heap allocation occurs
/// during normal operation (assuming capacities are not exceeded).
#[derive(Debug)]
pub struct MatchingEngine {
    /// Symbol -> OrderBook.
    /// Using FxHashMap for faster hashing (non-cryptographic).
    order_books: FxHashMap<Symbol, OrderBook>,
    /// (user_id, user_order_id) -> Symbol for cancel routing.
    order_to_symbol: FxHashMap<(u32, u32), Symbol>,
    /// Configuration.
    config: EngineConfig,
    /// Timestamp counter for ordering (can be replaced with external clock).
    timestamp_counter: u64,
}

// Compile-time verification
const _: () = assert!(
    std::mem::size_of::<MatchingEngine>() <= 256,
    "MatchingEngine should be reasonably sized"
);

impl MatchingEngine {
    /// Create a new engine with default configuration.
    pub fn new() -> Self {
        Self::with_config(EngineConfig::default())
    }

    /// Create a new engine with custom configuration.
    ///
    /// All memory is pre-allocated based on config values.
    pub fn with_config(config: EngineConfig) -> Self {
        debug_assert!(config.max_symbols > 0, "max_symbols must be > 0");
        debug_assert!(config.max_orders > 0, "max_orders must be > 0");
        debug_assert!(config.levels_per_side > 0, "levels_per_side must be > 0");

        let mut order_books = FxHashMap::default();
        order_books.reserve(config.max_symbols);

        let mut order_to_symbol = FxHashMap::default();
        order_to_symbol.reserve(config.max_orders);

        let engine = MatchingEngine {
            order_books,
            order_to_symbol,
            config,
            timestamp_counter: 0,
        };

        debug_assert!(engine.order_books.capacity() >= engine.config.max_symbols);
        debug_assert!(engine.order_to_symbol.capacity() >= engine.config.max_orders);

        engine
    }

    /// Get the configuration.
    #[inline]
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    // =========================================================================
    // Symbol Registration
    // =========================================================================

    /// Pre-register a symbol (creates order book upfront).
    ///
    /// Call this at startup for all known symbols to avoid allocation
    /// during trading.
    ///
    /// # Returns
    /// - `Ok(true)` if symbol was newly registered.
    /// - `Ok(false)` if symbol was already registered.
    /// - `Err` if max symbols exceeded.
    pub fn register_symbol(&mut self, symbol: Symbol) -> EngineResult<bool> {
        debug_assert!(!symbol.is_empty(), "cannot register empty symbol");

        if self.order_books.contains_key(&symbol) {
            debug_assert!(self.order_books.len() <= self.config.max_symbols);
            return Ok(false);
        }

        if self.order_books.len() >= self.config.max_symbols {
            return Err(EngineError::MaxSymbolsExceeded {
                current: self.order_books.len(),
                max: self.config.max_symbols,
            });
        }

        let book = OrderBook::with_capacity(symbol, self.config.levels_per_side);
        self.order_books.insert(symbol, book);

        debug_assert!(self.order_books.contains_key(&symbol));
        Ok(true)
    }

    /// Pre-register multiple symbols.
    ///
    /// # Returns
    /// Number of newly registered symbols.
    pub fn register_symbols(&mut self, symbols: impl IntoIterator<Item = Symbol>) -> EngineResult<usize> {
        let mut count = 0;
        for sym in symbols {
            if self.register_symbol(sym)? {
                count += 1;
            }
        }
        debug_assert!(count <= self.config.max_symbols);
        Ok(count)
    }

    /// Check if a symbol is registered.
    #[inline]
    pub fn is_registered(&self, symbol: Symbol) -> bool {
        self.order_books.contains_key(&symbol)
    }

    // =========================================================================
    // Message Processing
    // =========================================================================

    /// Process a single input message, writing outputs to the provided buffer.
    ///
    /// # Performance
    /// Uses fixed-size `ArrayVec` for zero-allocation output handling.
    ///
    /// # Returns
    /// - `Ok(())` on success.
    /// - `Err(EngineError)` on capacity exceeded or invalid input.
    pub fn process_message(
        &mut self,
        msg: InputMessage,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) -> EngineResult<()> {
        debug_assert!(
            outputs.remaining_capacity() > 0,
            "output buffer must have space"
        );

        match msg {
            InputMessage::NewOrder(new_order) => {
                self.process_new_order(&new_order, outputs)
            }
            InputMessage::Cancel(cancel) => {
                self.process_cancel(cancel, outputs);
                Ok(())
            }
            InputMessage::Flush => {
                self.process_flush(outputs);
                Ok(())
            }
            InputMessage::QueryTopOfBook(query) => {
                self.process_query_top_of_book(query, outputs);
                Ok(())
            }
        }
    }

    /// Convenience: process message and return new ArrayVec.
    ///
    /// Prefer `process_message` with reusable buffer for hot path.
    pub fn process(&mut self, msg: InputMessage) -> EngineResult<ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>> {
        let mut outputs = ArrayVec::new();
        self.process_message(msg, &mut outputs)?;
        Ok(outputs)
    }

    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get a reference to an order book by symbol.
    #[inline]
    pub fn get_book(&self, symbol: Symbol) -> Option<&OrderBook> {
        self.order_books.get(&symbol)
    }

    /// Get mutable reference to an order book.
    #[inline]
    pub fn get_book_mut(&mut self, symbol: Symbol) -> Option<&mut OrderBook> {
        self.order_books.get_mut(&symbol)
    }

    /// Get top-of-book snapshot for a symbol.
    #[inline]
    pub fn top_of_book(&self, symbol: Symbol) -> TopOfBookSnapshot {
        self.order_books
            .get(&symbol)
            .map(|b| b.top_of_book())
            .unwrap_or(TopOfBookSnapshot::EMPTY)
    }

    /// Number of registered symbols.
    #[inline]
    pub fn num_symbols(&self) -> usize {
        self.order_books.len()
    }

    /// Number of tracked orders.
    #[inline]
    pub fn num_orders(&self) -> usize {
        self.order_to_symbol.len()
    }

    /// Remaining order capacity.
    #[inline]
    pub fn remaining_order_capacity(&self) -> usize {
        self.config.max_orders.saturating_sub(self.order_to_symbol.len())
    }

    /// Set the timestamp counter (for deterministic testing).
    #[inline]
    pub fn set_timestamp(&mut self, ts: u64) {
        self.timestamp_counter = ts;
    }

    /// Get current timestamp.
    #[inline]
    pub fn current_timestamp(&self) -> u64 {
        self.timestamp_counter
    }

    // =========================================================================
    // Internal Handlers
    // =========================================================================

    fn process_new_order(
        &mut self,
        msg: &NewOrder,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) -> EngineResult<()> {
        debug_assert!(msg.quantity > 0, "new order with zero quantity");
        debug_assert!(!msg.symbol.is_empty(), "new order with empty symbol");

        let symbol = msg.symbol;
        let key = msg.key();

        // Check for duplicate order ID
        if self.order_to_symbol.contains_key(&key) {
            if !outputs.is_full() {
                outputs.push(OutputMessage::reject(
                    msg.user_id,
                    msg.user_order_id,
                    symbol,
                    RejectReason::DuplicateOrderId,
                ));
            }
            return Err(EngineError::DuplicateOrderId {
                user_id: msg.user_id,
                user_order_id: msg.user_order_id,
            });
        }

        // Check order tracking capacity (only for limit orders that may rest)
        if msg.is_limit() && self.order_to_symbol.len() >= self.config.max_orders {
            if !outputs.is_full() {
                outputs.push(OutputMessage::reject(
                    msg.user_id,
                    msg.user_order_id,
                    symbol,
                    RejectReason::CapacityExceeded,
                ));
            }
            return Err(EngineError::OrderCapacityExceeded {
                current: self.order_to_symbol.len(),
                max: self.config.max_orders,
            });
        }

        // Generate timestamp BEFORE borrowing the book to avoid borrow conflict
        let timestamp = self.next_timestamp();

        // Get order book (strict mode: reject unknown symbols)
        if self.config.strict_mode {
            if !self.order_books.contains_key(&symbol) {
                if !outputs.is_full() {
                    outputs.push(OutputMessage::reject(
                        msg.user_id,
                        msg.user_order_id,
                        symbol,
                        RejectReason::UnknownSymbol,
                    ));
                }
                return Err(EngineError::UnknownSymbol(symbol));
            }
        } else {
            // Lenient mode: auto-create book
            if !self.order_books.contains_key(&symbol) {
                if self.order_books.len() >= self.config.max_symbols {
                    if !outputs.is_full() {
                        outputs.push(OutputMessage::reject(
                            msg.user_id,
                            msg.user_order_id,
                            symbol,
                            RejectReason::CapacityExceeded,
                        ));
                    }
                    return Err(EngineError::MaxSymbolsExceeded {
                        current: self.order_books.len(),
                        max: self.config.max_symbols,
                    });
                }
                let new_book = OrderBook::with_capacity(symbol, self.config.levels_per_side);
                self.order_books.insert(symbol, new_book);
            }
        }

        // Now get mutable reference to the book
        let book = self.order_books.get_mut(&symbol).unwrap();

        // Process the order
        book.add_order(msg, timestamp, outputs)?;

        // Track order -> symbol mapping for cancels (limit orders only)
        if msg.is_limit() {
            self.order_to_symbol.insert(key, symbol);
        }

        Ok(())
    }

    fn process_cancel(
        &mut self,
        msg: Cancel,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) {
        debug_assert!(outputs.remaining_capacity() >= 3, "need space for cancel outputs");

        let key = msg.key();

        // Find which symbol this order belongs to
        let symbol_opt = self.order_to_symbol.get(&key).copied();

        match symbol_opt {
            Some(symbol) => {
                // Route to the correct book
                if let Some(book) = self.order_books.get_mut(&symbol) {
                    book.cancel_order(msg.user_id, msg.user_order_id, outputs);
                } else {
                    // Book doesn't exist (shouldn't happen)
                    if !outputs.is_full() {
                        outputs.push(OutputMessage::cancel_ack(
                            msg.user_id,
                            msg.user_order_id,
                            symbol,
                        ));
                    }
                }
                // Remove from tracking
                self.order_to_symbol.remove(&key);
            }
            None => {
                // Order not found - still emit CancelAck
                if !outputs.is_full() {
                    outputs.push(OutputMessage::cancel_ack(
                        msg.user_id,
                        msg.user_order_id,
                        UNKNOWN_SYMBOL,
                    ));
                }
            }
        }
    }

    fn process_flush(&mut self, outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>) {
        // Flush each order book
        for book in self.order_books.values_mut() {
            book.flush(outputs);
        }

        // Clear tracking
        self.order_to_symbol.clear();

        debug_assert_eq!(self.order_to_symbol.len(), 0, "tracking must be cleared");
    }

    fn process_query_top_of_book(
        &self,
        query: TopOfBookQuery,
        outputs: &mut ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER>,
    ) {
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
        if !outputs.is_full() {
            if bid_price == 0 {
                outputs.push(OutputMessage::top_of_book_eliminated(symbol, Side::Buy));
            } else {
                outputs.push(OutputMessage::top_of_book(symbol, Side::Buy, bid_price, bid_qty));
            }
        }

        // Emit ask side
        if !outputs.is_full() {
            if ask_price == 0 {
                outputs.push(OutputMessage::top_of_book_eliminated(symbol, Side::Sell));
            } else {
                outputs.push(OutputMessage::top_of_book(symbol, Side::Sell, ask_price, ask_qty));
            }
        }
    }

    #[inline]
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

    fn new_outputs() -> ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER> {
        ArrayVec::new()
    }

    #[test]
    fn test_strict_mode_rejects_unknown_symbol() {
        let mut engine = MatchingEngine::with_config(EngineConfig::for_testing());
        let mut outputs = new_outputs();

        // Don't register IBM
        let result = engine.process_message(new_order(1, 1, "IBM", 100, 10, Side::Buy), &mut outputs);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EngineError::UnknownSymbol(_)));
        assert!(outputs.iter().any(|m| m.is_reject()));
    }

    #[test]
    fn test_registered_symbol_works() {
        let mut engine = MatchingEngine::with_config(EngineConfig::for_testing());
        engine.register_symbol(sym("IBM")).unwrap();

        let mut outputs = new_outputs();
        let result = engine.process_message(new_order(1, 1, "IBM", 100, 10, Side::Buy), &mut outputs);

        assert!(result.is_ok());
        assert_eq!(engine.top_of_book(sym("IBM")).bid_price, 100);
    }

    #[test]
    fn test_lenient_mode_auto_creates() {
        let mut engine = MatchingEngine::with_config(EngineConfig::lenient());
        let mut outputs = new_outputs();

        // Should auto-create IBM
        let result = engine.process_message(new_order(1, 1, "IBM", 100, 10, Side::Buy), &mut outputs);

        assert!(result.is_ok());
        assert!(engine.is_registered(sym("IBM")));
    }

    #[test]
    fn test_match() {
        let mut engine = MatchingEngine::with_config(EngineConfig::for_testing());
        engine.register_symbol(sym("IBM")).unwrap();

        // Add bid
        engine.process(new_order(1, 1, "IBM", 100, 10, Side::Buy)).unwrap();

        // Add matching ask
        let outputs = engine.process(new_order(2, 1, "IBM", 100, 10, Side::Sell)).unwrap();

        // Should have trade
        let trade = outputs.iter().find(|m| m.is_trade());
        assert!(trade.is_some());

        // Book should be empty
        assert_eq!(engine.top_of_book(sym("IBM")).bid_price, 0);
        assert_eq!(engine.top_of_book(sym("IBM")).ask_price, 0);
    }

    #[test]
    fn test_cancel() {
        let mut engine = MatchingEngine::with_config(EngineConfig::for_testing());
        engine.register_symbol(sym("IBM")).unwrap();

        engine.process(new_order(1, 1, "IBM", 100, 10, Side::Buy)).unwrap();
        assert_eq!(engine.top_of_book(sym("IBM")).bid_price, 100);
        assert_eq!(engine.num_orders(), 1);

        let outputs = engine.process(cancel(1, 1)).unwrap();

        assert!(outputs.iter().any(|m| matches!(m, OutputMessage::CancelAck(_))));
        assert_eq!(engine.top_of_book(sym("IBM")).bid_price, 0);
        assert_eq!(engine.num_orders(), 0);
    }

    #[test]
    fn test_multi_symbol() {
        let mut engine = MatchingEngine::with_config(EngineConfig::for_testing());
        engine.register_symbols([sym("IBM"), sym("AAPL")].into_iter()).unwrap();

        engine.process(new_order(1, 1, "IBM", 100, 10, Side::Buy)).unwrap();
        engine.process(new_order(2, 1, "AAPL", 200, 20, Side::Buy)).unwrap();

        assert_eq!(engine.top_of_book(sym("IBM")).bid_price, 100);
        assert_eq!(engine.top_of_book(sym("AAPL")).bid_price, 200);
    }

    #[test]
    fn test_duplicate_order_id_rejected() {
        let mut engine = MatchingEngine::with_config(EngineConfig::for_testing());
        engine.register_symbol(sym("IBM")).unwrap();

        engine.process(new_order(1, 1, "IBM", 100, 10, Side::Buy)).unwrap();

        // Same user_id and order_id should be rejected
        let result = engine.process(new_order(1, 1, "IBM", 101, 20, Side::Buy));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EngineError::DuplicateOrderId { .. }));
    }

    #[test]
    fn test_order_capacity() {
        let mut config = EngineConfig::for_testing();
        config.max_orders = 2;
        let mut engine = MatchingEngine::with_config(config);
        engine.register_symbol(sym("IBM")).unwrap();

        engine.process(new_order(1, 1, "IBM", 100, 10, Side::Buy)).unwrap();
        engine.process(new_order(2, 1, "IBM", 101, 10, Side::Buy)).unwrap();

        // Third order should fail
        let result = engine.process(new_order(3, 1, "IBM", 102, 10, Side::Buy));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EngineError::OrderCapacityExceeded { .. }));
    }

    #[test]
    fn test_market_orders_not_tracked() {
        let mut engine = MatchingEngine::with_config(EngineConfig::for_testing());
        engine.register_symbol(sym("IBM")).unwrap();

        // Add liquidity
        engine.process(new_order(1, 1, "IBM", 100, 10, Side::Sell)).unwrap();
        assert_eq!(engine.num_orders(), 1);

        // Market order (price = 0) matches and doesn't add to tracking
        engine.process(new_order(2, 1, "IBM", 0, 10, Side::Buy)).unwrap();

        // Only the partially filled resting order should remain (if any)
        // In this case, fully matched, so 0 orders
        assert_eq!(engine.num_orders(), 0);
    }
}
