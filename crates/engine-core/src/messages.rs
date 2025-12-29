//! Message types for the matching engine.
//!
//! # Design Principles
//! - **Zero heap allocation**: All messages use `Symbol` (8 bytes) instead of `String`.
//! - **Symbol in every output**: Enables stateless routing/logging downstream.
//! - **`repr(C)`**: Predictable memory layout for binary serialization.
//! - **`Copy`**: Cheap to pass by value, no heap allocation.
//!
//! # Power of Ten Compliance
//! - Rule 3: No dynamic allocation.
//! - Rule 5: Assertions on construction.
//! - Compile-time size verification.

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
/// # Memory Layout (28 bytes)
/// ```text
/// Offset  Size  Field
/// ------  ----  -----
///   0      4    user_id
///   4      4    user_order_id  
///   8      8    symbol
///  16      4    price
///  20      4    quantity
///  24      1    side
///  25      3    _pad
/// ```
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

// Compile-time verification
const _: () = assert!(std::mem::size_of::<NewOrder>() == 28, "NewOrder must be 28 bytes");

impl NewOrder {
    /// Create a new order request.
    ///
    /// # Panics (debug only)
    /// - If quantity is zero.
    /// - If user_order_id is zero.
    /// - If symbol is empty.
    #[inline]
    pub fn new(
        user_id: u32,
        user_order_id: u32,
        symbol: Symbol,
        price: u32,
        quantity: u32,
        side: Side,
    ) -> Self {
        debug_assert!(quantity > 0, "NewOrder quantity must be > 0");
        debug_assert!(user_order_id > 0, "NewOrder user_order_id must be > 0");
        debug_assert!(!symbol.is_empty(), "NewOrder symbol cannot be empty");

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

    /// Check if this is a market order (price == 0).
    #[inline]
    pub const fn is_market(&self) -> bool {
        self.price == 0
    }

    /// Check if this is a limit order (price > 0).
    #[inline]
    pub const fn is_limit(&self) -> bool {
        self.price > 0
    }

    /// Get the order key for tracking.
    #[inline]
    pub const fn key(&self) -> (u32, u32) {
        (self.user_id, self.user_order_id)
    }
}

/// Cancel order request.
///
/// Note: Symbol is not included because the engine tracks order-to-symbol
/// mapping internally. The output CancelAck includes the symbol.
///
/// # Memory Layout (8 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Cancel {
    /// The user/session ID that submitted the original order.
    pub user_id: u32,
    /// The user-assigned order ID to cancel.
    pub user_order_id: u32,
}

const _: () = assert!(std::mem::size_of::<Cancel>() == 8, "Cancel must be 8 bytes");

impl Cancel {
    /// Create a new cancel request.
    #[inline]
    pub const fn new(user_id: u32, user_order_id: u32) -> Self {
        Cancel { user_id, user_order_id }
    }

    /// Get the order key.
    #[inline]
    pub const fn key(&self) -> (u32, u32) {
        (self.user_id, self.user_order_id)
    }
}

/// Top-of-book query request.
///
/// # Memory Layout (8 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TopOfBookQuery {
    /// The symbol to query.
    pub symbol: Symbol,
}

const _: () = assert!(std::mem::size_of::<TopOfBookQuery>() == 8, "TopOfBookQuery must be 8 bytes");

