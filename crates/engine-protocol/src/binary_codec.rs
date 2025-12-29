//! Binary encoding/decoding for engine-core messages.
//!
//! Wire format matches Zig/C client exactly. All integers are big-endian.
//!
//! # Power of Ten Compliance
//! - Rule 2: All loops bounded (none in this module).
//! - Rule 3: Zero allocation - uses fixed-size buffers.
//! - Rule 5: Assertions on all decode/encode functions.
//! - Rule 7: All slice accesses bounds-checked.
//!
//! # Wire Format
//!
//! ## Input Messages (client → server)
//!
//! ```text
//! NewOrder (27 bytes):
//!   [0]     magic (0x4D 'M')
//!   [1]     msg_type ('N' = 0x4E)
//!   [2-5]   user_id (u32 BE)
//!   [6-13]  symbol (8 bytes null-padded)
//!   [14-17] price (u32 BE)
//!   [18-21] quantity (u32 BE)
//!   [22]    side ('B' or 'S')
//!   [23-26] user_order_id (u32 BE)
//!
//! Cancel (18 bytes):
//!   [0]     magic (0x4D)
//!   [1]     msg_type ('C' = 0x43)
//!   [2-5]   user_id (u32 BE)
//!   [6-13]  symbol (8 bytes null-padded)
//!   [14-17] user_order_id (u32 BE)
//!
//! Flush (2 bytes):
//!   [0]     magic (0x4D)
//!   [1]     msg_type ('F' = 0x46)
//! ```
//!
//! ## Output Messages (server → client)
//!
//! ```text
//! Ack (18 bytes):
//!   [0]     magic (0x4D)
//!   [1]     msg_type ('A' = 0x41)
//!   [2-9]   symbol (8 bytes null-padded)
//!   [10-13] user_id (u32 BE)
//!   [14-17] user_order_id (u32 BE)
//!
//! CancelAck (18 bytes):
//!   [0]     magic (0x4D)
//!   [1]     msg_type ('X' = 0x58)
//!   [2-9]   symbol (8 bytes null-padded)
//!   [10-13] user_id (u32 BE)
//!   [14-17] user_order_id (u32 BE)
//!
//! Trade (34 bytes):
//!   [0]     magic (0x4D)
//!   [1]     msg_type ('T' = 0x54)
//!   [2-9]   symbol (8 bytes)
//!   [10-13] buy_user_id (u32 BE)
//!   [14-17] buy_order_id (u32 BE)
//!   [18-21] sell_user_id (u32 BE)
//!   [22-25] sell_order_id (u32 BE)
//!   [26-29] price (u32 BE)
//!   [30-33] quantity (u32 BE)
//!
//! TopOfBook (20 bytes):
//!   [0]     magic (0x4D)
//!   [1]     msg_type ('B' = 0x42)
//!   [2-9]   symbol (8 bytes)
//!   [10]    side ('B' or 'S')
//!   [11-14] price (u32 BE)
//!   [15-18] quantity (u32 BE)
//!   [19]    padding (0)
//!
//! Reject (20 bytes):
//!   [0]     magic (0x4D)
//!   [1]     msg_type ('R' = 0x52)
//!   [2-9]   symbol (8 bytes)
//!   [10-13] user_id (u32 BE)
//!   [14-17] user_order_id (u32 BE)
//!   [18]    reason (u8)
//!   [19]    padding (0)
//! ```

use std::fmt;

use engine_core::{
    Ack, Cancel, CancelAck, InputMessage, NewOrder, OutputMessage,
    Reject, RejectReason, Side, Symbol, TopOfBook, Trade,
};

use crate::wire_types::{
    MAGIC_BYTE, SYMBOL_SIZE, WireInputType, WireOutputType,
    NEW_ORDER_WIRE_SIZE, CANCEL_WIRE_SIZE, FLUSH_WIRE_SIZE,
    ACK_WIRE_SIZE, CANCEL_ACK_WIRE_SIZE, TRADE_WIRE_SIZE,
    TOP_OF_BOOK_WIRE_SIZE, REJECT_WIRE_SIZE, MAX_OUTPUT_WIRE_SIZE,
    MAX_INPUT_WIRE_SIZE,
};

