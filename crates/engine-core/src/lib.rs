//! High-performance matching engine core.
//!
//! # Overview
//! This crate provides a zero-allocation order matching engine implementing
//! price-time priority across multiple symbols.
//!
//! # Power of Ten Compliance
//! This crate follows NASA/JPL Power of Ten safety-critical coding rules:
//! - Rule 1: Simple control flow (no goto, no recursion).
//! - Rule 2: All loops have fixed upper bounds.
//! - Rule 3: No dynamic memory allocation after initialization.
//! - Rule 4: Functions are small (≤60 lines).
//! - Rule 5: Minimum 2 assertions per function.
//! - Rule 6: Variables declared at smallest scope.
//! - Rule 7: All return values checked.
//! - Rule 8: Limited preprocessor use.
//! - Rule 9: Restricted pointer use.
//! - Rule 10: Compile with all warnings, use static analysis.
//!
//! # Usage
//! ```
//! use engine_core::{MatchingEngine, EngineConfig, Symbol, Side, InputMessage, NewOrder};
//! use arrayvec::ArrayVec;
//!
//! // Create engine with strict configuration
//! let mut engine = MatchingEngine::with_config(EngineConfig::default());
//!
//! // Pre-register symbols at startup (no allocation during trading)
//! engine.register_symbol(Symbol::from_str("IBM")).unwrap();
//! engine.register_symbol(Symbol::from_str("AAPL")).unwrap();
//!
//! // Process orders using fixed-size output buffer
//! let order = InputMessage::NewOrder(NewOrder::new(
//!     1,                          // user_id
//!     100,                        // user_order_id  
//!     Symbol::from_str("IBM"),
//!     5000,                       // price (in ticks)
//!     100,                        // quantity
//!     Side::Buy,
//! ));
//!
//! let outputs = engine.process(order).unwrap();
//! ```
//!
//! # Performance Characteristics
//! | Metric | Value |
//! |--------|-------|
//! | Order struct size | 64 bytes (cache-line aligned) |
//! | Symbol size | 8 bytes (fixed, `Copy`) |
//! | Hot path allocations | 0 |
//! | Max orders per price level | 256 |
//! | Max price levels per side | 10,000 |

#![deny(warnings)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]

pub mod error;
pub mod messages;
pub mod order;
pub mod order_book;
pub mod order_type;
pub mod side;
pub mod symbol;
pub mod top_of_book;

mod matching_engine;

// Re-exports for convenient access
pub use error::{EngineError, EngineResult};
pub use matching_engine::{EngineConfig, MatchingEngine, UNKNOWN_SYMBOL};
pub use messages::{
    Ack, Cancel, CancelAck, InputMessage, NewOrder, OutputMessage, Reject, RejectReason,
    TopOfBook, TopOfBookQuery, Trade,
};
pub use order::Order;
pub use order_book::{OrderBook, PriceLevel, MAX_MATCH_ITERATIONS, MAX_ORDERS_PER_LEVEL, MAX_OUTPUTS_PER_ORDER, MAX_PRICE_LEVELS};
pub use order_type::OrderType;
pub use side::Side;
pub use symbol::{Symbol, SYMBOL_MAX_LEN};
pub use top_of_book::TopOfBookSnapshot;

// Compile-time verification of key sizes
const _: () = assert!(std::mem::size_of::<Order>() == 64);
const _: () = assert!(std::mem::size_of::<Symbol>() == 8);
const _: () = assert!(std::mem::size_of::<Side>() == 1);
const _: () = assert!(std::mem::size_of::<OrderType>() == 1);

#[cfg(test)]
mod integration_tests {
    use super::*;
    use arrayvec::ArrayVec;

    #[test]
    fn test_full_trading_session() {
        let mut engine = MatchingEngine::with_config(EngineConfig::for_testing());
        
        // Register symbols
        engine.register_symbols([
            Symbol::from_str("IBM"),
            Symbol::from_str("AAPL"),
            Symbol::from_str("NVDA"),
        ].into_iter()).unwrap();

        // Add bids
        for i in 1..=10 {
            let order = InputMessage::NewOrder(NewOrder::new(
                1, i, Symbol::from_str("IBM"), 100 + i, 10, Side::Buy
            ));
            engine.process(order).unwrap();
        }

        // Best bid should be at highest price
        assert_eq!(engine.top_of_book(Symbol::from_str("IBM")).bid_price, 110);

        // Add matching sells
        for i in 11..=15 {
            let order = InputMessage::NewOrder(NewOrder::new(
                2, i, Symbol::from_str("IBM"), 100, 10, Side::Sell
            ));
            let outputs = engine.process(order).unwrap();
            
            // Should have trades
            assert!(outputs.iter().any(|m| m.is_trade()));
        }

        // Some bids should remain
        assert!(engine.top_of_book(Symbol::from_str("IBM")).bid_price > 0);
    }

    #[test]
    fn test_output_buffer_reuse() {
        let mut engine = MatchingEngine::with_config(EngineConfig::for_testing());
        engine.register_symbol(Symbol::from_str("TEST")).unwrap();

        let mut outputs: ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER> = ArrayVec::new();

        // Process multiple orders, reusing buffer
        for i in 1..=100 {
            outputs.clear();
            
            let order = InputMessage::NewOrder(NewOrder::new(
                1, i, Symbol::from_str("TEST"), 100, 10, Side::Buy
            ));
            
            engine.process_message(order, &mut outputs).unwrap();
            
            // Each order should produce outputs
            assert!(!outputs.is_empty());
        }
    }
}
