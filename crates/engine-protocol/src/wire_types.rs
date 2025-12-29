//! Low-level wire types and constants.
//!
//! Wire format matches the Zig/C matching engine exactly.
//! All multi-byte integers are big-endian (network order).
//!
//! # Power of Ten Compliance
//! - Rule 3: No dynamic allocation.
//! - Rule 5: Assertions where meaningful.
//! - Rule 10: Compile-time size verification.

/// Magic byte for binary protocol detection.
/// ASCII 'M' for "Matching engine" - first byte of every binary message.
pub const MAGIC_BYTE: u8 = 0x4D; // 'M'

/// Fixed symbol size on wire (null-padded).
pub const SYMBOL_SIZE: usize = 8;

/// Maximum message size (prevents buffer overflow).
pub const MAX_MESSAGE_SIZE: usize = 64;

/// Protocol version (for future compatibility).
pub const PROTOCOL_VERSION: u8 = 1;

// =============================================================================
// Wire Message Sizes (compile-time constants)
// =============================================================================

/// NewOrder message size on wire.
pub const NEW_ORDER_WIRE_SIZE: usize = 27;

/// Cancel message size on wire.
pub const CANCEL_WIRE_SIZE: usize = 18;

/// Flush message size on wire.
pub const FLUSH_WIRE_SIZE: usize = 2;

/// Ack message size on wire.
pub const ACK_WIRE_SIZE: usize = 18;

/// CancelAck message size on wire.
pub const CANCEL_ACK_WIRE_SIZE: usize = 18;

/// Trade message size on wire.
pub const TRADE_WIRE_SIZE: usize = 34;

/// TopOfBook message size on wire.
pub const TOP_OF_BOOK_WIRE_SIZE: usize = 20;

/// Reject message size on wire (new).
pub const REJECT_WIRE_SIZE: usize = 20;

/// Maximum output message size (for buffer sizing).
pub const MAX_OUTPUT_WIRE_SIZE: usize = TRADE_WIRE_SIZE; // Trade is largest

/// Maximum input message size (for buffer sizing).
pub const MAX_INPUT_WIRE_SIZE: usize = NEW_ORDER_WIRE_SIZE;

// Compile-time verification
const _: () = assert!(NEW_ORDER_WIRE_SIZE <= MAX_MESSAGE_SIZE);
const _: () = assert!(CANCEL_WIRE_SIZE <= MAX_MESSAGE_SIZE);
const _: () = assert!(TRADE_WIRE_SIZE <= MAX_MESSAGE_SIZE);
const _: () = assert!(MAX_OUTPUT_WIRE_SIZE == 34);
const _: () = assert!(MAX_INPUT_WIRE_SIZE == 27);

// =============================================================================
// Input Message Types (client → server)
// =============================================================================

/// Input message types (client → server).
/// Uses ASCII characters matching Zig/C protocol.
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum WireInputType {
    /// New order submission.
    NewOrder = b'N',    // 0x4E
    /// Cancel existing order.
    Cancel = b'C',      // 0x43
    /// Flush all order books.
    Flush = b'F',       // 0x46
}

const _: () = assert!(std::mem::size_of::<WireInputType>() == 1);

impl WireInputType {
    /// All valid input types.
    pub const ALL: [WireInputType; 3] = [
        WireInputType::NewOrder,
        WireInputType::Cancel,
        WireInputType::Flush,
    ];

    /// Parse from u8 wire format.
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            b'N' => Some(WireInputType::NewOrder),
            b'C' => Some(WireInputType::Cancel),
            b'F' => Some(WireInputType::Flush),
            _ => None,
        }
    }

    /// Convert to u8 for wire format.
    #[inline]
    pub const fn to_u8(self) -> u8 {
        self as u8
    }

    /// Get the expected message size for this type.
    #[inline]
    pub const fn wire_size(self) -> usize {
        match self {
            WireInputType::NewOrder => NEW_ORDER_WIRE_SIZE,
            WireInputType::Cancel => CANCEL_WIRE_SIZE,
            WireInputType::Flush => FLUSH_WIRE_SIZE,
        }
    }

    /// Check if a buffer is large enough for this message type.
    #[inline]
    pub const fn fits_in(self, buf_len: usize) -> bool {
        buf_len >= self.wire_size()
    }
}

