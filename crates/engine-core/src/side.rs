//! Side (Buy / Sell) for orders and top-of-book.

/// Order side: Buy or Sell.
///
/// Mirrors your C++:
/// ```cpp
/// enum class Side : char {
///     BUY  = 'B',
///     SELL = 'S'
/// };
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    /// Convert to the legacy char representation (`'B'` / `'S'`),
    /// useful if you keep CSV output anywhere.
    pub fn as_char(self) -> char {
        match self {
            Side::Buy => 'B',
            Side::Sell => 'S',
        }
    }

    /// Try to parse from a char (`'B'` / `'S'`, case-sensitive).
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'B' => Some(Side::Buy),
            'S' => Some(Side::Sell),
            _ => None,
        }
    }
}

