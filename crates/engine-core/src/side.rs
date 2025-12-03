//! Order side: Buy or Sell.
//!
//! # Power of Ten Compliance
//! - Explicit `repr(u8)` for predictable 1-byte binary representation.
//! - No hidden allocations or complex logic.

/// Order side: Buy or Sell.
///
/// # Binary Representation
/// Explicit `repr(u8)` guarantees 1-byte size and allows safe
/// transmutation from wire format.
///
/// ```text
/// Buy  = 0x00
/// Sell = 0x01
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Side {
    /// Buy side (bid).
    #[default]
    Buy = 0,
    /// Sell side (ask/offer).
    Sell = 1,
}

impl Side {
    /// Convert to legacy char representation ('B' / 'S').
    /// Useful for CSV output or human-readable logs.
    #[inline]
    pub const fn as_char(self) -> char {
        match self {
            Side::Buy => 'B',
            Side::Sell => 'S',
        }
    }

    /// Parse from char ('B' / 'S'), case-sensitive.
    #[inline]
    pub const fn from_char(c: char) -> Option<Self> {
        match c {
            'B' => Some(Side::Buy),
            'S' => Some(Side::Sell),
            _ => None,
        }
    }

    /// Parse from u8 wire format.
    #[inline]
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Side::Buy),
            1 => Some(Side::Sell),
            _ => None,
        }
    }

    /// Convert to u8 for wire format.
    #[inline]
    pub const fn to_u8(self) -> u8 {
        self as u8
    }

    /// Returns the opposite side.
    #[inline]
    pub const fn opposite(self) -> Self {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_side_size() {
        assert_eq!(std::mem::size_of::<Side>(), 1);
    }

    #[test]
    fn test_side_char_roundtrip() {
        assert_eq!(Side::from_char(Side::Buy.as_char()), Some(Side::Buy));
        assert_eq!(Side::from_char(Side::Sell.as_char()), Some(Side::Sell));
    }

    #[test]
    fn test_side_u8_roundtrip() {
        assert_eq!(Side::from_u8(Side::Buy.to_u8()), Some(Side::Buy));
        assert_eq!(Side::from_u8(Side::Sell.to_u8()), Some(Side::Sell));
    }

    #[test]
    fn test_side_opposite() {
        assert_eq!(Side::Buy.opposite(), Side::Sell);
        assert_eq!(Side::Sell.opposite(), Side::Buy);
    }
}
