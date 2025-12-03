//! Message types for the matching engine.
//!
//! # Design Principles
//! - **Zero heap allocation**: All messages use `Symbol` (8 bytes) instead of `String`.
//! - **Symbol in every output**: Enables stateless routing/logging downstream.
//! - **`repr(C)`**: Predictable memory layout for potential direct serialization.
//! - **`Copy` where possible**: Cheap to pass by value.
//!

use crate::order_type::OrderType;
use crate::side::Side;
use crate::symbol::Symbol;

// =============================================================================
// Input Messages
// =============================================================================

/// Input message to the matching engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMessage {
    /// New order submission.
    NewOrder(NewOrder),
    /// Cancel an existing order.
    Cancel(Cancel),
    /// Flush all order books.
    Flush,
    /// Query current top-of-book for a symbol.
    QueryTopOfBook(TopOfBookQuery),
}

/// New order request.
///
/// Size: 32 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct NewOrder {
    /// User/session identifier.
    pub user_id: u32,
    /// User-assigned order ID (for cancel/fill tracking).
    pub user_order_id: u32,
    /// Symbol (fixed 8 bytes).
    pub symbol: Symbol,
    /// Price in ticks. 0 = market order.
    pub price: u32,
    /// Quantity to buy/sell.
    pub quantity: u32,
    /// Buy or Sell.
    pub side: Side,
    /// Padding for alignment.
    _pad: [u8; 3],
}

impl NewOrder {
    /// Create a new order with the specified parameters.
    #[inline]
    pub fn new(
        user_id: u32,
        user_order_id: u32,
        symbol: Symbol,
        price: u32,
        quantity: u32,
        side: Side,
    ) -> Self {
        NewOrder {
            user_id,
            user_order_id,
            symbol,
            price,
            quantity,
            side,
            _pad: [0; 3],
        }
    }

    /// Infer order type from price.
    #[inline]
    pub fn order_type(&self) -> OrderType {
        OrderType::from_price(self.price)
    }
}

/// Cancel order request.
///
/// Note: Symbol is not included here because the engine tracks
/// order-to-symbol mapping internally. The output CancelAck will
/// include the symbol.
///
/// Size: 8 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Cancel {
    /// The user/session ID that submitted the original order.
    pub user_id: u32,
    /// The user-assigned order ID to cancel.
    pub user_order_id: u32,
}

impl Cancel {
    /// Create a new cancel request.
    #[inline]
    pub fn new(user_id: u32, user_order_id: u32) -> Self {
        Cancel { user_id, user_order_id }
    }
}

/// Top-of-book query request.
///
/// Size: 8 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TopOfBookQuery {
    /// The symbol to query.
    pub symbol: Symbol,
}

impl TopOfBookQuery {
    /// Create a new top-of-book query for the given symbol.
    #[inline]
    pub fn new(symbol: Symbol) -> Self {
        TopOfBookQuery { symbol }
    }
}

// =============================================================================
// Output Messages
// =============================================================================

/// Output message from the matching engine.
///
/// Every variant includes the symbol for stateless downstream routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMessage {
    /// Order accepted.
    Ack(Ack),
    /// Cancel processed.
    CancelAck(CancelAck),
    /// Trade executed.
    Trade(Trade),
    /// Top-of-book update.
    TopOfBook(TopOfBook),
}

/// Order acknowledgement.
///
/// Size: 16 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Ack {
    /// The user/session ID that submitted the order.
    pub user_id: u32,
    /// The user-assigned order ID.
    pub user_order_id: u32,
    /// The symbol for the acknowledged order.
    pub symbol: Symbol,
}

impl Ack {
    /// Create a new order acknowledgement.
    #[inline]
    pub fn new(user_id: u32, user_order_id: u32, symbol: Symbol) -> Self {
        Ack { user_id, user_order_id, symbol }
    }
}

/// Cancel acknowledgement.
///
/// Size: 16 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct CancelAck {
    /// The user/session ID that submitted the cancel.
    pub user_id: u32,
    /// The user-assigned order ID that was cancelled.
    pub user_order_id: u32,
    /// The symbol for the cancelled order.
    pub symbol: Symbol,
}

impl CancelAck {
    /// Create a new cancel acknowledgement.
    #[inline]
    pub fn new(user_id: u32, user_order_id: u32, symbol: Symbol) -> Self {
        CancelAck { user_id, user_order_id, symbol }
    }
}

/// Trade execution report.
///
/// Size: 40 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Trade {
    /// Symbol traded.
    pub symbol: Symbol,
    /// Buyer's user ID.
    pub user_id_buy: u32,
    /// Buyer's order ID.
    pub user_order_id_buy: u32,
    /// Seller's user ID.
    pub user_id_sell: u32,
    /// Seller's order ID.
    pub user_order_id_sell: u32,
    /// Execution price.
    pub price: u32,
    /// Execution quantity.
    pub quantity: u32,
}

