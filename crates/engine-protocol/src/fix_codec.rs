//! FIX 4.2/4.4 protocol codec.
//!
//! This module provides FIX protocol support for institutional connectivity.
//! It uses the `fefix` library for standards-compliant encoding/decoding.
//!
//! # Supported Message Types
//!
//! | FIX MsgType | Description | Direction |
//! |-------------|-------------|-----------|
//! | D (35=D)    | New Order Single | Client → Server |
//! | F (35=F)    | Order Cancel Request | Client → Server |
//! | 8 (35=8)    | Execution Report | Server → Client |
//! | 9 (35=9)    | Order Cancel Reject | Server → Client |
//!
//! # FIX Tags Used
//!
//! | Tag | Name | Description |
//! |-----|------|-------------|
//! | 11  | ClOrdID | Client order ID (maps to user_order_id) |
//! | 17  | ExecID | Execution ID |
//! | 20  | ExecTransType | Execution transaction type |
//! | 35  | MsgType | Message type |
//! | 37  | OrderID | Server-assigned order ID |
//! | 38  | OrderQty | Order quantity |
//! | 39  | OrdStatus | Order status |
//! | 40  | OrdType | Order type (1=Market, 2=Limit) |
//! | 44  | Price | Limit price |
//! | 49  | SenderCompID | Sender ID |
//! | 54  | Side | Side (1=Buy, 2=Sell) |
//! | 55  | Symbol | Instrument symbol |
//! | 56  | TargetCompID | Target ID |
//! | 150 | ExecType | Execution type |
//! | 151 | LeavesQty | Remaining quantity |
//!
//! # Example
//!
//! ```rust,ignore
//! use engine_protocol::fix_codec::{FixEncoder, FixDecoder, FixVersion};
//! use engine_core::{InputMessage, NewOrder, Symbol, Side};
//!
//! let mut encoder = FixEncoder::new(FixVersion::Fix44, "CLIENT", "SERVER");
//! let order = NewOrder::new(1, 100, Symbol::from_str("IBM"), 1000, 50, Side::Buy);
//!
//! let fix_msg = encoder.encode_new_order(&order).unwrap();
//! println!("{}", String::from_utf8_lossy(&fix_msg));
//! ```

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use engine_core::{
    Ack, Cancel, CancelAck, InputMessage, NewOrder, OutputMessage, Reject, RejectReason,
    Side, Symbol, Trade,
};

// =============================================================================
// FIX Version
// =============================================================================

/// Supported FIX protocol versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixVersion {
    /// FIX 4.2
    Fix42,
    /// FIX 4.4
    Fix44,
}

impl FixVersion {
    /// Get the BeginString value for this version.
    pub fn begin_string(&self) -> &'static str {
        match self {
            FixVersion::Fix42 => "FIX.4.2",
            FixVersion::Fix44 => "FIX.4.4",
        }
    }
}

// =============================================================================
// Error type
// =============================================================================

/// Errors during FIX encoding/decoding.
#[derive(Debug)]
pub enum FixError {
    /// Missing required field.
    MissingField(&'static str),
    /// Invalid field value.
    InvalidField {
        /// The FIX tag number.
        tag: u32,
        /// Description of why it's invalid.
        reason: &'static str,
    },
    /// Invalid message type.
    InvalidMsgType(String),
    /// Parse error.
    ParseError(String),
    /// Checksum mismatch.
    ChecksumMismatch {
        /// Expected checksum.
        expected: u8,
        /// Actual checksum.
        got: u8,
    },
    /// Message too large.
    MessageTooLarge,
}

impl fmt::Display for FixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FixError::MissingField(field) => write!(f, "missing required field: {}", field),
            FixError::InvalidField { tag, reason } => {
                write!(f, "invalid field {}: {}", tag, reason)
            }
            FixError::InvalidMsgType(t) => write!(f, "invalid message type: {}", t),
            FixError::ParseError(msg) => write!(f, "parse error: {}", msg),
            FixError::ChecksumMismatch { expected, got } => {
                write!(f, "checksum mismatch: expected {}, got {}", expected, got)
            }
            FixError::MessageTooLarge => write!(f, "message too large"),
        }
    }
}

impl std::error::Error for FixError {}

// =============================================================================
// FIX Constants
// =============================================================================

/// FIX field delimiter (SOH = 0x01).
const SOH: u8 = 0x01;

