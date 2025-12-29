//! Order type: Market or Limit.
//!
//! # Power of Ten Compliance
//! - Rule 3: No dynamic allocation.
//! - Rule 5: Assertions where meaningful.
//! - Explicit `repr(u8)` for predictable 1-byte binary representation.

/// Order type: Market or Limit.
///
/// # Binary Representation
/// Explicit `repr(u8)` guarantees 1-byte size.
///
/// ```text
/// Market = 0x00
/// Limit  = 0x01
/// ```
///
/// # Semantics
/// - **Market**: Execute immediately at best available price. Price field = 0.
/// - **Limit**: Execute only at specified price or better. Price field > 0.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum OrderType {
    /// Market order: execute immediately at best available price.
    #[default]
    Market = 0,
    /// Limit order: execute only at specified price or better.
    Limit = 1,
}

// Compile-time size verification
const _: () = assert!(std::mem::size_of::<OrderType>() == 1, "OrderType must be 1 byte");

impl OrderType {
    /// All valid order types.
    pub const ALL: [OrderType; 2] = [OrderType::Market, OrderType::Limit];

    /// Infer order type from price.
    /// - `price == 0` => Market
    /// - `price > 0`  => Limit
    #[inline]
    pub const fn from_price(price: u32) -> Self {
        if price == 0 {
            OrderType::Market
        } else {
            OrderType::Limit
        }
    }

    /// Parse from u8 wire format.
    #[inline]
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(OrderType::Market),
            1 => Some(OrderType::Limit),
            _ => None,
        }
    }

    /// Parse from u8, panicking on invalid input.
    #[inline]
    pub const fn from_u8_unchecked(val: u8) -> Self {
        match val {
            0 => OrderType::Market,
            1 => OrderType::Limit,
            _ => panic!("invalid order type value"),
        }
    }

    /// Convert to u8 for wire format.
    #[inline]
    pub const fn to_u8(self) -> u8 {
        self as u8
    }

    /// Returns true if this is a market order.
    #[inline]
    pub const fn is_market(self) -> bool {
        matches!(self, OrderType::Market)
    }

    /// Returns true if this is a limit order.
    #[inline]
    pub const fn is_limit(self) -> bool {
        matches!(self, OrderType::Limit)
    }

    /// Check if this order type can rest in the book.
    /// Only limit orders can rest; market orders execute immediately or cancel.
    #[inline]
    pub const fn can_rest(self) -> bool {
        self.is_limit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_type_size() {
        assert_eq!(std::mem::size_of::<OrderType>(), 1);
    }

    #[test]
    fn test_order_type_values() {
        assert_eq!(OrderType::Market as u8, 0);
        assert_eq!(OrderType::Limit as u8, 1);
    }

    #[test]
    fn test_order_type_from_price() {
        assert_eq!(OrderType::from_price(0), OrderType::Market);
        assert_eq!(OrderType::from_price(1), OrderType::Limit);
        assert_eq!(OrderType::from_price(100), OrderType::Limit);
        assert_eq!(OrderType::from_price(u32::MAX), OrderType::Limit);
    }

    #[test]
    fn test_order_type_u8_roundtrip() {
        assert_eq!(OrderType::from_u8(OrderType::Market.to_u8()), Some(OrderType::Market));
        assert_eq!(OrderType::from_u8(OrderType::Limit.to_u8()), Some(OrderType::Limit));
        assert_eq!(OrderType::from_u8(2), None);
    }

    #[test]
    fn test_order_type_predicates() {
        assert!(OrderType::Market.is_market());
        assert!(!OrderType::Market.is_limit());
        assert!(!OrderType::Market.can_rest());

        assert!(OrderType::Limit.is_limit());
        assert!(!OrderType::Limit.is_market());
        assert!(OrderType::Limit.can_rest());
    }
}