impl TopOfBookQuery {
    /// Create a new top-of-book query.
    #[inline]
    pub fn new(symbol: Symbol) -> Self {
        debug_assert!(!symbol.is_empty(), "Cannot query empty symbol");
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
    /// Order rejected (for strict mode).
    Reject(Reject),
}

/// Order acknowledgement.
///
/// # Memory Layout (16 bytes)
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

const _: () = assert!(std::mem::size_of::<Ack>() == 16, "Ack must be 16 bytes");

impl Ack {
    #[inline]
    pub const fn new(user_id: u32, user_order_id: u32, symbol: Symbol) -> Self {
        Ack { user_id, user_order_id, symbol }
    }
}

/// Cancel acknowledgement.
///
/// # Memory Layout (16 bytes)
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

const _: () = assert!(std::mem::size_of::<CancelAck>() == 16, "CancelAck must be 16 bytes");

impl CancelAck {
    #[inline]
    pub const fn new(user_id: u32, user_order_id: u32, symbol: Symbol) -> Self {
        CancelAck { user_id, user_order_id, symbol }
    }
}

/// Trade execution report.
///
/// # Memory Layout (32 bytes)
/// ```text
/// Offset  Size  Field
/// ------  ----  -----
///   0      8    symbol
///   8      4    user_id_buy
///  12      4    user_order_id_buy
///  16      4    user_id_sell
///  20      4    user_order_id_sell
///  24      4    price
///  28      4    quantity
/// ```
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

const _: () = assert!(std::mem::size_of::<Trade>() == 32, "Trade must be 32 bytes");

impl Trade {
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
        debug_assert!(price > 0, "Trade price must be > 0");
        debug_assert!(quantity > 0, "Trade quantity must be > 0");
        debug_assert!(!symbol.is_empty(), "Trade symbol cannot be empty");

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

    /// Get buyer's order key.
    #[inline]
    pub const fn buyer_key(&self) -> (u32, u32) {
        (self.user_id_buy, self.user_order_id_buy)
    }

    /// Get seller's order key.
    #[inline]
    pub const fn seller_key(&self) -> (u32, u32) {
        (self.user_id_sell, self.user_order_id_sell)
    }
}

/// Top-of-book update.
///
/// # Memory Layout (20 bytes)
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

const _: () = assert!(std::mem::size_of::<TopOfBook>() == 20, "TopOfBook must be 20 bytes");

impl TopOfBook {
    /// Create an active (non-eliminated) top-of-book update.
    #[inline]
    pub fn active(symbol: Symbol, side: Side, price: u32, total_quantity: u32) -> Self {
        debug_assert!(price > 0, "active TOB must have price > 0");
        debug_assert!(total_quantity > 0, "active TOB must have quantity > 0");
        debug_assert!(!symbol.is_empty(), "TOB symbol cannot be empty");

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
        debug_assert!(!symbol.is_empty(), "TOB symbol cannot be empty");

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
    pub const fn is_eliminated(&self) -> bool {
        self.eliminated
    }

    /// Check if this side is active (has orders).
    #[inline]
    pub const fn is_active(&self) -> bool {
        !self.eliminated
    }
}

/// Rejection reason codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RejectReason {
    /// Symbol not registered.
    UnknownSymbol = 1,
    /// Order tracking capacity exceeded.
    CapacityExceeded = 2,
    /// Invalid order parameters.
    InvalidOrder = 3,
    /// Duplicate order ID.
    DuplicateOrderId = 4,
}

const _: () = assert!(std::mem::size_of::<RejectReason>() == 1);

/// Order rejection message.
///
/// # Memory Layout (20 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Reject {
    /// User ID.
    pub user_id: u32,
    /// Order ID.
    pub user_order_id: u32,
    /// Symbol (may be UNKNOWN).
    pub symbol: Symbol,
    /// Reason for rejection.
    pub reason: RejectReason,
    /// Padding.
    _pad: [u8; 3],
}

const _: () = assert!(std::mem::size_of::<Reject>() == 20, "Reject must be 20 bytes");

impl Reject {
    #[inline]
    pub const fn new(user_id: u32, user_order_id: u32, symbol: Symbol, reason: RejectReason) -> Self {
        Reject {
            user_id,
            user_order_id,
            symbol,
            reason,
            _pad: [0; 3],
        }
    }
}

// =============================================================================
// Convenience constructors on OutputMessage
// =============================================================================

impl OutputMessage {
    #[inline]
    pub fn ack(user_id: u32, user_order_id: u32, symbol: Symbol) -> Self {
        OutputMessage::Ack(Ack::new(user_id, user_order_id, symbol))
    }

    #[inline]
    pub fn cancel_ack(user_id: u32, user_order_id: u32, symbol: Symbol) -> Self {
        OutputMessage::CancelAck(CancelAck::new(user_id, user_order_id, symbol))
    }

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