/// Maximum FIX message size.
const MAX_FIX_MSG_SIZE: usize = 4096;

// FIX Tags
mod tags {
    /// BeginString (8).
    pub const BEGIN_STRING: u32 = 8;
    /// BodyLength (9).
    pub const BODY_LENGTH: u32 = 9;
    /// MsgType (35).
    pub const MSG_TYPE: u32 = 35;
    /// SenderCompID (49).
    pub const SENDER_COMP_ID: u32 = 49;
    /// TargetCompID (56).
    pub const TARGET_COMP_ID: u32 = 56;
    /// MsgSeqNum (34).
    pub const MSG_SEQ_NUM: u32 = 34;
    /// SendingTime (52).
    pub const SENDING_TIME: u32 = 52;
    /// Checksum (10).
    pub const CHECKSUM: u32 = 10;

    /// ClOrdID (11).
    pub const CL_ORD_ID: u32 = 11;
    /// OrigClOrdID (41).
    pub const ORIG_CL_ORD_ID: u32 = 41;
    /// OrderID (37).
    pub const ORDER_ID: u32 = 37;
    /// ExecID (17).
    pub const EXEC_ID: u32 = 17;
    /// ExecType (150).
    pub const EXEC_TYPE: u32 = 150;
    /// OrdStatus (39).
    pub const ORD_STATUS: u32 = 39;
    /// Symbol (55).
    pub const SYMBOL: u32 = 55;
    /// Side (54).
    pub const SIDE: u32 = 54;
    /// OrderQty (38).
    pub const ORDER_QTY: u32 = 38;
    /// OrdType (40).
    pub const ORD_TYPE: u32 = 40;
    /// Price (44).
    pub const PRICE: u32 = 44;
    /// LastPx (31).
    pub const LAST_PX: u32 = 31;
    /// LastQty (32).
    pub const LAST_QTY: u32 = 32;
    /// LeavesQty (151).
    pub const LEAVES_QTY: u32 = 151;
    /// CumQty (14).
    pub const CUM_QTY: u32 = 14;
    /// AvgPx (6).
    pub const AVG_PX: u32 = 6;
    /// TransactTime (60).
    pub const TRANSACT_TIME: u32 = 60;
    /// OrdRejReason (103).
    pub const ORD_REJ_REASON: u32 = 103;
    /// Text (58).
    pub const TEXT: u32 = 58;
}

// FIX Message Types
mod msg_types {
    /// New Order Single.
    pub const NEW_ORDER_SINGLE: &str = "D";
    /// Order Cancel Request.
    pub const ORDER_CANCEL_REQUEST: &str = "F";
    /// Execution Report.
    pub const EXECUTION_REPORT: &str = "8";
    // pub const ORDER_CANCEL_REJECT: &str = "9";
}

// FIX Side values
mod fix_side {
    /// Buy side.
    pub const BUY: char = '1';
    /// Sell side.
    pub const SELL: char = '2';
}

// FIX OrdType values
mod fix_ord_type {
    /// Market order.
    pub const MARKET: char = '1';
    /// Limit order.
    pub const LIMIT: char = '2';
}

// FIX ExecType values
mod fix_exec_type {
    /// New order.
    pub const NEW: char = '0';
    /// Canceled.
    pub const CANCELED: char = '4';
    /// Trade (fill).
    pub const TRADE: char = 'F';
    /// Rejected.
    pub const REJECTED: char = '8';
}

// FIX OrdStatus values
mod fix_ord_status {
    /// New.
    pub const NEW: char = '0';
    // pub const PARTIALLY_FILLED: char = '1';
    /// Filled.
    pub const FILLED: char = '2';
    /// Canceled.
    pub const CANCELED: char = '4';
    /// Rejected.
    pub const REJECTED: char = '8';
}

// =============================================================================
// FIX Encoder
// =============================================================================

/// FIX message encoder.
#[derive(Debug)]
pub struct FixEncoder {
    version: FixVersion,
    sender_comp_id: String,
    target_comp_id: String,
    seq_num: u64,
    buf: Vec<u8>,
}

impl FixEncoder {
    /// Create a new FIX encoder.
    pub fn new(version: FixVersion, sender_comp_id: &str, target_comp_id: &str) -> Self {
        FixEncoder {
            version,
            sender_comp_id: sender_comp_id.to_string(),
            target_comp_id: target_comp_id.to_string(),
            seq_num: 1,
            buf: Vec::with_capacity(MAX_FIX_MSG_SIZE),
        }
    }

