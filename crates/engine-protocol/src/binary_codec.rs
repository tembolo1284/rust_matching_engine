//! Binary encoding/decoding for engine-core messages.
//!
//! This module converts between:
//! - raw binary frames (`&[u8]`)
//! - high-level `engine_core::InputMessage` / `OutputMessage`
//!
//! Framing model (single-message buffer):
//!
//! ```text
//! Input (client → server)
//! -----------------------
//! [0]   : msg_type (WireInputType as u8)
//! [1]   : version  (PROTOCOL_VERSION)
//! [2..4]: reserved = 0
//! [4..] : body (depends on msg_type)
//!
//! NewOrder (type=0):
//!   [4..8]   user_id (u32 BE)
//!   [8..12]  user_order_id (u32 BE)
//!   [12..16] price (u32 BE)
//!   [16..20] quantity (u32 BE)
//!   [20]     side (0=Buy, 1=Sell)
//!   [21]     symbol_len (u8, 1..=MAX_SYMBOL_LEN)
//!   [22..]   symbol bytes (UTF-8)
//!
//! Cancel (type=1):
//!   [4..8]   user_id (u32 BE)
//!   [8..12]  user_order_id (u32 BE)
//!
//! Flush (type=2):
//!   [no body]
//!
//! QueryTopOfBook (type=3):
//!   [4]      symbol_len (u8, 1..=MAX_SYMBOL_LEN)
//!   [5..]    symbol bytes
//!
//! Output (server → client)
//! ------------------------
//! [0]   : msg_type (WireOutputType as u8)
//! [1]   : version
//! [2..4]: reserved = 0
//! [4..] : body
//!
//! Ack (type=10):
//!   [4..8]   user_id (u32 BE)
//!   [8..12]  user_order_id (u32 BE)
//!   [12]     symbol_len (u8)
//!   [13..]   symbol
//!
//! CancelAck (type=11):
//!   [4..8]   user_id (u32 BE)
//!   [8..12]  user_order_id (u32 BE)
//!   [12]     symbol_len (u8)
//!   [13..]   symbol
//!
//! Trade (type=12):
//!   [4]      symbol_len (u8)
//!   [5..]    symbol
//!   [...+4]  user_id_buy (u32 BE)
//!   [...+4]  user_order_id_buy (u32 BE)
//!   [...+4]  user_id_sell (u32 BE)
//!   [...+4]  user_order_id_sell (u32 BE)
//!   [...+4]  price (u32 BE)
//!   [...+4]  quantity (u32 BE)
//!
//! TopOfBook (type=13):
//!   [4]      symbol_len (u8)
//!   [5..]    symbol
//!   [...+1]  side (0=Bid, 1=Ask)
//!   [...+1]  eliminated (0/1)
//!   [...+4]  price (u32 BE, ignored if eliminated)
//!   [...+4]  total_quantity (u32 BE, ignored if eliminated)
//! ```
//!
//! NOTE: This module encodes/decodes **one message per buffer**. A TCP
//! stream server is expected to provide its own framing (e.g. length-
//! prefix each frame) using these functions for the payload.

use std::convert::TryFrom;
use std::fmt;

use engine_core::{
    Ack, Cancel, CancelAck, InputMessage, NewOrder, OutputMessage, Side, Symbol, TopOfBook,
    TopOfBookQuery, Trade,
};

use crate::wire_types::{
    validate_symbol_len, MAX_SYMBOL_LEN, PROTOCOL_VERSION, WireInputType, WireOutputType,
};