impl Trade {
    /// Create a new trade execution report.
    #[inline]
    pub fn new(
        symbol: Symbol,
        user_id_buy: u32,
        user_order_id_buy: u32,
        user_id_sell: u32,
        user_order_id_sell: u32,
        price: u32,
        quantity: u32,
    ) -> Self {
        Trade {
            symbol,
            user_id_buy,
            user_order_id_buy,
            user_id_sell,
            user_order_id_sell,
            price,
            quantity,
        }
    }
}

/// Top-of-book update.
///
/// Size: 24 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TopOfBook {
    /// Symbol.
    pub symbol: Symbol,
    /// Which side (bid or ask).
    pub side: Side,
    /// True if this side has no orders (eliminated).
    pub eliminated: bool,
    /// Padding.
    _pad: [u8; 2],
    /// Best price (0 if eliminated).
    pub price: u32,
    /// Total quantity at best price (0 if eliminated).
    pub total_quantity: u32,
}

impl TopOfBook {
    /// Create an active (non-eliminated) top-of-book update.
    #[inline]
    pub fn active(symbol: Symbol, side: Side, price: u32, total_quantity: u32) -> Self {
        debug_assert!(price > 0, "active TOB must have price > 0");
        debug_assert!(total_quantity > 0, "active TOB must have quantity > 0");
        TopOfBook {
            symbol,
            side,
            eliminated: false,
            _pad: [0; 2],
            price,
            total_quantity,
        }
    }

    /// Create an eliminated top-of-book update.
    #[inline]
    pub fn eliminated(symbol: Symbol, side: Side) -> Self {
        TopOfBook {
            symbol,
            side,
            eliminated: true,
            _pad: [0; 2],
            price: 0,
            total_quantity: 0,
        }
    }

    /// Check if this side is eliminated (no orders).
    #[inline]
    pub fn is_eliminated(&self) -> bool {
        self.eliminated
    }
}

// =============================================================================
// Convenience constructors on OutputMessage
// =============================================================================

impl OutputMessage {
    /// Create an order acknowledgement message.
    #[inline]
    pub fn ack(user_id: u32, user_order_id: u32, symbol: Symbol) -> Self {
        OutputMessage::Ack(Ack::new(user_id, user_order_id, symbol))
    }

    /// Create a cancel acknowledgement message.
    #[inline]
    pub fn cancel_ack(user_id: u32, user_order_id: u32, symbol: Symbol) -> Self {
        OutputMessage::CancelAck(CancelAck::new(user_id, user_order_id, symbol))
    }

    /// Create a trade execution message.
    #[inline]
    pub fn trade(
        symbol: Symbol,
        user_id_buy: u32,
        user_order_id_buy: u32,
        user_id_sell: u32,
        user_order_id_sell: u32,
        price: u32,
        quantity: u32,
    ) -> Self {
        OutputMessage::Trade(Trade::new(
            symbol,
            user_id_buy,
            user_order_id_buy,
            user_id_sell,
            user_order_id_sell,
            price,
            quantity,
        ))
    }

    /// Create an active top-of-book update message.
    #[inline]
    pub fn top_of_book(symbol: Symbol, side: Side, price: u32, total_quantity: u32) -> Self {
        OutputMessage::TopOfBook(TopOfBook::active(symbol, side, price, total_quantity))
    }

    /// Create an eliminated top-of-book update message.
    #[inline]
    pub fn top_of_book_eliminated(symbol: Symbol, side: Side) -> Self {
        OutputMessage::TopOfBook(TopOfBook::eliminated(symbol, side))
    }

    /// Extract the symbol from any output message.
    #[inline]
    pub fn symbol(&self) -> Symbol {
        match self {
            OutputMessage::Ack(m) => m.symbol,
            OutputMessage::CancelAck(m) => m.symbol,
            OutputMessage::Trade(m) => m.symbol,
            OutputMessage::TopOfBook(m) => m.symbol,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_order_size() {
        assert_eq!(std::mem::size_of::<NewOrder>(), 32);
    }

    #[test]
    fn test_ack_size() {
        assert_eq!(std::mem::size_of::<Ack>(), 16);
    }

    #[test]
    fn test_trade_size() {
        assert_eq!(std::mem::size_of::<Trade>(), 40);
    }

    #[test]
    fn test_top_of_book_size() {
        assert_eq!(std::mem::size_of::<TopOfBook>(), 24);
    }

    #[test]
    fn test_output_message_symbol_extraction() {
        let sym = Symbol::from_str("AAPL");

        let ack = OutputMessage::ack(1, 2, sym);
        assert_eq!(ack.symbol(), sym);

        let trade = OutputMessage::trade(sym, 1, 2, 3, 4, 100, 50);
        assert_eq!(trade.symbol(), sym);
    }

    #[test]
    fn test_messages_are_copy() {
        let msg = OutputMessage::ack(1, 2, Symbol::from_str("IBM"));
        let msg2 = msg; // Copy
        assert_eq!(msg, msg2);
    }
}