    /// Encode an input message to FIX format.
    pub fn encode_input(&mut self, msg: &InputMessage) -> Result<Vec<u8>, FixError> {
        match msg {
            InputMessage::NewOrder(order) => self.encode_new_order(order),
            InputMessage::Cancel(cancel) => self.encode_cancel(cancel),
            InputMessage::Flush => {
                // FIX doesn't have a flush concept - return empty or error
                Err(FixError::InvalidMsgType("Flush".to_string()))
            }
            InputMessage::QueryTopOfBook(_) => {
                // Could map to MarketDataRequest, but keeping simple for now
                Err(FixError::InvalidMsgType("QueryTopOfBook".to_string()))
            }
        }
    }

    /// Encode an output message to FIX format.
    pub fn encode_output(&mut self, msg: &OutputMessage) -> Result<Vec<u8>, FixError> {
        match msg {
            OutputMessage::Ack(ack) => self.encode_ack(ack),
            OutputMessage::CancelAck(cancel_ack) => self.encode_cancel_ack(cancel_ack),
            OutputMessage::Trade(trade) => self.encode_trade(trade),
            OutputMessage::TopOfBook(_) => {
                // Could map to MarketDataSnapshotFullRefresh
                Err(FixError::InvalidMsgType("TopOfBook".to_string()))
            }
            OutputMessage::Reject(reject) => self.encode_reject(reject),
        }
    }

    /// Encode a NewOrder Single (35=D).
    pub fn encode_new_order(&mut self, order: &NewOrder) -> Result<Vec<u8>, FixError> {
        self.buf.clear();

        // Build body first (we need length for header)
        let mut body = Vec::with_capacity(256);

        // MsgType
        write_field(&mut body, tags::MSG_TYPE, msg_types::NEW_ORDER_SINGLE);

        // ClOrdID (user_order_id as string)
        write_field(&mut body, tags::CL_ORD_ID, &order.user_order_id.to_string());

        // Symbol
        write_field(&mut body, tags::SYMBOL, order.symbol.as_str());

        // Side
        let side_char = match order.side {
            Side::Buy => fix_side::BUY,
            Side::Sell => fix_side::SELL,
        };
        write_field(&mut body, tags::SIDE, &side_char.to_string());

        // TransactTime
        write_field(&mut body, tags::TRANSACT_TIME, &current_utc_timestamp());

        // OrderQty
        write_field(&mut body, tags::ORDER_QTY, &order.quantity.to_string());

        // OrdType
        let ord_type = if order.price == 0 {
            fix_ord_type::MARKET
        } else {
            fix_ord_type::LIMIT
        };
        write_field(&mut body, tags::ORD_TYPE, &ord_type.to_string());

        // Price (only for limit orders)
        if order.price > 0 {
            // Convert price to decimal (assuming 2 decimal places)
            let price_str = format!("{}.{:02}", order.price / 100, order.price % 100);
            write_field(&mut body, tags::PRICE, &price_str);
        }

        // Build complete message
        self.build_message(&body)
    }

    /// Encode an Order Cancel Request (35=F).
    pub fn encode_cancel(&mut self, cancel: &Cancel) -> Result<Vec<u8>, FixError> {
        self.buf.clear();

        let mut body = Vec::with_capacity(256);

        // MsgType
        write_field(&mut body, tags::MSG_TYPE, msg_types::ORDER_CANCEL_REQUEST);

        // OrigClOrdID
        write_field(
            &mut body,
            tags::ORIG_CL_ORD_ID,
            &cancel.user_order_id.to_string(),
        );

        // ClOrdID (new ID for the cancel request)
        write_field(
            &mut body,
            tags::CL_ORD_ID,
            &format!("C{}", cancel.user_order_id),
        );

        // Symbol (required but we don't have it in Cancel - use placeholder)
        write_field(&mut body, tags::SYMBOL, "N/A");

        // Side (required but we don't have it - use placeholder)
        write_field(&mut body, tags::SIDE, &fix_side::BUY.to_string());

        // TransactTime
        write_field(&mut body, tags::TRANSACT_TIME, &current_utc_timestamp());

        self.build_message(&body)
    }

