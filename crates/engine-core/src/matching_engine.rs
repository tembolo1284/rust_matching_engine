//! Multi-symbol matching engine orchestrator.
//!
//! This is the Rust analogue of your C++ `MatchingEngine`:
//! - Maintains one [`OrderBook`] per symbol.
//! - Creates order books on-demand on first use.
//! - Routes input messages to the appropriate book.
//! - Tracks `(user_id, user_order_id) -> symbol` for cancels.
//!
//! Differences / extensions vs C++:
//! - Supports `InputMessage::QueryTopOfBook` to snapshot current TOB
//!   for a given symbol.
//! - All outputs are symbol-aware (the book injects `symbol`).

use std::collections::HashMap;

use crate::messages::{
    Ack,
    Cancel,
    CancelAck,
    InputMessage,
    NewOrder,
    OutputMessage,
    TopOfBook,
    TopOfBookQuery,
};
use crate::order_book::OrderBook;
use crate::side::Side;

/// Multi-symbol matching engine.
///
/// Owns a set of `OrderBook`s, one per symbol, and a cross-book
/// mapping from `(user_id, user_order_id)` to symbol for cancel
/// routing (mirroring your C++ `order_to_symbol_` map).
#[derive(Debug, Default)]
pub struct MatchingEngine {
    /// Symbol -> OrderBook.
    order_books: HashMap<String, OrderBook>,

    /// Tracks which symbol an order belongs to, keyed by `(user_id, user_order_id)`.
    ///
    /// This mirrors your C++:
    /// ```cpp
    /// std::unordered_map<uint64_t, std::string> order_to_symbol_;
    /// ```
    /// but we use a `(u32, u32)` tuple rather than a packed u64.
    order_to_symbol: HashMap<(u32, u32), String>,
}

impl MatchingEngine {
    /// Create a new, empty matching engine.
    pub fn new() -> Self {
        MatchingEngine::default()
    }

    /// Process a single input message and return any output events.
    ///
    /// This combines the behavior of your C++ `processMessage`,
    /// `processNewOrder`, `processCancelOrder`, and `processFlush`,
    /// plus the new `QueryTopOfBook` support.
    pub fn process_message(&mut self, msg: InputMessage) -> Vec<OutputMessage> {
        match msg {
            InputMessage::NewOrder(new) => self.process_new_order(new),
            InputMessage::Cancel(cancel) => self.process_cancel(cancel),
            InputMessage::Flush => self.process_flush(),
            InputMessage::QueryTopOfBook(query) => self.process_query_top_of_book(query),
        }
    }

    // -------------------------------------------------------------------------
    // Internal handlers
    // -------------------------------------------------------------------------

    fn process_new_order(&mut self, msg: NewOrder) -> Vec<OutputMessage> {
        // Get or create order book for this symbol.
        let symbol = msg.symbol.clone();
        let book = self.get_or_create_order_book(&symbol);

        // Track order for future cancels.
        let key = (msg.user_id, msg.user_order_id);
        self.order_to_symbol.insert(key, symbol);

        // Delegate to the book. It will emit Ack, Trades, and TOB updates.
        book.add_order(&msg)
    }

    fn process_cancel(&mut self, msg: Cancel) -> Vec<OutputMessage> {
        let key = (msg.user_id, msg.user_order_id);

        // Find which symbol this order belongs to.
        let symbol_opt = self.order_to_symbol.get(&key).cloned();

        match symbol_opt {
            None => {
                // Order not found globally - still send CancelAck (C++ behavior).
                vec![OutputMessage::cancel_ack(
                    msg.user_id,
                    msg.user_order_id,
                    "<unknown>".to_string(), // We don't know the symbol; can be adjusted.
                )]
            }
            Some(symbol) => {
                // If the symbol is known but the book somehow doesn't exist,
                // still send CancelAck and clean up the mapping (C++ behavior).
                let outputs = if let Some(book) = self.order_books.get_mut(&symbol) {
                    book.cancel_order(msg.user_id, msg.user_order_id)
                } else {
                    vec![OutputMessage::cancel_ack(
                        msg.user_id,
                        msg.user_order_id,
                        symbol.clone(),
                    )]
                };

                // Remove from tracking map regardless of whether it existed in the book.
                self.order_to_symbol.remove(&key);

                outputs
            }
        }
    }

    fn process_flush(&mut self) -> Vec<OutputMessage> {
        // Clear all order books and mapping.
        self.order_books.clear();
        self.order_to_symbol.clear();

        // No output messages for flush (same as C++).
        Vec::new()
    }

    /// Process a query for current top-of-book for a given symbol.
    ///
    /// Semantics:
    /// - If the symbol has a book:
    ///   - Emit a bid-side TopOfBook (or eliminated) event.
    ///   - Emit an ask-side TopOfBook (or eliminated) event.
    /// - If the symbol/book doesn't exist:
    ///   - Emit eliminated events for both sides (no book = no orders).
    fn process_query_top_of_book(&mut self, query: TopOfBookQuery) -> Vec<OutputMessage> {
        let symbol = query.symbol;

        // If the book exists, use its snapshot. Otherwise, treat as empty.
        let (bid_price, bid_qty, ask_price, ask_qty) = if let Some(book) = self.order_books.get(&symbol)
        {
            (
                book.best_bid_price(),
                book.best_bid_quantity(),
                book.best_ask_price(),
                book.best_ask_quantity(),
            )
        } else {
            (0, 0, 0, 0)
        };

        let mut outputs = Vec::new();

        // Bid side snapshot.
        if bid_price == 0 {
            outputs.push(OutputMessage::top_of_book_eliminated(symbol.clone(), Side::Buy));
        } else {
            outputs.push(OutputMessage::top_of_book(
                symbol.clone(),
                Side::Buy,
                bid_price,
                bid_qty,
            ));
        }

        // Ask side snapshot.
        if ask_price == 0 {
            outputs.push(OutputMessage::top_of_book_eliminated(symbol.clone(), Side::Sell));
        } else {
            outputs.push(OutputMessage::top_of_book(
                symbol.clone(),
                Side::Sell,
                ask_price,
                ask_qty,
            ));
        }

        outputs
    }

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Get an existing order book for a symbol or create one if it doesn't exist.
    fn get_or_create_order_book(&mut self, symbol: &str) -> &mut OrderBook {
        self.order_books
            .entry(symbol.to_string())
            .or_insert_with(|| OrderBook::new(symbol))
    }

    /// For tests or admin queries: get immutable access to a book by symbol.
    pub fn get_book(&self, symbol: &str) -> Option<&OrderBook> {
        self.order_books.get(symbol)
    }

    /// For tests or admin queries: number of symbols currently tracked.
    pub fn num_symbols(&self) -> usize {
        self.order_books.len()
    }
}

