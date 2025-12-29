//! Fixed-size symbol identifier for zero-allocation hot paths.
//!
//! # Design Rationale
//! In HFT, symbol strings are a major source of heap allocations.
//! This type stores symbols as fixed 8-byte arrays (null-padded),
//! making them `Copy` and eliminating all heap allocation.
//!
//! # Power of Ten Compliance
//! - Rule 3: No dynamic allocation.
//! - Rule 5: Assertions in all functions.
//! - Rule 6: Smallest scope (no global state).

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

// Compile-time size verification
const _: () = assert!(std::mem::size_of::<Symbol>() == 8, "Symbol must be 8 bytes");
const _: () = assert!(std::mem::align_of::<Symbol>() == 1, "Symbol should have natural alignment");

impl Symbol {
    /// Empty/unset symbol constant.
    pub const EMPTY: Symbol = Symbol([0u8; SYMBOL_MAX_LEN]);

    /// Maximum valid symbol length.
    pub const MAX_LEN: usize = SYMBOL_MAX_LEN;

    /// Create from a string slice.
    ///
    /// # Behavior
    /// - Truncates silently if longer than 8 bytes.
    /// - Empty strings create `Symbol::EMPTY`.
    ///
    /// # Panics (debug only)
    /// - If string contains non-ASCII characters.
    #[inline]
    pub fn from_str(s: &str) -> Self {
        // Rule 5: Precondition assertions
        debug_assert!(s.is_ascii(), "Symbol must be ASCII: {:?}", s);

        let mut buf = [0u8; SYMBOL_MAX_LEN];
        let bytes = s.as_bytes();
        let len = bytes.len().min(SYMBOL_MAX_LEN);
        buf[..len].copy_from_slice(&bytes[..len]);

        let sym = Symbol(buf);

        // Rule 5: Postcondition assertions
        debug_assert!(sym.len() == len.min(SYMBOL_MAX_LEN), "length mismatch");

        sym
    }

    /// Create from a byte slice.
    ///
    /// # Panics (debug only)
    /// - If bytes contain null in the middle.
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        debug_assert!(
            !bytes.iter().take(bytes.len().min(SYMBOL_MAX_LEN)).any(|&b| b == 0),
            "Symbol bytes cannot contain embedded nulls"
        );

        let mut buf = [0u8; SYMBOL_MAX_LEN];
        let len = bytes.len().min(SYMBOL_MAX_LEN);
        buf[..len].copy_from_slice(&bytes[..len]);

        let sym = Symbol(buf);
        debug_assert!(sym.len() == len, "length mismatch");