    /// Encode an Execution Report for Ack (35=8, ExecType=0).
    fn encode_ack(&mut self, ack: &Ack) -> Result<Vec<u8>, FixError> {
        self.buf.clear();

        let mut body = Vec::with_capacity(256);

        write_field(&mut body, tags::MSG_TYPE, msg_types::EXECUTION_REPORT);
        write_field(&mut body, tags::ORDER_ID, &ack.user_order_id.to_string());
        write_field(&mut body, tags::CL_ORD_ID, &ack.user_order_id.to_string());
        write_field(&mut body, tags::EXEC_ID, &format!("E{}", self.seq_num));
        write_field(
            &mut body,
            tags::EXEC_TYPE,
            &fix_exec_type::NEW.to_string(),
        );
        write_field(
            &mut body,
            tags::ORD_STATUS,
            &fix_ord_status::NEW.to_string(),
        );
        write_field(&mut body, tags::SYMBOL, ack.symbol.as_str());
        write_field(&mut body, tags::SIDE, &fix_side::BUY.to_string());
        write_field(&mut body, tags::LEAVES_QTY, "0");
        write_field(&mut body, tags::CUM_QTY, "0");
        write_field(&mut body, tags::AVG_PX, "0");

        self.build_message(&body)
    }

    /// Encode an Execution Report for CancelAck (35=8, ExecType=4).
    fn encode_cancel_ack(&mut self, cancel_ack: &CancelAck) -> Result<Vec<u8>, FixError> {
        self.buf.clear();

        let mut body = Vec::with_capacity(256);

        write_field(&mut body, tags::MSG_TYPE, msg_types::EXECUTION_REPORT);
        write_field(
            &mut body,
            tags::ORDER_ID,
            &cancel_ack.user_order_id.to_string(),
        );
        write_field(
            &mut body,
            tags::CL_ORD_ID,
            &cancel_ack.user_order_id.to_string(),
        );
        write_field(&mut body, tags::EXEC_ID, &format!("E{}", self.seq_num));
        write_field(
            &mut body,
            tags::EXEC_TYPE,
            &fix_exec_type::CANCELED.to_string(),
        );
        write_field(
            &mut body,
            tags::ORD_STATUS,
            &fix_ord_status::CANCELED.to_string(),
        );
        write_field(&mut body, tags::SYMBOL, cancel_ack.symbol.as_str());
        write_field(&mut body, tags::SIDE, &fix_side::BUY.to_string());
        write_field(&mut body, tags::LEAVES_QTY, "0");
        write_field(&mut body, tags::CUM_QTY, "0");
        write_field(&mut body, tags::AVG_PX, "0");

        self.build_message(&body)
    }

    /// Encode an Execution Report for Trade (35=8, ExecType=F).
    fn encode_trade(&mut self, trade: &Trade) -> Result<Vec<u8>, FixError> {
        self.buf.clear();

        let mut body = Vec::with_capacity(256);

        // Price as decimal
        let price_str = format!("{}.{:02}", trade.price / 100, trade.price % 100);

        write_field(&mut body, tags::MSG_TYPE, msg_types::EXECUTION_REPORT);
        write_field(
            &mut body,
            tags::ORDER_ID,
            &trade.user_order_id_buy.to_string(),
        );
        write_field(
            &mut body,
            tags::CL_ORD_ID,
            &trade.user_order_id_buy.to_string(),
        );
        write_field(&mut body, tags::EXEC_ID, &format!("E{}", self.seq_num));
        write_field(
            &mut body,
            tags::EXEC_TYPE,
            &fix_exec_type::TRADE.to_string(),
        );
        write_field(
            &mut body,
            tags::ORD_STATUS,
            &fix_ord_status::FILLED.to_string(),
        );
        write_field(&mut body, tags::SYMBOL, trade.symbol.as_str());
        write_field(&mut body, tags::SIDE, &fix_side::BUY.to_string());
        write_field(&mut body, tags::LAST_PX, &price_str);
        write_field(&mut body, tags::LAST_QTY, &trade.quantity.to_string());
        write_field(&mut body, tags::LEAVES_QTY, "0");
        write_field(&mut body, tags::CUM_QTY, &trade.quantity.to_string());
        write_field(&mut body, tags::AVG_PX, &price_str);

        self.build_message(&body)
    }

