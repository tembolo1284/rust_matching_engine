//! Low-level wire types and constants.
//!
//! Wire format matches the Zig matching engine client exactly.
//! All multi-byte integers are big-endian (network order).

/// Magic byte for binary protocol detection.
/// ASCII 'M' for "Matching engine" - first byte of every binary message.
pub const MAGIC_BYTE: u8 = 0x4D; // 'M'

/// Fixed symbol size on wire (null-padded).
pub const SYMBOL_SIZE: usize = 8;

/// Input message types (client → server).
/// Uses ASCII characters matching Zig protocol.
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WireInputType {
    NewOrder = b'N',    // 0x4E
    Cancel = b'C',      // 0x43
    Flush = b'F',       // 0x46
}

impl WireInputType {
    /// Parse from u8 wire format.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            b'N' => Some(WireInputType::NewOrder),
            b'C' => Some(WireInputType::Cancel),
            b'F' => Some(WireInputType::Flush),
            _ => None,
        }
    }
}

/// Output message types (server → client).
/// Uses ASCII characters matching Zig protocol.
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WireOutputType {
    Ack = b'A',         // 0x41
    CancelAck = b'X',   // 0x58
    Trade = b'T',       // 0x54
    TopOfBook = b'B',   // 0x42
}

impl WireOutputType {
    /// Parse from u8 wire format.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            b'A' => Some(WireOutputType::Ack),
            b'X' => Some(WireOutputType::CancelAck),
            b'T' => Some(WireOutputType::Trade),
            b'B' => Some(WireOutputType::TopOfBook),
            _ => None,
        }
    }
}

// Legacy exports for compatibility
pub const MAX_SYMBOL_LEN: usize = SYMBOL_SIZE;
pub const PROTOCOL_VERSION: u8 = 1; // Not used in Zig format but kept for reference

/// A tiny helper for validating symbol lengths.
pub fn validate_symbol_len(len: usize) -> bool {
    len > 0 && len <= MAX_SYMBOL_LEN
}