/// Errors that can arise when encoding/decoding a binary frame.
#[derive(Debug)]
pub enum ProtocolError {
    /// Buffer too short for the expected fields.
    Truncated,
    /// Unknown or unsupported message type.
    UnknownMessageType(u8),
    /// Unsupported or mismatched protocol version.
    VersionMismatch(u8),
    /// Invalid symbol length or malformed UTF-8.
    InvalidSymbol,
    /// Invalid side or other semantic issue.
    InvalidField(&'static str),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::Truncated => write!(f, "Buffer truncated"),
            ProtocolError::UnknownMessageType(t) => write!(f, "Unknown message type: {}", t),
            ProtocolError::VersionMismatch(v) => {
                write!(f, "Protocol version mismatch: got {}, expected {}", v, PROTOCOL_VERSION)
            }
            ProtocolError::InvalidSymbol => write!(f, "Invalid symbol"),
            ProtocolError::InvalidField(field) => write!(f, "Invalid field: {}", field),
        }
    }
}

impl std::error::Error for ProtocolError {}

// ============================================================================
// INPUT: client → server
// ============================================================================

/// Decode a single input message from a binary buffer.
///
/// The buffer must contain exactly one full message as described above.
pub fn decode_input(buf: &[u8]) -> Result<InputMessage, ProtocolError> {
    if buf.len() < 4 {
        return Err(ProtocolError::Truncated);
    }

    let msg_type = buf[0];
    let version = buf[1];

    if version != PROTOCOL_VERSION {
        return Err(ProtocolError::VersionMismatch(version));
    }

    let wire_type =
        WireInputType::from_u8(msg_type).ok_or(ProtocolError::UnknownMessageType(msg_type))?;

    match wire_type {
        WireInputType::NewOrder => decode_new_order(buf),
        WireInputType::Cancel => decode_cancel(buf),
        WireInputType::Flush => Ok(InputMessage::Flush),
        WireInputType::QueryTopOfBook => decode_query_tob(buf),
    }
}

/// Encode a single input message into a binary frame.
///
/// The encoded bytes are appended to `out`.
pub fn encode_input(msg: &InputMessage, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    match msg {
        InputMessage::NewOrder(n) => encode_input_new_order(n, out),
        InputMessage::Cancel(c) => encode_input_cancel(c, out),
        InputMessage::Flush => encode_input_flush(out),
        InputMessage::QueryTopOfBook(q) => encode_input_query_tob(q, out),
    }
}

fn decode_new_order(buf: &[u8]) -> Result<InputMessage, ProtocolError> {
    if buf.len() < 22 {
        return Err(ProtocolError::Truncated);
    }

    let user_id = read_u32_be(&buf[4..8]);
    let user_order_id = read_u32_be(&buf[8..12]);
    let price = read_u32_be(&buf[12..16]);
    let quantity = read_u32_be(&buf[16..20]);

    let side_raw = buf[20];
    let side = match side_raw {
        0 => Side::Buy,
        1 => Side::Sell,
        _ => return Err(ProtocolError::InvalidField("side")),
    };

    let symbol_len = buf[21] as usize;
    if !validate_symbol_len(symbol_len) {
        return Err(ProtocolError::InvalidSymbol);
    }

    if buf.len() < 22 + symbol_len {
        return Err(ProtocolError::Truncated);
    }

    let symbol_bytes = &buf[22..22 + symbol_len];
    let symbol_str = std::str::from_utf8(symbol_bytes)
        .map_err(|_| ProtocolError::InvalidSymbol)?;
    let symbol = Symbol::from_str(symbol_str);

    if quantity == 0 {
        return Err(ProtocolError::InvalidField("quantity"));
    }

    Ok(InputMessage::NewOrder(NewOrder::new(
        user_id,
        user_order_id,
        symbol,
        price,
        quantity,
        side,
    )))
}

fn decode_cancel(buf: &[u8]) -> Result<InputMessage, ProtocolError> {
    if buf.len() < 12 {
        return Err(ProtocolError::Truncated);
    }

    let user_id = read_u32_be(&buf[4..8]);
    let user_order_id = read_u32_be(&buf[8..12]);

    Ok(InputMessage::Cancel(Cancel::new(user_id, user_order_id)))
}