// =============================================================================
// Output Message Types (server → client)
// =============================================================================

/// Output message types (server → client).
/// Uses ASCII characters matching Zig/C protocol.
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum WireOutputType {
    /// Order acknowledgement.
    Ack = b'A',         // 0x41
    /// Cancel acknowledgement.
    CancelAck = b'X',   // 0x58
    /// Trade execution.
    Trade = b'T',       // 0x54
    /// Top of book update.
    TopOfBook = b'B',   // 0x42
    /// Order rejection (new).
    Reject = b'R',      // 0x52
}

const _: () = assert!(std::mem::size_of::<WireOutputType>() == 1);

impl WireOutputType {
    /// All valid output types.
    pub const ALL: [WireOutputType; 5] = [
        WireOutputType::Ack,
        WireOutputType::CancelAck,
        WireOutputType::Trade,
        WireOutputType::TopOfBook,
        WireOutputType::Reject,
    ];

    /// Parse from u8 wire format.
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            b'A' => Some(WireOutputType::Ack),
            b'X' => Some(WireOutputType::CancelAck),
            b'T' => Some(WireOutputType::Trade),
            b'B' => Some(WireOutputType::TopOfBook),
            b'R' => Some(WireOutputType::Reject),
            _ => None,
        }
    }

    /// Convert to u8 for wire format.
    #[inline]
    pub const fn to_u8(self) -> u8 {
        self as u8
    }

    /// Get the expected message size for this type.
    #[inline]
    pub const fn wire_size(self) -> usize {
        match self {
            WireOutputType::Ack => ACK_WIRE_SIZE,
            WireOutputType::CancelAck => CANCEL_ACK_WIRE_SIZE,
            WireOutputType::Trade => TRADE_WIRE_SIZE,
            WireOutputType::TopOfBook => TOP_OF_BOOK_WIRE_SIZE,
            WireOutputType::Reject => REJECT_WIRE_SIZE,
        }
    }
}

// =============================================================================
// Validation Helpers
// =============================================================================

/// Validate symbol length.
#[inline]
pub const fn is_valid_symbol_len(len: usize) -> bool {
    len > 0 && len <= SYMBOL_SIZE
}

/// Check if a byte could be a valid magic byte.
#[inline]
pub const fn is_valid_magic(byte: u8) -> bool {
    byte == MAGIC_BYTE
}

/// Check if a buffer starts with valid binary protocol header.
#[inline]
pub fn is_binary_protocol(buf: &[u8]) -> bool {
    buf.first().copied() == Some(MAGIC_BYTE)
}

/// Minimum bytes needed to determine message type.
pub const MIN_HEADER_SIZE: usize = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wire_type_sizes() {
        assert_eq!(std::mem::size_of::<WireInputType>(), 1);
        assert_eq!(std::mem::size_of::<WireOutputType>(), 1);
    }

    #[test]
    fn test_input_type_roundtrip() {
        for &t in &WireInputType::ALL {
            assert_eq!(WireInputType::from_u8(t.to_u8()), Some(t));
        }
    }

    #[test]
    fn test_output_type_roundtrip() {
        for &t in &WireOutputType::ALL {
            assert_eq!(WireOutputType::from_u8(t.to_u8()), Some(t));
        }
    }

    #[test]
    fn test_invalid_types() {
        assert_eq!(WireInputType::from_u8(b'Z'), None);
        assert_eq!(WireOutputType::from_u8(b'Z'), None);
    }

    #[test]
    fn test_wire_sizes() {
        assert_eq!(WireInputType::NewOrder.wire_size(), 27);
        assert_eq!(WireInputType::Cancel.wire_size(), 18);
        assert_eq!(WireInputType::Flush.wire_size(), 2);
        assert_eq!(WireOutputType::Trade.wire_size(), 34);
    }

    #[test]
    fn test_is_binary_protocol() {
        assert!(is_binary_protocol(&[MAGIC_BYTE, b'N']));
        assert!(!is_binary_protocol(&[b'N', MAGIC_BYTE]));
        assert!(!is_binary_protocol(&[]));
    }
}
