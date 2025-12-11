//! Binary encoding/decoding for engine-core messages.
//!
//! This module converts between:
//! - raw binary frames (`&[u8]`)
//! - high-level `engine_core::InputMessage` / `OutputMessage`
//!
//! Frame format (single-message buffer):
//!
//! ```text
//! All frames start with:
//! [0]   : magic = 0x4D ('M')
//! [1]   : msg_type
//! [2]   : version (PROTOCOL_VERSION)
//! [3]   : reserved = 0
//! [4..] : body (depends on msg_type)
//!
//! Input (client → server)
//! -----------------------
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
    validate_symbol_len, MAX_SYMBOL_LEN, MAGIC_BYTE, PROTOCOL_VERSION, WireInputType, WireOutputType,
};

/// Header size: magic(1) + type(1) + version(1) + reserved(1) = 4 bytes
pub const HEADER_SIZE: usize = 4;

/// Errors that can arise when encoding/decoding a binary frame.
#[derive(Debug)]
pub enum ProtocolError {
    /// Buffer too short for the expected fields.
    Truncated,
    /// Unknown or unsupported message type.
    UnknownMessageType(u8),
    /// Unsupported or mismatched protocol version.
    VersionMismatch(u8),
    /// Invalid magic byte.
    InvalidMagic(u8),
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
            ProtocolError::InvalidMagic(m) => {
                write!(f, "Invalid magic byte: got 0x{:02X}, expected 0x{:02X}", m, MAGIC_BYTE)
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
    if buf.len() < HEADER_SIZE {
        return Err(ProtocolError::Truncated);
    }

    let magic = buf[0];
    let msg_type = buf[1];
    let version = buf[2];
    // buf[3] is reserved

    if magic != MAGIC_BYTE {
        return Err(ProtocolError::InvalidMagic(magic));
    }

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
    // Header(4) + user_id(4) + order_id(4) + price(4) + qty(4) + side(1) + sym_len(1) = 22
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
    // Header(4) + user_id(4) + order_id(4) = 12
    if buf.len() < 12 {
        return Err(ProtocolError::Truncated);
    }

    let user_id = read_u32_be(&buf[4..8]);
    let user_order_id = read_u32_be(&buf[8..12]);

    Ok(InputMessage::Cancel(Cancel::new(user_id, user_order_id)))
}

fn decode_query_tob(buf: &[u8]) -> Result<InputMessage, ProtocolError> {
    // Header(4) + sym_len(1) = 5
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

/// Write the standard header: magic, msg_type, version, reserved
fn write_header(out: &mut Vec<u8>, msg_type: u8) {
    out.push(MAGIC_BYTE);
    out.push(msg_type);
    out.push(PROTOCOL_VERSION);
    out.push(0); // reserved
}

fn encode_input_new_order(n: &NewOrder, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let symbol_len = n.symbol.len();
    if symbol_len == 0 || symbol_len > MAX_SYMBOL_LEN {
        return Err(ProtocolError::InvalidSymbol);
    }

    write_header(out, WireInputType::NewOrder as u8);

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
    write_header(out, WireInputType::Cancel as u8);

    out.extend_from_slice(&c.user_id.to_be_bytes());
    out.extend_from_slice(&c.user_order_id.to_be_bytes());

    Ok(())
}

fn encode_input_flush(out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    write_header(out, WireInputType::Flush as u8);
    Ok(())
}

fn encode_input_query_tob(q: &TopOfBookQuery, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let symbol_len = q.symbol.len();
    if symbol_len == 0 || symbol_len > MAX_SYMBOL_LEN {
        return Err(ProtocolError::InvalidSymbol);
    }

    write_header(out, WireInputType::QueryTopOfBook as u8);

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
    if buf.len() < HEADER_SIZE {
        return Err(ProtocolError::Truncated);
    }

    let magic = buf[0];
    let msg_type = buf[1];
    let version = buf[2];

    if magic != MAGIC_BYTE {
        return Err(ProtocolError::InvalidMagic(magic));
    }

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

    write_header(out, WireOutputType::Ack as u8);

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

    write_header(out, WireOutputType::CancelAck as u8);

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

    write_header(out, WireOutputType::Trade as u8);

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

    write_header(out, WireOutputType::TopOfBook as u8);

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
    // Header(4) + user_id(4) + order_id(4) + sym_len(1) = 13
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
    // Header(4) + user_id(4) + order_id(4) + sym_len(1) = 13
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
    // Header(4) + sym_len(1) = 5 minimum
    if buf.len() < 5 {
        return Err(ProtocolError::Truncated);
    }

    let symbol_len = buf[4] as usize;
    if !validate_symbol_len(symbol_len) {
        return Err(ProtocolError::InvalidSymbol);
    }

    // Header(4) + sym_len(1) + symbol + 6*u32(24) = 5 + symbol_len + 24
    if buf.len() < 5 + symbol_len + 24 {
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
    // Header(4) + sym_len(1) = 5 minimum
    if buf.len() < 5 {
        return Err(ProtocolError::Truncated);
    }

    let symbol_len = buf[4] as usize;
    if !validate_symbol_len(symbol_len) {
        return Err(ProtocolError::InvalidSymbol);
    }

    // Header(4) + sym_len(1) + symbol + side(1) + elim(1) + price(4) + qty(4) = 5 + symbol_len + 10
    if buf.len() < 5 + symbol_len + 10 {
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
    /// - [0]: magic ('M')
    /// - [1]: msg_type
    /// - [2]: version
    /// - [3]: reserved
    /// - [4..]: body (variable length based on msg_type)
    ///
    /// This returns the minimum frame size needed for the given message type.
    pub fn peek_frame_size(&self, header: &[u8]) -> Result<usize, ProtocolError> {
        if header.len() < HEADER_SIZE {
            return Err(ProtocolError::Truncated);
        }

        let magic = header[0];
        let msg_type = header[1];
        let version = header[2];

        if magic != MAGIC_BYTE {
            return Err(ProtocolError::InvalidMagic(magic));
        }

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

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_format() {
        let mut buf = Vec::new();
        write_header(&mut buf, 0);
        assert_eq!(buf[0], MAGIC_BYTE);
        assert_eq!(buf[1], 0); // msg_type
        assert_eq!(buf[2], PROTOCOL_VERSION);
        assert_eq!(buf[3], 0); // reserved
    }

    #[test]
    fn test_roundtrip_new_order() {
        let order = NewOrder::new(1, 100, Symbol::from_str("IBM"), 5000, 10, Side::Buy);
        let msg = InputMessage::NewOrder(order);

        let mut buf = Vec::new();
        encode_input(&msg, &mut buf).unwrap();

        // Verify magic byte
        assert_eq!(buf[0], MAGIC_BYTE);

        let decoded = decode_input(&buf).unwrap();
        match decoded {
            InputMessage::NewOrder(o) => {
                assert_eq!(o.user_id, 1);
                assert_eq!(o.user_order_id, 100);
                assert_eq!(o.price, 5000);
                assert_eq!(o.quantity, 10);
                assert_eq!(o.side, Side::Buy);
            }
            _ => panic!("Expected NewOrder"),
        }
    }

    #[test]
    fn test_invalid_magic() {
        let buf = [0x00, 0x00, PROTOCOL_VERSION, 0x00]; // Wrong magic
        let result = decode_input(&buf);
        assert!(matches!(result, Err(ProtocolError::InvalidMagic(0x00))));
    }
}