    #[inline]
    pub fn top_of_book(symbol: Symbol, side: Side, price: u32, total_quantity: u32) -> Self {
        OutputMessage::TopOfBook(TopOfBook::active(symbol, side, price, total_quantity))
    }

    #[inline]
    pub fn top_of_book_eliminated(symbol: Symbol, side: Side) -> Self {
        OutputMessage::TopOfBook(TopOfBook::eliminated(symbol, side))
    }

    #[inline]
    pub fn reject(user_id: u32, user_order_id: u32, symbol: Symbol, reason: RejectReason) -> Self {
        OutputMessage::Reject(Reject::new(user_id, user_order_id, symbol, reason))
    }

    /// Extract the symbol from any output message.
    #[inline]
    pub fn symbol(&self) -> Symbol {
        match self {
            OutputMessage::Ack(m) => m.symbol,
            OutputMessage::CancelAck(m) => m.symbol,
            OutputMessage::Trade(m) => m.symbol,
            OutputMessage::TopOfBook(m) => m.symbol,
            OutputMessage::Reject(m) => m.symbol,
        }
    }

    /// Check if this is an acknowledgement.
    #[inline]
    pub const fn is_ack(&self) -> bool {
        matches!(self, OutputMessage::Ack(_))
    }

    /// Check if this is a trade.
    #[inline]
    pub const fn is_trade(&self) -> bool {
        matches!(self, OutputMessage::Trade(_))
    }

    /// Check if this is a rejection.
    #[inline]
    pub const fn is_reject(&self) -> bool {
        matches!(self, OutputMessage::Reject(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_sizes() {
        assert_eq!(std::mem::size_of::<NewOrder>(), 28);
        assert_eq!(std::mem::size_of::<Cancel>(), 8);
        assert_eq!(std::mem::size_of::<TopOfBookQuery>(), 8);
        assert_eq!(std::mem::size_of::<Ack>(), 16);
        assert_eq!(std::mem::size_of::<CancelAck>(), 16);
        assert_eq!(std::mem::size_of::<Trade>(), 32);
        assert_eq!(std::mem::size_of::<TopOfBook>(), 20);
        assert_eq!(std::mem::size_of::<Reject>(), 20);
        assert_eq!(std::mem::size_of::<RejectReason>(), 1);
    }

    #[test]
    fn test_new_order_key() {
        let order = NewOrder::new(42, 100, Symbol::from_str("IBM"), 50, 10, Side::Buy);
        assert_eq!(order.key(), (42, 100));
        assert!(order.is_limit());
        assert!(!order.is_market());
    }

    #[test]
    fn test_market_order() {
        let order = NewOrder::new(1, 1, Symbol::from_str("IBM"), 0, 10, Side::Buy);
        assert!(order.is_market());
        assert!(!order.is_limit());
    }

    #[test]
    fn test_trade_keys() {
        let trade = Trade::new(
            Symbol::from_str("IBM"),
            1, 100,  // buyer
            2, 200,  // seller
            50, 10,
        );
        assert_eq!(trade.buyer_key(), (1, 100));
        assert_eq!(trade.seller_key(), (2, 200));
    }

    #[test]
    fn test_output_message_symbol() {
        let sym = Symbol::from_str("AAPL");

        assert_eq!(OutputMessage::ack(1, 2, sym).symbol(), sym);
        assert_eq!(OutputMessage::cancel_ack(1, 2, sym).symbol(), sym);
        assert_eq!(OutputMessage::trade(sym, 1, 2, 3, 4, 100, 50).symbol(), sym);
        assert_eq!(OutputMessage::top_of_book(sym, Side::Buy, 100, 50).symbol(), sym);
    }

    #[test]
    fn test_messages_are_copy() {
        let msg = OutputMessage::ack(1, 2, Symbol::from_str("IBM"));
        let msg2 = msg; // Copy
        assert_eq!(msg, msg2);

        let input = InputMessage::NewOrder(NewOrder::new(
            1, 1, Symbol::from_str("X"), 100, 10, Side::Buy
        ));
        let input2 = input; // Copy
        assert_eq!(input, input2);
    }
}
