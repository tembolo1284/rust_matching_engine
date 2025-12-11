//! Binary encoding/decoding for engine-core messages.
//!
//! Wire format matches Zig client exactly. All integers are big-endian.
//!
//! # Input Messages (client → server)
//!
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
//!
//! # Output Messages (server → client)
//!
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

use std::fmt;

use engine_core::{
    Ack, Cancel, CancelAck, InputMessage, NewOrder, OutputMessage, Side, Symbol, TopOfBook, Trade,
};

use crate::wire_types::{MAGIC_BYTE, SYMBOL_SIZE, WireInputType, WireOutputType};

// Message sizes
pub const NEW_ORDER_SIZE: usize = 27;
pub const CANCEL_SIZE: usize = 18;
pub const FLUSH_SIZE: usize = 2;
pub const ACK_SIZE: usize = 18;
pub const CANCEL_ACK_SIZE: usize = 18;
pub const TRADE_SIZE: usize = 34;
pub const TOP_OF_BOOK_SIZE: usize = 20;

/// Errors that can arise when encoding/decoding a binary frame.
#[derive(Debug)]
pub enum ProtocolError {
    /// Buffer too short for the expected fields.
    Truncated,
    /// Unknown or unsupported message type.
    UnknownMessageType(u8),
    /// Invalid magic byte.
    InvalidMagic(u8),
    /// Invalid symbol (empty or too long).
    InvalidSymbol,
    /// Invalid side or other semantic issue.
    InvalidField(&'static str),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::Truncated => write!(f, "Buffer truncated"),
            ProtocolError::UnknownMessageType(t) => write!(f, "Unknown message type: 0x{:02X}", t),
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
pub fn decode_input(buf: &[u8]) -> Result<InputMessage, ProtocolError> {
    if buf.len() < 2 {
        return Err(ProtocolError::Truncated);
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
        WireInputType::Flush => Ok(InputMessage::Flush),
    }
}

fn decode_new_order(buf: &[u8]) -> Result<InputMessage, ProtocolError> {
    if buf.len() < NEW_ORDER_SIZE {
        return Err(ProtocolError::Truncated);
    }

    let user_id = read_u32_be(&buf[2..6]);
    let symbol = read_symbol(&buf[6..14]);
    let price = read_u32_be(&buf[14..18]);
    let quantity = read_u32_be(&buf[18..22]);
    
    let side = match buf[22] {
        b'B' | b'b' => Side::Buy,
        b'S' | b's' => Side::Sell,
        _ => return Err(ProtocolError::InvalidField("side")),
    };
    
    let user_order_id = read_u32_be(&buf[23..27]);

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
    if buf.len() < CANCEL_SIZE {
        return Err(ProtocolError::Truncated);
    }

    let user_id = read_u32_be(&buf[2..6]);
    let _symbol = read_symbol(&buf[6..14]); // Symbol present but not used in Cancel
    let user_order_id = read_u32_be(&buf[14..18]);

    Ok(InputMessage::Cancel(Cancel::new(user_id, user_order_id)))
}

/// Encode a single input message into a binary frame.
pub fn encode_input(msg: &InputMessage, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    match msg {
        InputMessage::NewOrder(n) => encode_new_order(n, out),
        InputMessage::Cancel(c) => encode_cancel(c, out),
        InputMessage::Flush => encode_flush(out),
        InputMessage::QueryTopOfBook(_) => {
            // QueryTopOfBook not in Zig protocol - treat as flush for now
            encode_flush(out)
        }
    }
}

fn encode_new_order(order: &NewOrder, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    out.push(MAGIC_BYTE);
    out.push(WireInputType::NewOrder as u8);
    out.extend_from_slice(&order.user_id.to_be_bytes());
    write_symbol(order.symbol.as_str(), out);
    out.extend_from_slice(&order.price.to_be_bytes());
    out.extend_from_slice(&order.quantity.to_be_bytes());
    out.push(match order.side {
        Side::Buy => b'B',
        Side::Sell => b'S',
    });
    out.extend_from_slice(&order.user_order_id.to_be_bytes());
    Ok(())
}

fn encode_cancel(cancel: &Cancel, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    out.push(MAGIC_BYTE);
    out.push(WireInputType::Cancel as u8);
    out.extend_from_slice(&cancel.user_id.to_be_bytes());
    // Symbol field (8 bytes of zeros since Cancel doesn't carry symbol in engine-core)
    out.extend_from_slice(&[0u8; SYMBOL_SIZE]);
    out.extend_from_slice(&cancel.user_order_id.to_be_bytes());
    Ok(())
}

fn encode_flush(out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    out.push(MAGIC_BYTE);
    out.push(WireInputType::Flush as u8);
    Ok(())
}

// ============================================================================
// OUTPUT: server → client
// ============================================================================

/// Decode a single output message from a binary buffer.
pub fn decode_output(buf: &[u8]) -> Result<OutputMessage, ProtocolError> {
    if buf.len() < 2 {
        return Err(ProtocolError::Truncated);
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
    }
}

fn decode_ack(buf: &[u8]) -> Result<OutputMessage, ProtocolError> {
    if buf.len() < ACK_SIZE {
        return Err(ProtocolError::Truncated);
    }

    let symbol = read_symbol(&buf[2..10]);
    let user_id = read_u32_be(&buf[10..14]);
    let user_order_id = read_u32_be(&buf[14..18]);

    Ok(OutputMessage::Ack(Ack::new(user_id, user_order_id, symbol)))
}

fn decode_cancel_ack(buf: &[u8]) -> Result<OutputMessage, ProtocolError> {
    if buf.len() < CANCEL_ACK_SIZE {
        return Err(ProtocolError::Truncated);
    }

    let symbol = read_symbol(&buf[2..10]);
    let user_id = read_u32_be(&buf[10..14]);
    let user_order_id = read_u32_be(&buf[14..18]);

    Ok(OutputMessage::CancelAck(CancelAck::new(
        user_id,
        user_order_id,
        symbol,
    )))
}

fn decode_trade(buf: &[u8]) -> Result<OutputMessage, ProtocolError> {
    if buf.len() < TRADE_SIZE {
        return Err(ProtocolError::Truncated);
    }

    let symbol = read_symbol(&buf[2..10]);
    let buy_user_id = read_u32_be(&buf[10..14]);
    let buy_order_id = read_u32_be(&buf[14..18]);
    let sell_user_id = read_u32_be(&buf[18..22]);
    let sell_order_id = read_u32_be(&buf[22..26]);
    let price = read_u32_be(&buf[26..30]);
    let quantity = read_u32_be(&buf[30..34]);

    Ok(OutputMessage::Trade(Trade::new(
        symbol,
        buy_user_id,
        buy_order_id,
        sell_user_id,
        sell_order_id,
        price,
        quantity,
    )))
}

fn decode_top_of_book(buf: &[u8]) -> Result<OutputMessage, ProtocolError> {
    if buf.len() < TOP_OF_BOOK_SIZE {
        return Err(ProtocolError::Truncated);
    }

    let symbol = read_symbol(&buf[2..10]);
    let side = match buf[10] {
        b'B' | b'b' => Side::Buy,
        b'S' | b's' => Side::Sell,
        _ => return Err(ProtocolError::InvalidField("side")),
    };
    let price = read_u32_be(&buf[11..15]);
    let quantity = read_u32_be(&buf[15..19]);
    // buf[19] is padding

    let tob = if price == 0 && quantity == 0 {
        TopOfBook::eliminated(symbol, side)
    } else {
        TopOfBook::active(symbol, side, price, quantity)
    };

    Ok(OutputMessage::TopOfBook(tob))
}

/// Encode a single output message into a binary frame.
pub fn encode_output(msg: &OutputMessage, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    match msg {
        OutputMessage::Ack(a) => encode_ack(a, out),
        OutputMessage::CancelAck(c) => encode_cancel_ack(c, out),
        OutputMessage::Trade(t) => encode_trade(t, out),
        OutputMessage::TopOfBook(tob) => encode_top_of_book(tob, out),
    }
}

fn encode_ack(ack: &Ack, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    out.push(MAGIC_BYTE);
    out.push(WireOutputType::Ack as u8);
    write_symbol(ack.symbol.as_str(), out);
    out.extend_from_slice(&ack.user_id.to_be_bytes());
    out.extend_from_slice(&ack.user_order_id.to_be_bytes());
    Ok(())
}

fn encode_cancel_ack(ack: &CancelAck, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    out.push(MAGIC_BYTE);
    out.push(WireOutputType::CancelAck as u8);
    write_symbol(ack.symbol.as_str(), out);
    out.extend_from_slice(&ack.user_id.to_be_bytes());
    out.extend_from_slice(&ack.user_order_id.to_be_bytes());
    Ok(())
}

fn encode_trade(trade: &Trade, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    out.push(MAGIC_BYTE);
    out.push(WireOutputType::Trade as u8);
    write_symbol(trade.symbol.as_str(), out);
    out.extend_from_slice(&trade.user_id_buy.to_be_bytes());
    out.extend_from_slice(&trade.user_order_id_buy.to_be_bytes());
    out.extend_from_slice(&trade.user_id_sell.to_be_bytes());
    out.extend_from_slice(&trade.user_order_id_sell.to_be_bytes());
    out.extend_from_slice(&trade.price.to_be_bytes());
    out.extend_from_slice(&trade.quantity.to_be_bytes());
    Ok(())
}

fn encode_top_of_book(tob: &TopOfBook, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    out.push(MAGIC_BYTE);
    out.push(WireOutputType::TopOfBook as u8);
    write_symbol(tob.symbol.as_str(), out);
    out.push(match tob.side {
        Side::Buy => b'B',
        Side::Sell => b'S',
    });
    
    let (price, qty) = if tob.eliminated {
        (0u32, 0u32)
    } else {
        (tob.price, tob.total_quantity)
    };
    
    out.extend_from_slice(&price.to_be_bytes());
    out.extend_from_slice(&qty.to_be_bytes());
    out.push(0); // padding byte
    Ok(())
}

// ============================================================================
// Encoder/Decoder wrapper types
// ============================================================================

/// Stateful binary encoder.
#[derive(Debug, Default)]
pub struct BinaryEncoder {
    buf: Vec<u8>,
}

impl BinaryEncoder {
    pub fn new() -> Self {
        BinaryEncoder {
            buf: Vec::with_capacity(64),
        }
    }

    pub fn encode_input(&mut self, msg: &InputMessage) -> Result<&[u8], ProtocolError> {
        self.buf.clear();
        encode_input(msg, &mut self.buf)?;
        Ok(&self.buf)
    }

    pub fn encode_output(&mut self, msg: &OutputMessage) -> Result<&[u8], ProtocolError> {
        self.buf.clear();
        encode_output(msg, &mut self.buf)?;
        Ok(&self.buf)
    }

    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }
}

/// Stateful binary decoder.
#[derive(Debug, Default)]
pub struct BinaryDecoder;

impl BinaryDecoder {
    pub fn new() -> Self {
        BinaryDecoder
    }

