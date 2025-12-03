//! Error types for the matching engine.
//!
//! The core engine is designed to be infallible for normal operations.
//! Invalid input should be filtered at the protocol layer.
//!
//! These errors are for exceptional conditions and admin operations.

use crate::symbol::Symbol;

/// Engine error type.
///
/// Uses `Symbol` instead of `String` to avoid heap allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineError {
    /// The requested symbol does not exist.
    UnknownSymbol(Symbol),

    /// Order not found for cancel.
    OrderNotFound {
        /// The user/session ID that submitted the order.
        user_id: u32,
        /// The user-assigned order ID.
        user_order_id: u32,
    },

    /// Capacity exceeded (e.g., max orders, max symbols).
    CapacityExceeded,

    /// Invalid price (e.g., zero price for limit order).
    InvalidPrice,

    /// Invalid quantity (e.g., zero quantity).
    InvalidQuantity,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::UnknownSymbol(sym) => {
                write!(f, "unknown symbol: {}", sym)
            }
            EngineError::OrderNotFound { user_id, user_order_id } => {
                write!(f, "order not found: user_id={}, user_order_id={}", user_id, user_order_id)
            }
            EngineError::CapacityExceeded => {
                write!(f, "capacity exceeded")
            }
            EngineError::InvalidPrice => {
                write!(f, "invalid price")
            }
            EngineError::InvalidQuantity => {
                write!(f, "invalid quantity")
            }
        }
    }
}

impl std::error::Error for EngineError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_is_copy() {
        let err = EngineError::UnknownSymbol(Symbol::from_str("IBM"));
        let err2 = err; // Copy
        assert_eq!(err, err2);
    }

    #[test]
    fn test_error_display() {
        let err = EngineError::UnknownSymbol(Symbol::from_str("AAPL"));
        assert_eq!(format!("{}", err), "unknown symbol: AAPL");
    }
}