// =============================================================================
// Error Type
// =============================================================================

/// Errors that can arise when encoding/decoding a binary frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    /// Buffer too short for the expected fields.
    Truncated {
        /// Expected number of bytes.
        expected: usize,
        /// Actual number of bytes.
        got: usize,
    },
    /// Unknown or unsupported message type.
    UnknownMessageType(u8),
    /// Invalid magic byte.
    InvalidMagic(u8),
    /// Invalid side value.
    InvalidSide(u8),
    /// Invalid field value.
    InvalidField {
        /// Field name.
        field: &'static str,
        /// Invalid value.
        value: u32,
    },
    /// Output buffer too small.
    BufferTooSmall {
        /// Bytes needed.
        needed: usize,
        /// Bytes available.
        available: usize,
    },
}

// ProtocolError is Copy - no allocation
const _: () = assert!(std::mem::size_of::<ProtocolError>() <= 24);

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::Truncated { expected, got } => {
                write!(f, "buffer truncated: expected {} bytes, got {}", expected, got)
            }
            ProtocolError::UnknownMessageType(t) => {
                write!(f, "unknown message type: 0x{:02X}", t)
            }
            ProtocolError::InvalidMagic(m) => {
                write!(f, "invalid magic: got 0x{:02X}, expected 0x{:02X}", m, MAGIC_BYTE)
            }
            ProtocolError::InvalidSide(s) => {
                write!(f, "invalid side: 0x{:02X}", s)
            }
            ProtocolError::InvalidField { field, value } => {
                write!(f, "invalid {}: {}", field, value)
            }
            ProtocolError::BufferTooSmall { needed, available } => {
                write!(f, "buffer too small: need {} bytes, have {}", needed, available)
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

// =============================================================================
// Fixed-Size Encode Buffers
// =============================================================================

/// Fixed-size buffer for encoding input messages.
pub type InputEncodeBuffer = [u8; MAX_INPUT_WIRE_SIZE];

/// Fixed-size buffer for encoding output messages.
pub type OutputEncodeBuffer = [u8; MAX_OUTPUT_WIRE_SIZE];

// =============================================================================
// Decode Functions (Input)
// =============================================================================

/// Decode a single input message from a binary buffer.
///
/// # Returns
/// - `Ok((message, bytes_consumed))` on success.
/// - `Err(ProtocolError)` on failure.
pub fn decode_input(buf: &[u8]) -> Result<(InputMessage, usize), ProtocolError> {
    debug_assert!(!buf.is_empty(), "decode_input called with empty buffer");

    if buf.len() < 2 {
        return Err(ProtocolError::Truncated { expected: 2, got: buf.len() });
    }

    let magic = buf[0];
    let msg_type = buf[1];

    if magic != MAGIC_BYTE {
        return Err(ProtocolError::InvalidMagic(magic));
    }

    let wire_type = WireInputType::from_u8(msg_type)
        .ok_or(ProtocolError::UnknownMessageType(msg_type))?;

    match wire_type {
        WireInputType::NewOrder => decode_new_order(buf),
        WireInputType::Cancel => decode_cancel(buf),
        WireInputType::Flush => Ok((InputMessage::Flush, FLUSH_WIRE_SIZE)),
    }
}

fn decode_new_order(buf: &[u8]) -> Result<(InputMessage, usize), ProtocolError> {
    debug_assert!(buf.len() >= 2, "header already validated");
    debug_assert!(buf[0] == MAGIC_BYTE && buf[1] == b'N');

    if buf.len() < NEW_ORDER_WIRE_SIZE {
        return Err(ProtocolError::Truncated {
            expected: NEW_ORDER_WIRE_SIZE,
            got: buf.len(),
        });
    }

    let user_id = read_u32_be(&buf[2..6]);
    let symbol = read_symbol(&buf[6..14]);
    let price = read_u32_be(&buf[14..18]);
    let quantity = read_u32_be(&buf[18..22]);

    let side = match buf[22] {
        b'B' | b'b' => Side::Buy,
        b'S' | b's' => Side::Sell,
        other => return Err(ProtocolError::InvalidSide(other)),
    };

    let user_order_id = read_u32_be(&buf[23..27]);

    // Validate
    if quantity == 0 {
        return Err(ProtocolError::InvalidField {
            field: "quantity",
            value: 0,
        });
    }

    if user_order_id == 0 {
        return Err(ProtocolError::InvalidField {
            field: "user_order_id",
            value: 0,
        });
    }

    let msg = InputMessage::NewOrder(NewOrder::new(
        user_id,
        user_order_id,
        symbol,
        price,
        quantity,
        side,
    ));

    Ok((msg, NEW_ORDER_WIRE_SIZE))
}

fn decode_cancel(buf: &[u8]) -> Result<(InputMessage, usize), ProtocolError> {
    debug_assert!(buf.len() >= 2, "header already validated");
    debug_assert!(buf[0] == MAGIC_BYTE && buf[1] == b'C');

    if buf.len() < CANCEL_WIRE_SIZE {
        return Err(ProtocolError::Truncated {
            expected: CANCEL_WIRE_SIZE,
            got: buf.len(),
        });
    }

    let user_id = read_u32_be(&buf[2..6]);
    let _symbol = read_symbol(&buf[6..14]); // Present but not used
    let user_order_id = read_u32_be(&buf[14..18]);

    let msg = InputMessage::Cancel(Cancel::new(user_id, user_order_id));

    Ok((msg, CANCEL_WIRE_SIZE))
}

// =============================================================================
// Decode Functions (Output)
// =============================================================================

/// Decode a single output message from a binary buffer.
pub fn decode_output(buf: &[u8]) -> Result<(OutputMessage, usize), ProtocolError> {
    debug_assert!(!buf.is_empty(), "decode_output called with empty buffer");

    if buf.len() < 2 {
        return Err(ProtocolError::Truncated { expected: 2, got: buf.len() });
    }

    let magic = buf[0];
    let msg_type = buf[1];

    if magic != MAGIC_BYTE {
        return Err(ProtocolError::InvalidMagic(magic));
    }

    let wire_type = WireOutputType::from_u8(msg_type)
        .ok_or(ProtocolError::UnknownMessageType(msg_type))?;

    match wire_type {
        WireOutputType::Ack => decode_ack(buf),
        WireOutputType::CancelAck => decode_cancel_ack(buf),
        WireOutputType::Trade => decode_trade(buf),
        WireOutputType::TopOfBook => decode_top_of_book(buf),
        WireOutputType::Reject => decode_reject(buf),
    }
}

fn decode_ack(buf: &[u8]) -> Result<(OutputMessage, usize), ProtocolError> {
    if buf.len() < ACK_WIRE_SIZE {
        return Err(ProtocolError::Truncated {
            expected: ACK_WIRE_SIZE,
            got: buf.len(),
        });
    }

    let symbol = read_symbol(&buf[2..10]);
    let user_id = read_u32_be(&buf[10..14]);
    let user_order_id = read_u32_be(&buf[14..18]);

    Ok((OutputMessage::Ack(Ack::new(user_id, user_order_id, symbol)), ACK_WIRE_SIZE))
}

fn decode_cancel_ack(buf: &[u8]) -> Result<(OutputMessage, usize), ProtocolError> {
    if buf.len() < CANCEL_ACK_WIRE_SIZE {
        return Err(ProtocolError::Truncated {
            expected: CANCEL_ACK_WIRE_SIZE,
            got: buf.len(),
        });
    }

    let symbol = read_symbol(&buf[2..10]);
    let user_id = read_u32_be(&buf[10..14]);
    let user_order_id = read_u32_be(&buf[14..18]);

    Ok((OutputMessage::CancelAck(CancelAck::new(user_id, user_order_id, symbol)), CANCEL_ACK_WIRE_SIZE))
}

fn decode_trade(buf: &[u8]) -> Result<(OutputMessage, usize), ProtocolError> {
    if buf.len() < TRADE_WIRE_SIZE {
        return Err(ProtocolError::Truncated {
            expected: TRADE_WIRE_SIZE,
            got: buf.len(),
        });
    }

    let symbol = read_symbol(&buf[2..10]);
    let buy_user_id = read_u32_be(&buf[10..14]);
    let buy_order_id = read_u32_be(&buf[14..18]);
    let sell_user_id = read_u32_be(&buf[18..22]);
    let sell_order_id = read_u32_be(&buf[22..26]);
    let price = read_u32_be(&buf[26..30]);
    let quantity = read_u32_be(&buf[30..34]);

    let trade = Trade::new(
        symbol,
        buy_user_id,
        buy_order_id,
        sell_user_id,
        sell_order_id,
        price,
        quantity,
    );

    Ok((OutputMessage::Trade(trade), TRADE_WIRE_SIZE))
}

fn decode_top_of_book(buf: &[u8]) -> Result<(OutputMessage, usize), ProtocolError> {
    if buf.len() < TOP_OF_BOOK_WIRE_SIZE {
        return Err(ProtocolError::Truncated {
            expected: TOP_OF_BOOK_WIRE_SIZE,
            got: buf.len(),
        });
    }

    let symbol = read_symbol(&buf[2..10]);
    let side = match buf[10] {
        b'B' | b'b' => Side::Buy,
        b'S' | b's' => Side::Sell,
        other => return Err(ProtocolError::InvalidSide(other)),
    };
    let price = read_u32_be(&buf[11..15]);
    let quantity = read_u32_be(&buf[15..19]);

    let tob = if price == 0 && quantity == 0 {
        TopOfBook::eliminated(symbol, side)
    } else {
        TopOfBook::active(symbol, side, price, quantity)
    };

    Ok((OutputMessage::TopOfBook(tob), TOP_OF_BOOK_WIRE_SIZE))
}

fn decode_reject(buf: &[u8]) -> Result<(OutputMessage, usize), ProtocolError> {
    if buf.len() < REJECT_WIRE_SIZE {
        return Err(ProtocolError::Truncated {
            expected: REJECT_WIRE_SIZE,
            got: buf.len(),
        });
    }

    let symbol = read_symbol(&buf[2..10]);
    let user_id = read_u32_be(&buf[10..14]);
    let user_order_id = read_u32_be(&buf[14..18]);
    let reason_byte = buf[18];

    let reason = match reason_byte {
        1 => RejectReason::UnknownSymbol,
        2 => RejectReason::CapacityExceeded,
        3 => RejectReason::InvalidOrder,
        4 => RejectReason::DuplicateOrderId,
        _ => RejectReason::InvalidOrder, // Default fallback
    };

    Ok((OutputMessage::Reject(Reject::new(user_id, user_order_id, symbol, reason)), REJECT_WIRE_SIZE))
}

// =============================================================================
// Encode Functions (Input) - Zero Allocation
// =============================================================================

/// Encode an input message into a fixed-size buffer.
///
/// # Returns
/// - `Ok(bytes_written)` - number of bytes written to buffer.
/// - `Err(ProtocolError)` if buffer is too small.
pub fn encode_input_to_buf(
    msg: &InputMessage,
    buf: &mut [u8],
) -> Result<usize, ProtocolError> {
    match msg {
        InputMessage::NewOrder(n) => encode_new_order_to_buf(n, buf),
        InputMessage::Cancel(c) => encode_cancel_to_buf(c, buf),
        InputMessage::Flush => encode_flush_to_buf(buf),
        InputMessage::QueryTopOfBook(_) => encode_flush_to_buf(buf), // No wire format
    }
}

fn encode_new_order_to_buf(order: &NewOrder, buf: &mut [u8]) -> Result<usize, ProtocolError> {
    if buf.len() < NEW_ORDER_WIRE_SIZE {
        return Err(ProtocolError::BufferTooSmall {
            needed: NEW_ORDER_WIRE_SIZE,
            available: buf.len(),
        });
    }

    buf[0] = MAGIC_BYTE;
    buf[1] = WireInputType::NewOrder as u8;
    write_u32_be(&mut buf[2..6], order.user_id);
    write_symbol(&mut buf[6..14], &order.symbol);
    write_u32_be(&mut buf[14..18], order.price);
    write_u32_be(&mut buf[18..22], order.quantity);
    buf[22] = match order.side {
        Side::Buy => b'B',
        Side::Sell => b'S',
    };
    write_u32_be(&mut buf[23..27], order.user_order_id);

    Ok(NEW_ORDER_WIRE_SIZE)
}

fn encode_cancel_to_buf(cancel: &Cancel, buf: &mut [u8]) -> Result<usize, ProtocolError> {
    if buf.len() < CANCEL_WIRE_SIZE {
        return Err(ProtocolError::BufferTooSmall {
            needed: CANCEL_WIRE_SIZE,
            available: buf.len(),
        });
    }

    buf[0] = MAGIC_BYTE;
    buf[1] = WireInputType::Cancel as u8;
    write_u32_be(&mut buf[2..6], cancel.user_id);
    // Symbol field (8 bytes of zeros)
    buf[6..14].fill(0);
    write_u32_be(&mut buf[14..18], cancel.user_order_id);

    Ok(CANCEL_WIRE_SIZE)
}

fn encode_flush_to_buf(buf: &mut [u8]) -> Result<usize, ProtocolError> {
    if buf.len() < FLUSH_WIRE_SIZE {
        return Err(ProtocolError::BufferTooSmall {
            needed: FLUSH_WIRE_SIZE,
            available: buf.len(),
        });
    }

    buf[0] = MAGIC_BYTE;
    buf[1] = WireInputType::Flush as u8;

    Ok(FLUSH_WIRE_SIZE)
}

// =============================================================================
// Encode Functions (Output) - Zero Allocation
// =============================================================================

/// Encode an output message into a fixed-size buffer.
///
/// # Returns
/// - `Ok(bytes_written)` - number of bytes written to buffer.
/// - `Err(ProtocolError)` if buffer is too small.
pub fn encode_output_to_buf(
    msg: &OutputMessage,
    buf: &mut [u8],
) -> Result<usize, ProtocolError> {
    match msg {
        OutputMessage::Ack(a) => encode_ack_to_buf(a, buf),
        OutputMessage::CancelAck(c) => encode_cancel_ack_to_buf(c, buf),
        OutputMessage::Trade(t) => encode_trade_to_buf(t, buf),
        OutputMessage::TopOfBook(tob) => encode_top_of_book_to_buf(tob, buf),
        OutputMessage::Reject(r) => encode_reject_to_buf(r, buf),
    }
}

fn encode_ack_to_buf(ack: &Ack, buf: &mut [u8]) -> Result<usize, ProtocolError> {
    if buf.len() < ACK_WIRE_SIZE {
        return Err(ProtocolError::BufferTooSmall {
            needed: ACK_WIRE_SIZE,
            available: buf.len(),
        });
    }

    buf[0] = MAGIC_BYTE;
    buf[1] = WireOutputType::Ack as u8;
    write_symbol(&mut buf[2..10], &ack.symbol);
    write_u32_be(&mut buf[10..14], ack.user_id);
    write_u32_be(&mut buf[14..18], ack.user_order_id);

    Ok(ACK_WIRE_SIZE)
}

fn encode_cancel_ack_to_buf(ack: &CancelAck, buf: &mut [u8]) -> Result<usize, ProtocolError> {
    if buf.len() < CANCEL_ACK_WIRE_SIZE {
        return Err(ProtocolError::BufferTooSmall {
            needed: CANCEL_ACK_WIRE_SIZE,
            available: buf.len(),
        });
    }

    buf[0] = MAGIC_BYTE;
    buf[1] = WireOutputType::CancelAck as u8;
    write_symbol(&mut buf[2..10], &ack.symbol);
    write_u32_be(&mut buf[10..14], ack.user_id);
    write_u32_be(&mut buf[14..18], ack.user_order_id);

    Ok(CANCEL_ACK_WIRE_SIZE)
}

fn encode_trade_to_buf(trade: &Trade, buf: &mut [u8]) -> Result<usize, ProtocolError> {
    if buf.len() < TRADE_WIRE_SIZE {
        return Err(ProtocolError::BufferTooSmall {
            needed: TRADE_WIRE_SIZE,
            available: buf.len(),
        });
    }

    buf[0] = MAGIC_BYTE;
    buf[1] = WireOutputType::Trade as u8;
    write_symbol(&mut buf[2..10], &trade.symbol);
    write_u32_be(&mut buf[10..14], trade.user_id_buy);
    write_u32_be(&mut buf[14..18], trade.user_order_id_buy);
    write_u32_be(&mut buf[18..22], trade.user_id_sell);
    write_u32_be(&mut buf[22..26], trade.user_order_id_sell);
    write_u32_be(&mut buf[26..30], trade.price);
    write_u32_be(&mut buf[30..34], trade.quantity);

    Ok(TRADE_WIRE_SIZE)
}

fn encode_top_of_book_to_buf(tob: &TopOfBook, buf: &mut [u8]) -> Result<usize, ProtocolError> {
    if buf.len() < TOP_OF_BOOK_WIRE_SIZE {
        return Err(ProtocolError::BufferTooSmall {
            needed: TOP_OF_BOOK_WIRE_SIZE,
            available: buf.len(),
        });
    }

    buf[0] = MAGIC_BYTE;
    buf[1] = WireOutputType::TopOfBook as u8;
    write_symbol(&mut buf[2..10], &tob.symbol);
    buf[10] = match tob.side {
        Side::Buy => b'B',
        Side::Sell => b'S',
    };

    let (price, qty) = if tob.eliminated {
        (0u32, 0u32)
    } else {
        (tob.price, tob.total_quantity)
    };

    write_u32_be(&mut buf[11..15], price);
    write_u32_be(&mut buf[15..19], qty);
    buf[19] = 0; // padding

    Ok(TOP_OF_BOOK_WIRE_SIZE)
}

fn encode_reject_to_buf(reject: &Reject, buf: &mut [u8]) -> Result<usize, ProtocolError> {
    if buf.len() < REJECT_WIRE_SIZE {
        return Err(ProtocolError::BufferTooSmall {
            needed: REJECT_WIRE_SIZE,
            available: buf.len(),
        });
    }

    buf[0] = MAGIC_BYTE;
    buf[1] = WireOutputType::Reject as u8;
    write_symbol(&mut buf[2..10], &reject.symbol);
    write_u32_be(&mut buf[10..14], reject.user_id);
    write_u32_be(&mut buf[14..18], reject.user_order_id);
    buf[18] = reject.reason as u8;
    buf[19] = 0; // padding

    Ok(REJECT_WIRE_SIZE)
}

// =============================================================================
// Legacy API (allocating - for compatibility)
// =============================================================================

/// Decode input from buffer (legacy API).
#[inline]
pub fn decode_input_legacy(buf: &[u8]) -> Result<InputMessage, ProtocolError> {
    decode_input(buf).map(|(msg, _)| msg)
}

/// Decode output from buffer (legacy API).
#[inline]
pub fn decode_output_legacy(buf: &[u8]) -> Result<OutputMessage, ProtocolError> {
    decode_output(buf).map(|(msg, _)| msg)
}

/// Encode input to Vec (legacy allocating API).
pub fn encode_input(msg: &InputMessage, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let mut buf = [0u8; MAX_INPUT_WIRE_SIZE];
    let len = encode_input_to_buf(msg, &mut buf)?;
    out.extend_from_slice(&buf[..len]);
    Ok(())
}

/// Encode output to Vec (legacy allocating API).
pub fn encode_output(msg: &OutputMessage, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let mut buf = [0u8; MAX_OUTPUT_WIRE_SIZE];
    let len = encode_output_to_buf(msg, &mut buf)?;
    out.extend_from_slice(&buf[..len]);
    Ok(())
}

// =============================================================================
// Encoder/Decoder Wrapper Types
// =============================================================================

/// Zero-allocation binary encoder.
#[derive(Debug)]
pub struct BinaryEncoder {
    /// Fixed output buffer.
    buf: OutputEncodeBuffer,
}

impl Default for BinaryEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryEncoder {
    /// Create a new encoder.
    pub const fn new() -> Self {
        BinaryEncoder {
            buf: [0u8; MAX_OUTPUT_WIRE_SIZE],
        }
    }

    /// Encode an input message, returning a slice into internal buffer.
    pub fn encode_input(&mut self, msg: &InputMessage) -> Result<&[u8], ProtocolError> {
        let len = encode_input_to_buf(msg, &mut self.buf)?;
        Ok(&self.buf[..len])
    }

    /// Encode an output message, returning a slice into internal buffer.
    pub fn encode_output(&mut self, msg: &OutputMessage) -> Result<&[u8], ProtocolError> {
        let len = encode_output_to_buf(msg, &mut self.buf)?;
        Ok(&self.buf[..len])
    }

    /// Get the internal buffer.
    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }
}

