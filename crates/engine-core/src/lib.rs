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
pub use order_book::{
    OrderBook, PriceLevel, 
    MAX_MATCH_ITERATIONS, MAX_ORDERS_PER_LEVEL, MAX_OUTPUTS_PER_ORDER, MAX_PRICE_LEVELS,
};
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

        assert_eq!(engine.top_of_book(Symbol::from_str("IBM")).bid_price, 110);

        // Add matching sells
        for i in 11..=15 {
            let order = InputMessage::NewOrder(NewOrder::new(
                2, i, Symbol::from_str("IBM"), 100, 10, Side::Sell
            ));
            let outputs = engine.process(order).unwrap();
            assert!(outputs.iter().any(|m| m.is_trade()));
        }

        assert!(engine.top_of_book(Symbol::from_str("IBM")).bid_price > 0);
    }

    #[test]
    fn test_output_buffer_reuse() {
        let mut engine = MatchingEngine::with_config(EngineConfig::for_testing());
        engine.register_symbol(Symbol::from_str("TEST")).unwrap();

        let mut outputs: ArrayVec<OutputMessage, MAX_OUTPUTS_PER_ORDER> = ArrayVec::new();

        for i in 1..=100 {
            outputs.clear();
            
            let order = InputMessage::NewOrder(NewOrder::new(
                1, i, Symbol::from_str("TEST"), 100, 10, Side::Buy
            ));
            
            engine.process_message(order, &mut outputs).unwrap();
            assert!(!outputs.is_empty());
        }
    }
}