        sym
    }

    /// Create from exactly 8 bytes (no validation, for wire format).
    #[inline]
    pub const fn from_bytes_exact(bytes: [u8; SYMBOL_MAX_LEN]) -> Self {
        Symbol(bytes)
    }

    /// Get the symbol as a string slice (strips trailing nulls).
    ///
    /// # Returns
    /// - Valid UTF-8 string if symbol contains ASCII.
    /// - Empty string if symbol contains invalid UTF-8 (defensive).
    ///
    /// # Panics (debug only)
    /// - If symbol contains non-UTF8 bytes (indicates bug in creation).
    #[inline]
    pub fn as_str(&self) -> &str {
        let len = self.len();

        debug_assert!(
            std::str::from_utf8(&self.0[..len]).is_ok(),
            "Symbol contains invalid UTF-8: {:?}",
            &self.0[..len]
        );

        // Defensive: return empty on invalid UTF-8 in release
        std::str::from_utf8(&self.0[..len]).unwrap_or("")
    }

    /// Try to get symbol as string, returning error if invalid UTF-8.
    #[inline]
    pub fn try_as_str(&self) -> Result<&str, std::str::Utf8Error> {
        let len = self.len();
        std::str::from_utf8(&self.0[..len])
    }

    /// Get the raw bytes (includes trailing nulls).
    #[inline]
    pub const fn as_bytes(&self) -> &[u8; SYMBOL_MAX_LEN] {
        &self.0
    }

    /// Get raw bytes as slice (excludes trailing nulls).
    #[inline]
    pub fn as_bytes_trimmed(&self) -> &[u8] {
        &self.0[..self.len()]
    }

    /// Get the actual length (excluding trailing nulls).
    #[inline]
    pub fn len(&self) -> usize {
        // Find first null byte
        self.0.iter().position(|&b| b == 0).unwrap_or(SYMBOL_MAX_LEN)
    }

    /// Check if symbol is empty/unset.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.0[0] == 0
    }

    /// Check if symbol is valid (non-empty, ASCII).
    #[inline]
    pub fn is_valid(&self) -> bool {
        !self.is_empty() && self.0.iter().all(|&b| b == 0 || b.is_ascii_graphic())
    }

    /// Compare symbols for sorting (lexicographic on raw bytes).
    #[inline]
    pub fn cmp_bytes(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }

    /// Get first character (for routing, e.g., A-M vs N-Z).
    #[inline]
    pub const fn first_char(&self) -> u8 {
        self.0[0]
    }

    /// Check if symbol starts with letter in range [A-M] (for dual-processor routing).
    #[inline]
    pub const fn is_a_to_m(&self) -> bool {
        let c = self.0[0];
        (c >= b'A' && c <= b'M') || (c >= b'a' && c <= b'm')
    }

    /// Check if symbol starts with letter in range [N-Z].
    #[inline]
    pub const fn is_n_to_z(&self) -> bool {
        let c = self.0[0];
        (c >= b'N' && c <= b'Z') || (c >= b'n' && c <= b'z')
    }

    /// Branchless processor routing: returns 0 for A-M, 1 for N-Z.
    /// Matches C version's branchless routing.
    #[inline]
    pub const fn processor_id(&self) -> usize {
        self.is_n_to_z() as usize
    }
}

impl Hash for Symbol {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash all 8 bytes for consistency (nulls included)
        // This matches HashMap behavior and avoids recomputing length
        self.0.hash(state);
    }
}

impl Ord for Symbol {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for Symbol {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
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

impl From<[u8; SYMBOL_MAX_LEN]> for Symbol {
    #[inline]
    fn from(bytes: [u8; SYMBOL_MAX_LEN]) -> Self {
        Symbol::from_bytes_exact(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_size() {
        assert_eq!(std::mem::size_of::<Symbol>(), 8);
    }

    #[test]
    fn test_symbol_from_str() {
        let sym = Symbol::from_str("IBM");
        assert_eq!(sym.as_str(), "IBM");
        assert_eq!(sym.len(), 3);
        assert!(!sym.is_empty());
        assert!(sym.is_valid());
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
        assert!(!sym.is_valid());
    }

    #[test]
    fn test_symbol_copy() {
        let sym1 = Symbol::from_str("AAPL");
        let sym2 = sym1; // Copy, not move
        assert_eq!(sym1, sym2);
    }

    #[test]
    fn test_symbol_routing() {
        // A-M symbols
        assert!(Symbol::from_str("AAPL").is_a_to_m());
        assert!(Symbol::from_str("IBM").is_a_to_m());
        assert!(Symbol::from_str("META").is_a_to_m());
        assert_eq!(Symbol::from_str("IBM").processor_id(), 0);

        // N-Z symbols
        assert!(Symbol::from_str("NVDA").is_n_to_z());
        assert!(Symbol::from_str("TSLA").is_n_to_z());
        assert!(Symbol::from_str("ZM").is_n_to_z());
        assert_eq!(Symbol::from_str("NVDA").processor_id(), 1);
    }

    #[test]
    fn test_symbol_ord() {
        let a = Symbol::from_str("AAPL");
        let b = Symbol::from_str("MSFT");
        let c = Symbol::from_str("AAPL");

        assert!(a < b);
        assert!(b > a);
        assert_eq!(a, c);
    }

    #[test]
    fn test_symbol_bytes_exact() {
        let bytes = [b'T', b'E', b'S', b'T', 0, 0, 0, 0];
        let sym = Symbol::from_bytes_exact(bytes);
        assert_eq!(sym.as_str(), "TEST");
    }
}