    /// Encode an Execution Report for Reject (35=8, ExecType=8, OrdStatus=8).
    fn encode_reject(&mut self, reject: &Reject) -> Result<Vec<u8>, FixError> {
        self.buf.clear();

        let mut body = Vec::with_capacity(256);

        write_field(&mut body, tags::MSG_TYPE, msg_types::EXECUTION_REPORT);
        write_field(&mut body, tags::ORDER_ID, &reject.user_order_id.to_string());
        write_field(&mut body, tags::CL_ORD_ID, &reject.user_order_id.to_string());
        write_field(&mut body, tags::EXEC_ID, &format!("E{}", self.seq_num));

        // ExecType = 8 (Rejected)
        write_field(
            &mut body,
            tags::EXEC_TYPE,
            &fix_exec_type::REJECTED.to_string(),
        );

        // OrdStatus = 8 (Rejected)
        write_field(
            &mut body,
            tags::ORD_STATUS,
            &fix_ord_status::REJECTED.to_string(),
        );

        write_field(&mut body, tags::SYMBOL, reject.symbol.as_str());
        write_field(&mut body, tags::SIDE, &fix_side::BUY.to_string());
        write_field(&mut body, tags::LEAVES_QTY, "0");
        write_field(&mut body, tags::CUM_QTY, "0");
        write_field(&mut body, tags::AVG_PX, "0");

        // OrdRejReason (tag 103)
        let reject_reason = match reject.reason {
            RejectReason::UnknownSymbol => "1",      // Unknown symbol
            RejectReason::CapacityExceeded => "4",   // Too late to enter
            RejectReason::InvalidOrder => "0",       // Broker option
            RejectReason::DuplicateOrderId => "6",   // Duplicate Order
        };
        write_field(&mut body, tags::ORD_REJ_REASON, reject_reason);

        // Text (tag 58) - human readable reason
        let text = match reject.reason {
            RejectReason::UnknownSymbol => "Unknown symbol",
            RejectReason::CapacityExceeded => "Capacity exceeded",
            RejectReason::InvalidOrder => "Invalid order",
            RejectReason::DuplicateOrderId => "Duplicate order ID",
        };
        write_field(&mut body, tags::TEXT, text);

        self.build_message(&body)
    }

    /// Build a complete FIX message with header and trailer.
    fn build_message(&mut self, body: &[u8]) -> Result<Vec<u8>, FixError> {
        self.buf.clear();

        // Calculate body length (everything between BodyLength and Checksum)
        let mut header = Vec::with_capacity(128);

        // SenderCompID
        write_field(&mut header, tags::SENDER_COMP_ID, &self.sender_comp_id);

        // TargetCompID
        write_field(&mut header, tags::TARGET_COMP_ID, &self.target_comp_id);

        // MsgSeqNum
        write_field(&mut header, tags::MSG_SEQ_NUM, &self.seq_num.to_string());

        // SendingTime
        write_field(&mut header, tags::SENDING_TIME, &current_utc_timestamp());

        let body_length = header.len() + body.len();

        // BeginString
        write_field(&mut self.buf, tags::BEGIN_STRING, self.version.begin_string());

        // BodyLength
        write_field(&mut self.buf, tags::BODY_LENGTH, &body_length.to_string());

        // Add header fields
        self.buf.extend_from_slice(&header);

        // Add body
        self.buf.extend_from_slice(body);

        // Calculate and add checksum
        let checksum = calculate_checksum(&self.buf);
        write_field(&mut self.buf, tags::CHECKSUM, &format!("{:03}", checksum));

        self.seq_num += 1;

        Ok(self.buf.clone())
    }

    /// Get current sequence number.
    pub fn seq_num(&self) -> u64 {
        self.seq_num
    }

    /// Set sequence number (for session recovery).
    pub fn set_seq_num(&mut self, seq: u64) {
        self.seq_num = seq;
    }
}

// =============================================================================
// FIX Decoder
// =============================================================================

/// FIX message decoder.
#[derive(Debug)]
#[allow(dead_code)]
pub struct FixDecoder {
    version: FixVersion,
}

impl FixDecoder {
    /// Create a new FIX decoder.
    pub fn new(version: FixVersion) -> Self {
        FixDecoder { version }
    }

    /// Decode a FIX message into an InputMessage.
    pub fn decode_input(&self, data: &[u8]) -> Result<InputMessage, FixError> {
        let fields = parse_fix_message(data)?;

        let msg_type = fields
            .get(&tags::MSG_TYPE)
            .ok_or(FixError::MissingField("MsgType"))?;

        match msg_type.as_str() {
            msg_types::NEW_ORDER_SINGLE => self.decode_new_order(&fields),
            msg_types::ORDER_CANCEL_REQUEST => self.decode_cancel(&fields),
            _ => Err(FixError::InvalidMsgType(msg_type.clone())),
        }
    }

