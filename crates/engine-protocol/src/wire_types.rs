//! Low-level wire types and constants.
//!
//! This module defines:
//! - Message type IDs for input and output messages.
//! - Protocol versioning.
//! - Small helpers for dealing with fixed/variable-length fields.
//!
//! The actual encode/decode logic lives in `binary_codec`.

/// Current protocol version.
///
/// This can be bumped in the future if we change the framing or add
/// incompatible message variants.
pub const PROTOCOL_VERSION: u8 = 1;

/// Input message types (client → server).
///
/// These IDs are used in the first byte of each binary frame.
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WireInputType {
    /// New order (market or limit).
    NewOrder = 0,

    /// Cancel `(user_id, user_order_id)`.
    Cancel = 1,

    /// Flush all books.
    Flush = 2,

    /// Query current top-of-book for a symbol.
    QueryTopOfBook = 3,
}

impl WireInputType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(WireInputType::NewOrder),
            1 => Some(WireInputType::Cancel),
            2 => Some(WireInputType::Flush),
            3 => Some(WireInputType::QueryTopOfBook),
            _ => None,
        }
    }
}

/// Output message types (server → client).
///
/// These IDs are used in the first byte of each binary frame.
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WireOutputType {
    /// Ack a new order.
    Ack = 10,

    /// Ack a cancel request.
    CancelAck = 11,

    /// Trade between buyer and seller.
    Trade = 12,

    /// Top-of-book event (snapshot or change).
    TopOfBook = 13,
}

impl WireOutputType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            10 => Some(WireOutputType::Ack),
            11 => Some(WireOutputType::CancelAck),
            12 => Some(WireOutputType::Trade),
            13 => Some(WireOutputType::TopOfBook),
            _ => None,
        }
    }
}

/// Maximum symbol length on the wire.
///
/// For the binary protocol we can enforce a hard limit
/// (e.g. 32 bytes UTF-8) to keep framing simple. Clients
/// sending longer symbols should be rejected at the protocol
/// layer.
pub const MAX_SYMBOL_LEN: usize = 32;

/// A tiny helper for validating symbol lengths.
pub fn validate_symbol_len(len: usize) -> bool {
    len > 0 && len <= MAX_SYMBOL_LEN
}

