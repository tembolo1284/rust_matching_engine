//! Order side: Buy or Sell.
//!
//! # Power of Ten Compliance
//! - Rule 3: No dynamic allocation.
//! - Rule 5: Assertions in non-trivial functions.
//! - Rule 8: Minimal preprocessor (no complex macros).
//! - Explicit `repr(u8)` for predictable 1-byte binary representation.

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

// Compile-time size verification
const _: () = assert!(std::mem::size_of::<Side>() == 1, "Side must be 1 byte");

impl Side {
    /// All valid sides (for iteration/validation).
    pub const ALL: [Side; 2] = [Side::Buy, Side::Sell];

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
    ///
    /// # Returns
    /// - `Some(Side)` if valid (0 or 1)
    /// - `None` if invalid
    #[inline]
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Side::Buy),
            1 => Some(Side::Sell),
            _ => None,
        }
    }

    /// Parse from u8, panicking on invalid input.
    /// Use only when input is trusted (e.g., from validated protocol layer).
    #[inline]
    pub const fn from_u8_unchecked(val: u8) -> Self {
        match val {
            0 => Side::Buy,
            1 => Side::Sell,
            _ => panic!("invalid side value"),
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

    /// Check if this is the buy side.
    #[inline]
    pub const fn is_buy(self) -> bool {
        matches!(self, Side::Buy)
    }

    /// Check if this is the sell side.
    #[inline]
    pub const fn is_sell(self) -> bool {
        matches!(self, Side::Sell)
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
    fn test_side_values() {
        assert_eq!(Side::Buy as u8, 0);
        assert_eq!(Side::Sell as u8, 1);
    }

    #[test]
    fn test_side_char_roundtrip() {
        assert_eq!(Side::from_char(Side::Buy.as_char()), Some(Side::Buy));
        assert_eq!(Side::from_char(Side::Sell.as_char()), Some(Side::Sell));
        assert_eq!(Side::from_char('X'), None);
    }

    #[test]
    fn test_side_u8_roundtrip() {
        assert_eq!(Side::from_u8(Side::Buy.to_u8()), Some(Side::Buy));
        assert_eq!(Side::from_u8(Side::Sell.to_u8()), Some(Side::Sell));
        assert_eq!(Side::from_u8(2), None);
        assert_eq!(Side::from_u8(255), None);
    }

    #[test]
    fn test_side_opposite() {
        assert_eq!(Side::Buy.opposite(), Side::Sell);
        assert_eq!(Side::Sell.opposite(), Side::Buy);
        // Double opposite returns original
        assert_eq!(Side::Buy.opposite().opposite(), Side::Buy);
    }

    #[test]
    fn test_side_predicates() {
        assert!(Side::Buy.is_buy());
        assert!(!Side::Buy.is_sell());
        assert!(Side::Sell.is_sell());
        assert!(!Side::Sell.is_buy());
    }

    #[test]
    fn test_side_all() {
        assert_eq!(Side::ALL.len(), 2);
        assert!(Side::ALL.contains(&Side::Buy));
        assert!(Side::ALL.contains(&Side::Sell));
    }
}
