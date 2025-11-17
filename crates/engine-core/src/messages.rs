//! Message types used by the core matching engine.
//!
//! These are **transport-agnostic** logical messages:
//! - [`InputMessage`]: what the engine consumes.
//! - [`OutputMessage`]: what the engine produces.
//!
//! All output messages are **symbol-aware** so the networking layer
//! can route / log them without extra context.
//!
//! Note: Binary / CSV encoders live in the `engine-protocol` crate;
//! this module is purely logical.

use crate::order_type::OrderType;
use crate::side::Side;

/// A high-level request into the matching engine.
///
/// This corresponds to your C++ `InputMessage` variant, but with:
/// - `QueryTopOfBook` support (for asking the current top-of-book),
/// - Strongly-typed structs instead of `std::variant`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMessage {
    /// New order: market (price = 0) or limit (price > 0).
    NewOrder(NewOrder),

    /// Cancel an existing order by `(user_id, user_order_id)`.
    Cancel(Cancel),

    /// Flush all order books and internal state.
    Flush,

    /// Query the current top-of-book for a given symbol.
    QueryTopOfBook(TopOfBookQuery),
}

/// A high-level event emitted by the matching engine.
///
/// This corresponds to your C++ `OutputMessage` variant, but:
/// - Every variant now includes `symbol` directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputMessage {
    /// Acknowledgement of a new order.
    Ack(Ack),

    /// Acknowledgement of a cancel request.
    CancelAck(CancelAck),

    /// Trade event between a buyer and a seller.
    Trade(Trade),

    /// Top-of-book change or snapshot.
    TopOfBook(TopOfBook),
}

/// New order message (input).
///
/// Equivalent to your C++ `NewOrderMessage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOrder {
    /// User identifier (logical session / account).
    pub user_id: u32,

    /// Instrument symbol, e.g. `"IBM"` or `"BTC-USD"`.
    pub symbol: String,

    /// Price in integer ticks.
    /// - `0` => market order
    /// - `>0` => limit order
    pub price: u32,

    /// Original quantity.
    pub quantity: u32,

    /// Buy or Sell.
    pub side: Side,

    /// User-local order identifier (for canceling later).
    pub user_order_id: u32,
}

impl NewOrder {
    /// Helper: returns the corresponding `OrderType`
    /// (market vs limit) based on price.
    pub fn order_type(&self) -> OrderType {
        if self.price == 0 {
            OrderType::Market
        } else {
            OrderType::Limit
        }
    }
}

/// Cancel message (input).
///
/// Equivalent to your C++ `CancelMessage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cancel {
    pub user_id: u32,
    pub user_order_id: u32,
}

/// Query top-of-book message (input).
///
/// NEW compared to the C++ version:
/// lets a client ask for the current best bid/ask for a symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopOfBookQuery {
    /// Symbol whose top-of-book is requested.
    pub symbol: String,
}

/// Acknowledgement of a new order (output).
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack {
    pub user_id: u32,
    pub user_order_id: u32,
    pub symbol: String,
}

/// Acknowledgement of a cancel request (output).
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelAck {
    pub user_id: u32,
    pub user_order_id: u32,
    pub symbol: String,
}

/// Trade event (output).
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trade {
    /// Instrument symbol.
    pub symbol: String,

    pub user_id_buy: u32,
    pub user_order_id_buy: u32,

    pub user_id_sell: u32,
    pub user_order_id_sell: u32,

    pub price: u32,
    pub quantity: u32,
}

/// Top-of-book event (output).
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopOfBook {
    /// Instrument symbol.
    pub symbol: String,

    /// Side this TOB event refers to (bid or ask).
    pub side: Side,

    /// Best price; `0` means "no price" (side eliminated).
    pub price: u32,

    /// Total quantity at the best price; `0` implies eliminated.
    pub total_quantity: u32,

    /// True when the side is eliminated (no orders on that side).
    /// When `true`, `price` and `total_quantity` should be ignored.
    pub eliminated: bool,
}

// -----------------------------------------------------------------------------
// Convenience constructors (similar spirit to your C++ static helpers)
// -----------------------------------------------------------------------------

impl OutputMessage {
    /// Convenience constructor for an Ack event.
    pub fn ack(user_id: u32, user_order_id: u32, symbol: impl Into<String>) -> Self {
        OutputMessage::Ack(Ack {
            user_id,
            user_order_id,
            symbol: symbol.into(),
        })
    }

    /// Convenience constructor for a CancelAck event.
    pub fn cancel_ack(user_id: u32, user_order_id: u32, symbol: impl Into<String>) -> Self {
        OutputMessage::CancelAck(CancelAck {
            user_id,
            user_order_id,
            symbol: symbol.into(),
        })
    }

    /// Convenience constructor for a Trade event.
    pub fn trade(
        symbol: impl Into<String>,
        user_id_buy: u32,
        user_order_id_buy: u32,
        user_id_sell: u32,
        user_order_id_sell: u32,
        price: u32,
        quantity: u32,
    ) -> Self {
        OutputMessage::Trade(Trade {
            symbol: symbol.into(),
            user_id_buy,
            user_order_id_buy,
            user_id_sell,
            user_order_id_sell,
            price,
            quantity,
        })
    }

    /// Convenience constructor for a non-eliminated top-of-book event.
    pub fn top_of_book(
        symbol: impl Into<String>,
        side: Side,
        price: u32,
        total_quantity: u32,
    ) -> Self {
        OutputMessage::TopOfBook(TopOfBook {
            symbol: symbol.into(),
            side,
            price,
            total_quantity,
            eliminated: false,
        })
    }

    /// Convenience constructor for an eliminated top-of-book event.
    pub fn top_of_book_eliminated(symbol: impl Into<String>, side: Side) -> Self {
        OutputMessage::TopOfBook(TopOfBook {
            symbol: symbol.into(),
            side,
            price: 0,
            total_quantity: 0,
            eliminated: true,
        })
    }
}