fn decode_query_tob(buf: &[u8]) -> Result<InputMessage, ProtocolError> {
    if buf.len() < 5 {
        return Err(ProtocolError::Truncated);
    }

    let symbol_len = buf[4] as usize;
    if !validate_symbol_len(symbol_len) {
        return Err(ProtocolError::InvalidSymbol);
    }

    if buf.len() < 5 + symbol_len {
        return Err(ProtocolError::Truncated);
    }

    let symbol_bytes = &buf[5..5 + symbol_len];
    let symbol_str = std::str::from_utf8(symbol_bytes)
        .map_err(|_| ProtocolError::InvalidSymbol)?;
    let symbol = Symbol::from_str(symbol_str);

    Ok(InputMessage::QueryTopOfBook(TopOfBookQuery::new(symbol)))
}

fn encode_input_new_order(n: &NewOrder, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let symbol_len = n.symbol.len();
    if symbol_len == 0 || symbol_len > MAX_SYMBOL_LEN {
        return Err(ProtocolError::InvalidSymbol);
    }

    out.push(WireInputType::NewOrder as u8);
    out.push(PROTOCOL_VERSION);
    out.extend_from_slice(&[0, 0]); // reserved

    out.extend_from_slice(&n.user_id.to_be_bytes());
    out.extend_from_slice(&n.user_order_id.to_be_bytes());
    out.extend_from_slice(&n.price.to_be_bytes());
    out.extend_from_slice(&n.quantity.to_be_bytes());

    let side_byte = match n.side {
        Side::Buy => 0,
        Side::Sell => 1,
    };
    out.push(side_byte);

    out.push(u8::try_from(symbol_len).unwrap());
    out.extend_from_slice(&n.symbol.as_bytes()[..symbol_len]);

    Ok(())
}

fn encode_input_cancel(c: &Cancel, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    out.push(WireInputType::Cancel as u8);
    out.push(PROTOCOL_VERSION);
    out.extend_from_slice(&[0, 0]);

    out.extend_from_slice(&c.user_id.to_be_bytes());
    out.extend_from_slice(&c.user_order_id.to_be_bytes());

    Ok(())
}

fn encode_input_flush(out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    out.push(WireInputType::Flush as u8);
    out.push(PROTOCOL_VERSION);
    out.extend_from_slice(&[0, 0]);
    Ok(())
}

fn encode_input_query_tob(q: &TopOfBookQuery, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let symbol_len = q.symbol.len();
    if symbol_len == 0 || symbol_len > MAX_SYMBOL_LEN {
        return Err(ProtocolError::InvalidSymbol);
    }

    out.push(WireInputType::QueryTopOfBook as u8);
    out.push(PROTOCOL_VERSION);
    out.extend_from_slice(&[0, 0]);

    out.push(u8::try_from(symbol_len).unwrap());
    out.extend_from_slice(&q.symbol.as_bytes()[..symbol_len]);

    Ok(())
}

// ============================================================================
// OUTPUT: server → client
// ============================================================================

/// Encode a single output message into a binary frame.
///
/// The encoded bytes are appended to `out`.
pub fn encode_output(msg: &OutputMessage, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    match msg {
        OutputMessage::Ack(a) => encode_ack(a, out),
        OutputMessage::CancelAck(c) => encode_cancel_ack(c, out),
        OutputMessage::Trade(t) => encode_trade(t, out),
        OutputMessage::TopOfBook(tob) => encode_top_of_book(tob, out),
    }
}

/// Decode a single output message from a binary buffer.
///
/// This is useful on the **client** side when reading from the server.
pub fn decode_output(buf: &[u8]) -> Result<OutputMessage, ProtocolError> {
    if buf.len() < 4 {
        return Err(ProtocolError::Truncated);
    }

    let msg_type = buf[0];
    let version = buf[1];

    if version != PROTOCOL_VERSION {
        return Err(ProtocolError::VersionMismatch(version));
    }

    let wire_type =
        WireOutputType::from_u8(msg_type).ok_or(ProtocolError::UnknownMessageType(msg_type))?;

    match wire_type {
        WireOutputType::Ack => decode_ack(buf),
        WireOutputType::CancelAck => decode_cancel_ack(buf),
        WireOutputType::Trade => decode_trade(buf),
        WireOutputType::TopOfBook => decode_top_of_book(buf),
    }
}

