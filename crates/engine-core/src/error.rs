//! Error types for the matching engine.
//!
//! # Design
//! - All errors are `Copy` for zero-allocation error handling.
//! - Error codes match reject reasons for wire protocol.
//!
//! # Power of Ten Compliance
//! - Rule 3: No dynamic allocation (no `String` in errors).
//! - Rule 7: All errors are explicit and must be handled.

use crate::symbol::Symbol;

/// Errors that can occur during order processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineError {
    /// Symbol is not registered (strict mode).
    UnknownSymbol(Symbol),

    /// Order tracking map is at capacity.
    OrderCapacityExceeded {
        current: usize,
        max: usize,
    },

    /// Price level capacity exceeded for a symbol.
    PriceLevelCapacityExceeded {
        symbol: Symbol,
        side: crate::side::Side,
    },

    /// Orders per price level exceeded.
    OrdersPerLevelExceeded {
        symbol: Symbol,
        price: u32,
    },

    /// Output buffer is full.
    OutputBufferFull {
        current: usize,
        max: usize,
    },

    /// Duplicate order ID.
    DuplicateOrderId {
        user_id: u32,
        user_order_id: u32,
    },

    /// Invalid order parameters.
    InvalidOrder(&'static str),

    /// Symbol already registered.
    SymbolAlreadyRegistered(Symbol),

    /// Maximum symbols exceeded.
    MaxSymbolsExceeded {
        current: usize,
        max: usize,
    },
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSymbol(sym) => write!(f, "unknown symbol: {}", sym),
            Self::OrderCapacityExceeded { current, max } => {
                write!(f, "order capacity exceeded: {}/{}", current, max)
            }
            Self::PriceLevelCapacityExceeded { symbol, side } => {
                write!(f, "price level capacity exceeded: {} {:?}", symbol, side)
            }
            Self::OrdersPerLevelExceeded { symbol, price } => {
                write!(f, "orders per level exceeded: {} @ {}", symbol, price)
            }
            Self::OutputBufferFull { current, max } => {
                write!(f, "output buffer full: {}/{}", current, max)
            }
            Self::DuplicateOrderId { user_id, user_order_id } => {
                write!(f, "duplicate order ID: ({}, {})", user_id, user_order_id)
            }
            Self::InvalidOrder(reason) => write!(f, "invalid order: {}", reason),
            Self::SymbolAlreadyRegistered(sym) => {
                write!(f, "symbol already registered: {}", sym)
            }
            Self::MaxSymbolsExceeded { current, max } => {
                write!(f, "max symbols exceeded: {}/{}", current, max)
            }
        }
    }
}

impl std::error::Error for EngineError {}

/// Result type for engine operations.
pub type EngineResult<T> = Result<T, EngineError>;

// Compile-time size check - ensure error is small enough to return by value
const _: () = assert!(
    std::mem::size_of::<EngineError>() <= 32,
    "EngineError should be small for efficient returns"
);

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
        let err = EngineError::OrderCapacityExceeded {
            current: 1000,
            max: 1000,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("1000"));
    }
}