/// Binary decoder (stateless).
#[derive(Debug, Default, Clone, Copy)]
pub struct BinaryDecoder;

impl BinaryDecoder {
    /// Create a new decoder.
    pub const fn new() -> Self {
        BinaryDecoder
    }

    /// Decode an input message.
    pub fn decode_input(&self, data: &[u8]) -> Result<(InputMessage, usize), ProtocolError> {
        decode_input(data)
    }

    /// Decode an output message.
    pub fn decode_output(&self, data: &[u8]) -> Result<(OutputMessage, usize), ProtocolError> {
        decode_output(data)
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Read big-endian u32 from exactly 4 bytes.
#[inline]
fn read_u32_be(bytes: &[u8]) -> u32 {
    debug_assert!(bytes.len() >= 4, "read_u32_be: need 4 bytes");
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Write big-endian u32 to exactly 4 bytes.
#[inline]
fn write_u32_be(buf: &mut [u8], val: u32) {
    debug_assert!(buf.len() >= 4, "write_u32_be: need 4 bytes");
    let bytes = val.to_be_bytes();
    buf[0] = bytes[0];
    buf[1] = bytes[1];
    buf[2] = bytes[2];
    buf[3] = bytes[3];
}

/// Read a null-padded symbol from exactly SYMBOL_SIZE bytes.
#[inline]
fn read_symbol(bytes: &[u8]) -> Symbol {
    debug_assert!(bytes.len() == SYMBOL_SIZE, "read_symbol: need {} bytes", SYMBOL_SIZE);

    // Create fixed array
    let mut arr = [0u8; SYMBOL_SIZE];
    arr.copy_from_slice(bytes);

    Symbol::from_bytes_exact(arr)
}

/// Write a symbol as SYMBOL_SIZE bytes, null-padded.
#[inline]
fn write_symbol(buf: &mut [u8], symbol: &Symbol) {
    debug_assert!(buf.len() >= SYMBOL_SIZE, "write_symbol: need {} bytes", SYMBOL_SIZE);
    buf[..SYMBOL_SIZE].copy_from_slice(symbol.as_bytes());
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_new_order_zig_format() {
        let buf: [u8; 27] = [
            0x4D, 0x4E,             // M, N
            0x00, 0x00, 0x00, 0x01, // user_id = 1
            b'I', b'B', b'M', 0, 0, 0, 0, 0, // symbol = "IBM"
            0x00, 0x00, 0x00, 0x64, // price = 100
            0x00, 0x00, 0x00, 0x32, // quantity = 50
            b'B',                   // side = Buy
            0x00, 0x00, 0x00, 0x01, // order_id = 1
        ];

        let (msg, len) = decode_input(&buf).unwrap();
        assert_eq!(len, NEW_ORDER_WIRE_SIZE);

        match msg {
            InputMessage::NewOrder(o) => {
                assert_eq!(o.user_id, 1);
                assert_eq!(o.user_order_id, 1);
                assert_eq!(o.symbol.as_str(), "IBM");
                assert_eq!(o.price, 100);
                assert_eq!(o.quantity, 50);
                assert_eq!(o.side, Side::Buy);
            }
            _ => panic!("Expected NewOrder"),
        }
    }

    #[test]
    fn test_roundtrip_new_order() {
        let order = NewOrder::new(42, 1001, Symbol::from_str("GOOG"), 500, 10, Side::Sell);
        let msg = InputMessage::NewOrder(order);

        let mut buf = [0u8; MAX_INPUT_WIRE_SIZE];
        let len = encode_input_to_buf(&msg, &mut buf).unwrap();

        let (decoded, decoded_len) = decode_input(&buf[..len]).unwrap();
        assert_eq!(len, decoded_len);

        match decoded {
            InputMessage::NewOrder(o) => {
                assert_eq!(o.user_id, 42);
                assert_eq!(o.user_order_id, 1001);
                assert_eq!(o.symbol.as_str(), "GOOG");
                assert_eq!(o.price, 500);
                assert_eq!(o.quantity, 10);
                assert_eq!(o.side, Side::Sell);
            }
            _ => panic!("Expected NewOrder"),
        }
    }

    #[test]
    fn test_roundtrip_trade() {
        let trade = Trade::new(
            Symbol::from_str("AAPL"),
            1, 100,  // buy
            2, 200,  // sell
            15000, 50,
        );
        let msg = OutputMessage::Trade(trade);

        let mut buf = [0u8; MAX_OUTPUT_WIRE_SIZE];
        let len = encode_output_to_buf(&msg, &mut buf).unwrap();

        let (decoded, decoded_len) = decode_output(&buf[..len]).unwrap();
        assert_eq!(len, decoded_len);

        match decoded {
            OutputMessage::Trade(t) => {
                assert_eq!(t.symbol.as_str(), "AAPL");
                assert_eq!(t.user_id_buy, 1);
                assert_eq!(t.user_order_id_buy, 100);
                assert_eq!(t.price, 15000);
                assert_eq!(t.quantity, 50);
            }
            _ => panic!("Expected Trade"),
        }
    }

    #[test]
    fn test_encoder_reuse() {
        let mut encoder = BinaryEncoder::new();

        // Encode multiple messages, reusing encoder
        let ack = OutputMessage::ack(1, 100, Symbol::from_str("IBM"));
        let bytes1 = encoder.encode_output(&ack).unwrap();
        assert_eq!(bytes1.len(), ACK_WIRE_SIZE);

        let trade = OutputMessage::trade(
            Symbol::from_str("X"), 1, 1, 2, 2, 100, 10
        );
        let bytes2 = encoder.encode_output(&trade).unwrap();
        assert_eq!(bytes2.len(), TRADE_WIRE_SIZE);
    }

    #[test]
    fn test_invalid_magic() {
        let buf = [0x00, b'N', 0, 0];
        let result = decode_input(&buf);
        assert!(matches!(result, Err(ProtocolError::InvalidMagic(0x00))));
    }

    #[test]
    fn test_truncated_message() {
        let buf = [MAGIC_BYTE, b'N', 0, 0]; // Too short for NewOrder
        let result = decode_input(&buf);
        assert!(matches!(result, Err(ProtocolError::Truncated { .. })));
    }

    #[test]
    fn test_buffer_too_small() {
        let mut small_buf = [0u8; 5];
        let order = NewOrder::new(1, 1, Symbol::from_str("X"), 100, 10, Side::Buy);
        let result = encode_input_to_buf(&InputMessage::NewOrder(order), &mut small_buf);
        assert!(matches!(result, Err(ProtocolError::BufferTooSmall { .. })));
    }
}