fn encode_ack(a: &Ack, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let symbol_len = a.symbol.len();
    if symbol_len == 0 || symbol_len > MAX_SYMBOL_LEN {
        return Err(ProtocolError::InvalidSymbol);
    }

    out.push(WireOutputType::Ack as u8);
    out.push(PROTOCOL_VERSION);
    out.extend_from_slice(&[0, 0]); // reserved

    out.extend_from_slice(&a.user_id.to_be_bytes());
    out.extend_from_slice(&a.user_order_id.to_be_bytes());

    out.push(u8::try_from(symbol_len).unwrap());
    out.extend_from_slice(&a.symbol.as_bytes()[..symbol_len]);

    Ok(())
}

fn encode_cancel_ack(c: &CancelAck, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let symbol_len = c.symbol.len();
    if symbol_len == 0 || symbol_len > MAX_SYMBOL_LEN {
        return Err(ProtocolError::InvalidSymbol);
    }

    out.push(WireOutputType::CancelAck as u8);
    out.push(PROTOCOL_VERSION);
    out.extend_from_slice(&[0, 0]);

    out.extend_from_slice(&c.user_id.to_be_bytes());
    out.extend_from_slice(&c.user_order_id.to_be_bytes());

    out.push(u8::try_from(symbol_len).unwrap());
    out.extend_from_slice(&c.symbol.as_bytes()[..symbol_len]);

    Ok(())
}

fn encode_trade(t: &Trade, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let symbol_len = t.symbol.len();
    if symbol_len == 0 || symbol_len > MAX_SYMBOL_LEN {
        return Err(ProtocolError::InvalidSymbol);
    }

    out.push(WireOutputType::Trade as u8);
    out.push(PROTOCOL_VERSION);
    out.extend_from_slice(&[0, 0]);

    // symbol
    out.push(u8::try_from(symbol_len).unwrap());
    out.extend_from_slice(&t.symbol.as_bytes()[..symbol_len]);

    // fields
    out.extend_from_slice(&t.user_id_buy.to_be_bytes());
    out.extend_from_slice(&t.user_order_id_buy.to_be_bytes());
    out.extend_from_slice(&t.user_id_sell.to_be_bytes());
    out.extend_from_slice(&t.user_order_id_sell.to_be_bytes());
    out.extend_from_slice(&t.price.to_be_bytes());
    out.extend_from_slice(&t.quantity.to_be_bytes());

    Ok(())
}

fn encode_top_of_book(t: &TopOfBook, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let symbol_len = t.symbol.len();
    if symbol_len == 0 || symbol_len > MAX_SYMBOL_LEN {
        return Err(ProtocolError::InvalidSymbol);
    }

    out.push(WireOutputType::TopOfBook as u8);
    out.push(PROTOCOL_VERSION);
    out.extend_from_slice(&[0, 0]);

    // symbol
    out.push(u8::try_from(symbol_len).unwrap());
    out.extend_from_slice(&t.symbol.as_bytes()[..symbol_len]);

    // side
    let side_byte = match t.side {
        Side::Buy => 0,
        Side::Sell => 1,
    };
    out.push(side_byte);

    // eliminated
    out.push(if t.is_eliminated() { 1 } else { 0 });

    // price & qty (ignored by client if eliminated=1)
    out.extend_from_slice(&t.price.to_be_bytes());
    out.extend_from_slice(&t.total_quantity.to_be_bytes());

    Ok(())
}

