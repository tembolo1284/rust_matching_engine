//! Fixed-size symbol identifier for zero-allocation hot paths.
//!
//! In HFT, symbol strings are a major source of heap allocations.
//! This type stores symbols as fixed 8-byte arrays (null-padded),
//! making them `Copy` and eliminating all heap allocation.
//!

use std::fmt;
use std::hash::{Hash, Hasher};

/// Maximum symbol length in bytes.
pub const SYMBOL_MAX_LEN: usize = 8;

/// Fixed-size symbol identifier. Zero heap allocation.
///
/// Symbols shorter than 8 bytes are null-padded on the right.
/// Example: "IBM" -> `[b'I', b'B', b'M', 0, 0, 0, 0, 0]`
///
/// # Memory Layout
/// Exactly 8 bytes, `Copy`, cache-friendly.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct Symbol(pub [u8; SYMBOL_MAX_LEN]);

impl Symbol {
    /// Empty/unset symbol constant.
    pub const EMPTY: Symbol = Symbol([0u8; SYMBOL_MAX_LEN]);

    /// Create from a string slice.
    ///
    /// Truncates silently if longer than 8 bytes.
    /// In production, you'd validate symbol length at the protocol layer.
    #[inline]
    pub fn from_str(s: &str) -> Self {
        let mut buf = [0u8; SYMBOL_MAX_LEN];
        let bytes = s.as_bytes();
        let len = bytes.len().min(SYMBOL_MAX_LEN);
        buf[..len].copy_from_slice(&bytes[..len]);
        Symbol(buf)
    }

    /// Create from a byte slice.
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut buf = [0u8; SYMBOL_MAX_LEN];
        let len = bytes.len().min(SYMBOL_MAX_LEN);
        buf[..len].copy_from_slice(&bytes[..len]);
        Symbol(buf)
    }

    /// Get the symbol as a string slice (strips trailing nulls).
    ///
    /// # Safety
    /// Assumes the symbol contains valid UTF-8 (ASCII subset).
    /// This is guaranteed if symbols are created via `from_str`.
    #[inline]
    pub fn as_str(&self) -> &str {
        let len = self.len();
        // Safety: We only store valid UTF-8 (ASCII symbols)
        unsafe { std::str::from_utf8_unchecked(&self.0[..len]) }
    }

    /// Get the raw bytes (includes trailing nulls).
    #[inline]
    pub fn as_bytes(&self) -> &[u8; SYMBOL_MAX_LEN] {
        &self.0
    }

    /// Get the actual length (excluding trailing nulls).
    #[inline]
    pub fn len(&self) -> usize {
        self.0.iter().position(|&b| b == 0).unwrap_or(SYMBOL_MAX_LEN)
    }

    /// Check if symbol is empty/unset.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0[0] == 0
    }
}

impl Hash for Symbol {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash all 8 bytes for consistency (nulls included)
        self.0.hash(state);
    }
}

impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Symbol(\"{}\")", self.as_str())
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for Symbol {
    #[inline]
    fn from(s: &str) -> Self {
        Symbol::from_str(s)
    }
}

impl From<&[u8]> for Symbol {
    #[inline]
    fn from(bytes: &[u8]) -> Self {
        Symbol::from_bytes(bytes)
    }
}

impl From<String> for Symbol {
    #[inline]
    fn from(s: String) -> Self {
        Symbol::from_str(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_from_str() {
        let sym = Symbol::from_str("IBM");
        assert_eq!(sym.as_str(), "IBM");
        assert_eq!(sym.len(), 3);
        assert!(!sym.is_empty());
    }

    #[test]
    fn test_symbol_truncation() {
        let sym = Symbol::from_str("VERYLONGSYMBOL");
        assert_eq!(sym.len(), 8);
        assert_eq!(sym.as_str(), "VERYLONG");
    }

    #[test]
    fn test_symbol_empty() {
        let sym = Symbol::EMPTY;
        assert!(sym.is_empty());
        assert_eq!(sym.len(), 0);
        assert_eq!(sym.as_str(), "");
    }

    #[test]
    fn test_symbol_copy() {
        let sym1 = Symbol::from_str("AAPL");
        let sym2 = sym1; // Copy, not move
        assert_eq!(sym1, sym2);
    }

    #[test]
    fn test_symbol_size() {
        assert_eq!(std::mem::size_of::<Symbol>(), 8);
    }
}
