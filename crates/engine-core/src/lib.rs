//! High-performance matching engine core.
//!
//! # Design Principles
//!
//! ## Cache Optimization
//!
//! - `Order`: 64 bytes, cache-line aligned
//! - `Symbol`: 8 bytes fixed, `Copy`
//! - Price levels in sorted `Vec` for sequential access
//! - Output buffer passed by caller (no allocation)
//!
//! ## Zero-Allocation Hot Path
//!
//! After initialization:
//! - No `String` allocations (use `Symbol`)
//! - No `Vec` allocations (caller-provided buffer)
//! - No hash map resizing (pre-sized)
//!
//! # Example
//!
//! ```rust
//! use engine_core::{
//!     MatchingEngine, InputMessage, NewOrder, Symbol, Side, OutputMessage,
//! };
//!
//! let mut engine = MatchingEngine::new();
//!
//! // Pre-register symbols
//! engine.register_symbol(Symbol::from_str("IBM"));
//!
//! // Reusable output buffer
//! let mut outputs = Vec::with_capacity(64);
//!
//! // Process order
//! let order = NewOrder::new(
//!     1,                          // user_id
//!     100,                        // user_order_id
//!     Symbol::from_str("IBM"),    // symbol
//!     1000,                       // price
//!     50,                         // quantity
//!     Side::Buy,
//! );
//!
//! engine.process_message(InputMessage::NewOrder(order), &mut outputs);
//!
//! for msg in &outputs {
//!     println!("{:?}", msg);
//! }
//! ```

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(clippy::all)]

pub mod error;
pub mod matching_engine;
pub mod messages;
pub mod order;
pub mod order_book;
pub mod order_type;
pub mod side;
pub mod symbol;
pub mod top_of_book;

// Re-export main types at crate root
pub use error::EngineError;
pub use matching_engine::{EngineConfig, MatchingEngine};
pub use messages::{
    Ack, Cancel, CancelAck, InputMessage, NewOrder, OutputMessage, TopOfBook, TopOfBookQuery, Trade,
};
pub use order::Order;
pub use order_book::OrderBook;
pub use order_type::OrderType;
pub use side::Side;
pub use symbol::Symbol;
pub use top_of_book::TopOfBookSnapshot;
