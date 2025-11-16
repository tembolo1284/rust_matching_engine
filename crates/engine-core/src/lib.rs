//! engine-core
//!
//! Pure matching engine logic:
//! - messages (input/output types)
//! - order representation
//! - per-symbol order book
//! - multi-symbol matching engine

pub mod side;
pub mod order_type;
pub mod messages;
pub mod order;
pub mod order_book;
pub mod matching_engine;
pub mod error;
pub mod top_of_book;

pub use side::Side;
pub use order_type::OrderType;

pub use messages::{
    Ack,
    Cancel,
    CancelAck,
    InputMessage,
    NewOrder,
    OutputMessage,
    TopOfBook,
    TopOfBookQuery,
    Trade,
};

pub use order::Order;
pub use order_book::OrderBook;
pub use matching_engine::MatchingEngine;
pub use error::EngineError;