fn decode_ack(buf: &[u8]) -> Result<OutputMessage, ProtocolError> {
    if buf.len() < 13 {
        return Err(ProtocolError::Truncated);
    }

    let user_id = read_u32_be(&buf[4..8]);
    let user_order_id = read_u32_be(&buf[8..12]);
    let symbol_len = buf[12] as usize;

    if !validate_symbol_len(symbol_len) || buf.len() < 13 + symbol_len {
        return Err(ProtocolError::InvalidSymbol);
    }

    let symbol_bytes = &buf[13..13 + symbol_len];
    let symbol_str = std::str::from_utf8(symbol_bytes)
        .map_err(|_| ProtocolError::InvalidSymbol)?;
    let symbol = Symbol::from_str(symbol_str);

    Ok(OutputMessage::Ack(Ack::new(user_id, user_order_id, symbol)))
}

fn decode_cancel_ack(buf: &[u8]) -> Result<OutputMessage, ProtocolError> {
    if buf.len() < 13 {
        return Err(ProtocolError::Truncated);
    }

    let user_id = read_u32_be(&buf[4..8]);
    let user_order_id = read_u32_be(&buf[8..12]);
    let symbol_len = buf[12] as usize;

    if !validate_symbol_len(symbol_len) || buf.len() < 13 + symbol_len {
        return Err(ProtocolError::InvalidSymbol);
    }

    let symbol_bytes = &buf[13..13 + symbol_len];
    let symbol_str = std::str::from_utf8(symbol_bytes)
        .map_err(|_| ProtocolError::InvalidSymbol)?;
    let symbol = Symbol::from_str(symbol_str);

    Ok(OutputMessage::CancelAck(CancelAck::new(user_id, user_order_id, symbol)))
}

fn decode_trade(buf: &[u8]) -> Result<OutputMessage, ProtocolError> {
    if buf.len() < 5 {
        return Err(ProtocolError::Truncated);
    }

    let symbol_len = buf[4] as usize;
    if !validate_symbol_len(symbol_len) {
        return Err(ProtocolError::InvalidSymbol);
    }

    if buf.len() < 5 + symbol_len + 4 * 6 {
        return Err(ProtocolError::Truncated);
    }

    let symbol_bytes = &buf[5..5 + symbol_len];
    let symbol_str = std::str::from_utf8(symbol_bytes)
        .map_err(|_| ProtocolError::InvalidSymbol)?;
    let symbol = Symbol::from_str(symbol_str);

    let mut offset = 5 + symbol_len;

    let user_id_buy = read_u32_be(&buf[offset..offset + 4]);
    offset += 4;
    let user_order_id_buy = read_u32_be(&buf[offset..offset + 4]);
    offset += 4;
    let user_id_sell = read_u32_be(&buf[offset..offset + 4]);
    offset += 4;
    let user_order_id_sell = read_u32_be(&buf[offset..offset + 4]);
    offset += 4;
    let price = read_u32_be(&buf[offset..offset + 4]);
    offset += 4;
    let quantity = read_u32_be(&buf[offset..offset + 4]);

    Ok(OutputMessage::Trade(Trade::new(
        symbol,
        user_id_buy,
        user_order_id_buy,
        user_id_sell,
        user_order_id_sell,
        price,
        quantity,
    )))
}

fn decode_top_of_book(buf: &[u8]) -> Result<OutputMessage, ProtocolError> {
    if buf.len() < 5 {
        return Err(ProtocolError::Truncated);
    }

    let symbol_len = buf[4] as usize;
    if !validate_symbol_len(symbol_len) {
        return Err(ProtocolError::InvalidSymbol);
    }

    if buf.len() < 5 + symbol_len + 1 + 1 + 4 + 4 {
        return Err(ProtocolError::Truncated);
    }

    let symbol_bytes = &buf[5..5 + symbol_len];
    let symbol_str = std::str::from_utf8(symbol_bytes)
        .map_err(|_| ProtocolError::InvalidSymbol)?;
    let symbol = Symbol::from_str(symbol_str);

    let mut offset = 5 + symbol_len;

    let side_byte = buf[offset];
    offset += 1;
    let side = match side_byte {
        0 => Side::Buy,
        1 => Side::Sell,
        _ => return Err(ProtocolError::InvalidField("side")),
    };

    let eliminated = buf[offset] != 0;
    offset += 1;

    let price = read_u32_be(&buf[offset..offset + 4]);
    offset += 4;
    let total_quantity = read_u32_be(&buf[offset..offset + 4]);

    // Use the appropriate constructor based on eliminated flag
    let tob = if eliminated {
        TopOfBook::eliminated(symbol, side)
    } else {
        TopOfBook::active(symbol, side, price, total_quantity)
    };

    Ok(OutputMessage::TopOfBook(tob))
}

