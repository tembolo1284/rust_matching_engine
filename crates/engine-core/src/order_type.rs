//! Order type: Market or Limit.
//!
//! # Power of Ten Compliance
//! - Explicit `repr(u8)` for predictable 1-byte binary representation.
//! - No hidden allocations or complex logic.

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

impl OrderType {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_type_size() {
        assert_eq!(std::mem::size_of::<OrderType>(), 1);
    }

    #[test]
    fn test_order_type_from_price() {
        assert_eq!(OrderType::from_price(0), OrderType::Market);
        assert_eq!(OrderType::from_price(100), OrderType::Limit);
        assert_eq!(OrderType::from_price(1), OrderType::Limit);
    }

    #[test]
    fn test_order_type_u8_roundtrip() {
        assert_eq!(OrderType::from_u8(OrderType::Market.to_u8()), Some(OrderType::Market));
        assert_eq!(OrderType::from_u8(OrderType::Limit.to_u8()), Some(OrderType::Limit));
    }
}