    pub fn decode_input(&self, data: &[u8]) -> Result<InputMessage, ProtocolError> {
        decode_input(data)
    }

    pub fn decode_output(&self, data: &[u8]) -> Result<OutputMessage, ProtocolError> {
        decode_output(data)
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn read_u32_be(bytes: &[u8]) -> u32 {
    let arr: [u8; 4] = bytes[0..4].try_into().expect("slice with incorrect length");
    u32::from_be_bytes(arr)
}

/// Read a null-padded symbol from exactly SYMBOL_SIZE bytes.
fn read_symbol(bytes: &[u8]) -> Symbol {
    debug_assert!(bytes.len() == SYMBOL_SIZE);
    
    // Find null terminator or use full length
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(SYMBOL_SIZE);
    
    // Convert to string (ASCII only)
    let s = std::str::from_utf8(&bytes[..len]).unwrap_or("");
    Symbol::from_str(s)
}

/// Write a symbol as SYMBOL_SIZE bytes, null-padded.
fn write_symbol(s: &str, out: &mut Vec<u8>) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(SYMBOL_SIZE);
    out.extend_from_slice(&bytes[..len]);
    
    // Pad with zeros
    for _ in len..SYMBOL_SIZE {
        out.push(0);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_new_order_zig_format() {
        // Actual bytes from Zig client
        let buf: [u8; 27] = [
            0x4D, 0x4E,             // M, N
            0x00, 0x00, 0x00, 0x01, // user_id = 1
            b'I', b'B', b'M', 0, 0, 0, 0, 0, // symbol = "IBM"
            0x00, 0x00, 0x00, 0x64, // price = 100
            0x00, 0x00, 0x00, 0x32, // quantity = 50
            b'B',                   // side = Buy
            0x00, 0x00, 0x00, 0x01, // order_id = 1
        ];

        let msg = decode_input(&buf).unwrap();
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
    fn test_decode_cancel_zig_format() {
        let buf: [u8; 18] = [
            0x4D, 0x43,             // M, C
            0x00, 0x00, 0x00, 0x01, // user_id = 1
            b'I', b'B', b'M', 0, 0, 0, 0, 0, // symbol = "IBM"
            0x00, 0x00, 0x00, 0x05, // order_id = 5
        ];

        let msg = decode_input(&buf).unwrap();
        match msg {
            InputMessage::Cancel(c) => {
                assert_eq!(c.user_id, 1);
                assert_eq!(c.user_order_id, 5);
            }
            _ => panic!("Expected Cancel"),
        }
    }

    #[test]
    fn test_decode_flush_zig_format() {
        let buf: [u8; 2] = [0x4D, 0x46]; // M, F

        let msg = decode_input(&buf).unwrap();
        assert!(matches!(msg, InputMessage::Flush));
    }

    #[test]
    fn test_encode_ack_zig_format() {
        let ack = Ack::new(1, 100, Symbol::from_str("IBM"));
        let mut buf = Vec::new();
        encode_ack(&ack, &mut buf).unwrap();

        assert_eq!(buf.len(), ACK_SIZE);
        assert_eq!(buf[0], 0x4D); // magic
        assert_eq!(buf[1], b'A'); // msg_type
        assert_eq!(&buf[2..5], b"IBM"); // symbol starts
        assert_eq!(buf[9], 0); // symbol null-padded
    }

    #[test]
    fn test_encode_trade_zig_format() {
        let trade = Trade::new(
            Symbol::from_str("AAPL"),
            1, 100,  // buy user, order
            2, 200,  // sell user, order
            150, 50, // price, qty
        );
        let mut buf = Vec::new();
        encode_trade(&trade, &mut buf).unwrap();

        assert_eq!(buf.len(), TRADE_SIZE);
        assert_eq!(buf[0], 0x4D);
        assert_eq!(buf[1], b'T');
    }

    #[test]
    fn test_encode_top_of_book_zig_format() {
        let tob = TopOfBook::active(Symbol::from_str("IBM"), Side::Buy, 100, 50);
        let mut buf = Vec::new();
        encode_top_of_book(&tob, &mut buf).unwrap();

        assert_eq!(buf.len(), TOP_OF_BOOK_SIZE);
        assert_eq!(buf[0], 0x4D);
        assert_eq!(buf[1], b'B');
        assert_eq!(buf[10], b'B'); // side
    }

    #[test]
    fn test_roundtrip_new_order() {
        let order = NewOrder::new(42, 1001, Symbol::from_str("GOOG"), 500, 10, Side::Sell);
        let msg = InputMessage::NewOrder(order);

        let mut buf = Vec::new();
        encode_input(&msg, &mut buf).unwrap();

        let decoded = decode_input(&buf).unwrap();
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
    fn test_invalid_magic() {
        let buf = [0x00, b'N', 0, 0]; // Wrong magic
        let result = decode_input(&buf);
        assert!(matches!(result, Err(ProtocolError::InvalidMagic(0x00))));
    }

    #[test]
    fn test_unknown_message_type() {
        let buf = [0x4D, b'Z', 0, 0]; // Unknown type 'Z'
        let result = decode_input(&buf);
        assert!(matches!(result, Err(ProtocolError::UnknownMessageType(b'Z'))));
    }
}