// =============================================================================
// Encoder/Decoder wrapper types for stateful usage
// =============================================================================

/// Stateful binary encoder.
///
/// Wraps the encode functions with an internal buffer for convenience.
#[derive(Debug, Default)]
pub struct BinaryEncoder {
    buf: Vec<u8>,
}

impl BinaryEncoder {
    /// Create a new encoder with default buffer capacity.
    pub fn new() -> Self {
        BinaryEncoder {
            buf: Vec::with_capacity(256),
        }
    }

    /// Encode an input message, returning the bytes.
    pub fn encode_input(&mut self, msg: &InputMessage) -> Result<&[u8], ProtocolError> {
        self.buf.clear();
        encode_input(msg, &mut self.buf)?;
        Ok(&self.buf)
    }

    /// Encode an output message, returning the bytes.
    pub fn encode_output(&mut self, msg: &OutputMessage) -> Result<&[u8], ProtocolError> {
        self.buf.clear();
        encode_output(msg, &mut self.buf)?;
        Ok(&self.buf)
    }

    /// Get the internal buffer (for zero-copy access after encoding).
    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    /// Clear the internal buffer.
    pub fn clear(&mut self) {
        self.buf.clear();
    }
}

/// Stateful binary decoder.
#[derive(Debug, Default)]
pub struct BinaryDecoder;

impl BinaryDecoder {
    /// Create a new decoder.
    pub fn new() -> Self {
        BinaryDecoder
    }

    /// Decode an input message from bytes.
    pub fn decode_input(&self, data: &[u8]) -> Result<InputMessage, ProtocolError> {
        decode_input(data)
    }

    /// Decode an output message from bytes.
    pub fn decode_output(&self, data: &[u8]) -> Result<OutputMessage, ProtocolError> {
        decode_output(data)
    }

    /// Peek at frame header to determine total frame size.
    ///
    /// For our protocol, the frame structure is:
    /// - [0]: msg_type
    /// - [1]: version
    /// - [2..4]: reserved
    /// - [4..]: body (variable length based on msg_type)
    ///
    /// This returns the minimum frame size needed for the given message type.
    pub fn peek_frame_size(&self, header: &[u8]) -> Result<usize, ProtocolError> {
        if header.len() < 4 {
            return Err(ProtocolError::Truncated);
        }

        let msg_type = header[0];
        let version = header[1];

        if version != PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch(version));
        }

        // Determine size based on message type
        // These are minimum sizes; actual size depends on symbol length
        let base_size = match msg_type {
            0 => 22,  // NewOrder: header(4) + user_id(4) + user_order_id(4) + price(4) + qty(4) + side(1) + symbol_len(1) + symbol(1+)
            1 => 12,  // Cancel: header(4) + user_id(4) + user_order_id(4)
            2 => 4,   // Flush: header(4)
            3 => 5,   // QueryTopOfBook: header(4) + symbol_len(1) + symbol(1+)
            _ => return Err(ProtocolError::UnknownMessageType(msg_type)),
        };

        Ok(base_size)
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn read_u32_be(bytes: &[u8]) -> u32 {
    let arr: [u8; 4] = bytes[0..4].try_into().expect("slice with incorrect length");
    u32::from_be_bytes(arr)
}