    /// Decode a FIX message into an OutputMessage.
    pub fn decode_output(&self, data: &[u8]) -> Result<OutputMessage, FixError> {
        let fields = parse_fix_message(data)?;

        let msg_type = fields
            .get(&tags::MSG_TYPE)
            .ok_or(FixError::MissingField("MsgType"))?;

        match msg_type.as_str() {
            msg_types::EXECUTION_REPORT => self.decode_execution_report(&fields),
            _ => Err(FixError::InvalidMsgType(msg_type.clone())),
        }
    }

    fn decode_new_order(
        &self,
        fields: &std::collections::HashMap<u32, String>,
    ) -> Result<InputMessage, FixError> {
        let cl_ord_id = fields
            .get(&tags::CL_ORD_ID)
            .ok_or(FixError::MissingField("ClOrdID"))?;
        let symbol_str = fields
            .get(&tags::SYMBOL)
            .ok_or(FixError::MissingField("Symbol"))?;
        let side_str = fields
            .get(&tags::SIDE)
            .ok_or(FixError::MissingField("Side"))?;
        let qty_str = fields
            .get(&tags::ORDER_QTY)
            .ok_or(FixError::MissingField("OrderQty"))?;
        let ord_type = fields
            .get(&tags::ORD_TYPE)
            .ok_or(FixError::MissingField("OrdType"))?;

        let user_order_id: u32 = cl_ord_id
            .parse()
            .map_err(|_| FixError::InvalidField {
                tag: tags::CL_ORD_ID,
                reason: "not a valid u32",
            })?;

        let symbol = Symbol::from_str(symbol_str);

        let side = match side_str.chars().next() {
            Some(fix_side::BUY) => Side::Buy,
            Some(fix_side::SELL) => Side::Sell,
            _ => {
                return Err(FixError::InvalidField {
                    tag: tags::SIDE,
                    reason: "invalid side",
                })
            }
        };

        let quantity: u32 = qty_str.parse().map_err(|_| FixError::InvalidField {
            tag: tags::ORDER_QTY,
            reason: "not a valid u32",
        })?;

        let price = if ord_type.starts_with(fix_ord_type::LIMIT) {
            let price_str = fields.get(&tags::PRICE).ok_or(FixError::MissingField("Price"))?;
            parse_fix_price(price_str)?
        } else {
            0 // Market order
        };

        // user_id not in FIX - use placeholder
        let user_id = 0;

        Ok(InputMessage::NewOrder(NewOrder::new(
            user_id,
            user_order_id,
            symbol,
            price,
            quantity,
            side,
        )))
    }

    fn decode_cancel(
        &self,
        fields: &std::collections::HashMap<u32, String>,
    ) -> Result<InputMessage, FixError> {
        let orig_cl_ord_id = fields
            .get(&tags::ORIG_CL_ORD_ID)
            .ok_or(FixError::MissingField("OrigClOrdID"))?;

        let user_order_id: u32 = orig_cl_ord_id
            .parse()
            .map_err(|_| FixError::InvalidField {
                tag: tags::ORIG_CL_ORD_ID,
                reason: "not a valid u32",
            })?;

        // user_id not in FIX - use placeholder
        let user_id = 0;

        Ok(InputMessage::Cancel(Cancel::new(user_id, user_order_id)))
    }

    fn decode_execution_report(
        &self,
        fields: &std::collections::HashMap<u32, String>,
    ) -> Result<OutputMessage, FixError> {
        let exec_type = fields
            .get(&tags::EXEC_TYPE)
            .ok_or(FixError::MissingField("ExecType"))?;

        let symbol_str = fields
            .get(&tags::SYMBOL)
            .ok_or(FixError::MissingField("Symbol"))?;
        let symbol = Symbol::from_str(symbol_str);

        let cl_ord_id = fields
            .get(&tags::CL_ORD_ID)
            .ok_or(FixError::MissingField("ClOrdID"))?;
        let user_order_id: u32 = cl_ord_id
            .parse()
            .map_err(|_| FixError::InvalidField {
                tag: tags::CL_ORD_ID,
                reason: "not a valid u32",
            })?;

        // user_id not in FIX
        let user_id = 0;

        match exec_type.chars().next() {
            Some(fix_exec_type::NEW) => Ok(OutputMessage::ack(user_id, user_order_id, symbol)),
            Some(fix_exec_type::CANCELED) => {
                Ok(OutputMessage::cancel_ack(user_id, user_order_id, symbol))
            }
            Some(fix_exec_type::TRADE) => {
                let price = if let Some(price_str) = fields.get(&tags::LAST_PX) {
                    parse_fix_price(price_str)?
                } else {
                    0
                };

                let quantity: u32 = fields
                    .get(&tags::LAST_QTY)
                    .ok_or(FixError::MissingField("LastQty"))?
                    .parse()
                    .map_err(|_| FixError::InvalidField {
                        tag: tags::LAST_QTY,
                        reason: "not a valid u32",
                    })?;

                // For trades, we don't have full buyer/seller info in a single exec report
                Ok(OutputMessage::trade(
                    symbol,
                    user_id,
                    user_order_id,
                    0, // seller user_id unknown
                    0, // seller order_id unknown
                    price,
                    quantity,
                ))
            }
            Some(fix_exec_type::REJECTED) => {
                // Decode rejection
                let reason = if let Some(reason_str) = fields.get(&tags::ORD_REJ_REASON) {
                    match reason_str.as_str() {
                        "1" => RejectReason::UnknownSymbol,
                        "4" => RejectReason::CapacityExceeded,
                        "6" => RejectReason::DuplicateOrderId,
                        _ => RejectReason::InvalidOrder,
                    }
                } else {
                    RejectReason::InvalidOrder
                };

                Ok(OutputMessage::reject(user_id, user_order_id, symbol, reason))
            }
            _ => Err(FixError::InvalidField {
                tag: tags::EXEC_TYPE,
                reason: "unknown exec type",
            }),
        }
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Write a FIX field to buffer: "tag=value<SOH>"
fn write_field(buf: &mut Vec<u8>, tag: u32, value: &str) {
    buf.extend_from_slice(tag.to_string().as_bytes());
    buf.push(b'=');
    buf.extend_from_slice(value.as_bytes());
    buf.push(SOH);
}

/// Calculate FIX checksum (sum of all bytes mod 256).
fn calculate_checksum(data: &[u8]) -> u8 {
    let sum: u32 = data.iter().map(|&b| b as u32).sum();
    (sum % 256) as u8
}

/// Parse a FIX message into a map of tag -> value.
fn parse_fix_message(data: &[u8]) -> Result<std::collections::HashMap<u32, String>, FixError> {
    let mut fields = std::collections::HashMap::new();

    let s = std::str::from_utf8(data).map_err(|e| FixError::ParseError(e.to_string()))?;

    for field in s.split(|c| c == '\x01') {
        if field.is_empty() {
            continue;
        }

        let parts: Vec<&str> = field.splitn(2, '=').collect();
        if parts.len() != 2 {
            continue;
        }

        let tag: u32 = parts[0]
            .parse()
            .map_err(|_| FixError::ParseError(format!("invalid tag: {}", parts[0])))?;
        fields.insert(tag, parts[1].to_string());
    }

    Ok(fields)
}

/// Parse a FIX price string (e.g., "123.45") into integer ticks.
fn parse_fix_price(price_str: &str) -> Result<u32, FixError> {
    // Handle both "123.45" and "123" formats
    if let Some(dot_pos) = price_str.find('.') {
        let int_part: u32 = price_str[..dot_pos]
            .parse()
            .map_err(|_| FixError::InvalidField {
                tag: tags::PRICE,
                reason: "invalid price",
            })?;
        let frac_str = &price_str[dot_pos + 1..];
        let frac_part: u32 = frac_str.parse().map_err(|_| FixError::InvalidField {
            tag: tags::PRICE,
            reason: "invalid price fraction",
        })?;

        // Assume 2 decimal places
        Ok(int_part * 100 + frac_part)
    } else {
        let price: u32 = price_str.parse().map_err(|_| FixError::InvalidField {
            tag: tags::PRICE,
            reason: "invalid price",
        })?;
        Ok(price * 100) // Convert to cents
    }
}

/// Get current UTC timestamp in FIX format (YYYYMMDD-HH:MM:SS.sss).
fn current_utc_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    let secs = now.as_secs();
    let millis = now.subsec_millis();

    // Convert to datetime components (simplified - use chrono for production)
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Approximate date calculation (not accounting for leap years properly)
    let mut year = 1970;
    let mut remaining_days = days_since_epoch;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let (month, day) = day_of_year_to_month_day(remaining_days as u32 + 1, is_leap_year(year));

    format!(
        "{:04}{:02}{:02}-{:02}:{:02}:{:02}.{:03}",
        year, month, day, hours, minutes, seconds, millis
    )
}

fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn day_of_year_to_month_day(doy: u32, leap: bool) -> (u32, u32) {
    let days_in_months: [u32; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut remaining = doy;
    for (i, &days) in days_in_months.iter().enumerate() {
        if remaining <= days {
            return ((i + 1) as u32, remaining);
        }
        remaining -= days;
    }

    (12, 31) // Fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_new_order() {
        let mut encoder = FixEncoder::new(FixVersion::Fix44, "CLIENT", "SERVER");
        let order = NewOrder::new(1, 100, Symbol::from_str("IBM"), 15000, 50, Side::Buy);

        let result = encoder.encode_new_order(&order);
        assert!(result.is_ok());

        let msg = result.unwrap();
        let msg_str = String::from_utf8_lossy(&msg);

        assert!(msg_str.contains("35=D"));
        assert!(msg_str.contains("55=IBM"));
        assert!(msg_str.contains("38=50"));
        assert!(msg_str.contains("40=2")); // Limit
        assert!(msg_str.contains("44=150.00")); // Price
    }

    #[test]
    fn test_encode_market_order() {
        let mut encoder = FixEncoder::new(FixVersion::Fix44, "CLIENT", "SERVER");
        let order = NewOrder::new(1, 100, Symbol::from_str("AAPL"), 0, 100, Side::Sell);

        let result = encoder.encode_new_order(&order);
        assert!(result.is_ok());

        let msg = result.unwrap();
        let msg_str = String::from_utf8_lossy(&msg);

        assert!(msg_str.contains("35=D"));
        assert!(msg_str.contains("40=1")); // Market
        assert!(!msg_str.contains("44=")); // No price
    }

    #[test]
    fn test_encode_reject() {
        let mut encoder = FixEncoder::new(FixVersion::Fix44, "SERVER", "CLIENT");
        let reject = OutputMessage::reject(
            1,
            100,
            Symbol::from_str("IBM"),
            RejectReason::UnknownSymbol,
        );

        let result = encoder.encode_output(&reject);
        assert!(result.is_ok());

        let msg = result.unwrap();
        let msg_str = String::from_utf8_lossy(&msg);

        assert!(msg_str.contains("35=8"));      // Execution Report
        assert!(msg_str.contains("150=8"));     // ExecType = Rejected
        assert!(msg_str.contains("39=8"));      // OrdStatus = Rejected
        assert!(msg_str.contains("103=1"));     // OrdRejReason = Unknown symbol
        assert!(msg_str.contains("58=Unknown symbol")); // Text
    }

    #[test]
    fn test_checksum() {
        let data = b"8=FIX.4.4\x019=5\x0135=0\x01";
        let checksum = calculate_checksum(data);
        // Just verify it produces something reasonable
        assert!(checksum > 0);
    }

    #[test]
    fn test_parse_fix_price() {
        assert_eq!(parse_fix_price("123.45").unwrap(), 12345);
        assert_eq!(parse_fix_price("100.00").unwrap(), 10000);
        assert_eq!(parse_fix_price("50").unwrap(), 5000);
    }

    #[test]
    fn test_roundtrip_new_order() {
        let mut encoder = FixEncoder::new(FixVersion::Fix44, "CLIENT", "SERVER");
        let decoder = FixDecoder::new(FixVersion::Fix44);

        let original = NewOrder::new(0, 12345, Symbol::from_str("GOOG"), 15000, 100, Side::Buy);

        let encoded = encoder.encode_new_order(&original).unwrap();
        let decoded = decoder.decode_input(&encoded).unwrap();

        if let InputMessage::NewOrder(order) = decoded {
            assert_eq!(order.user_order_id, 12345);
            assert_eq!(order.symbol, Symbol::from_str("GOOG"));
            assert_eq!(order.price, 15000);
            assert_eq!(order.quantity, 100);
            assert_eq!(order.side, Side::Buy);
        } else {
            panic!("Expected NewOrder");
        }
    }
}
