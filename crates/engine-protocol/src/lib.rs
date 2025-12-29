//! engine-protocol
//!
//! Wire-level encoding/decoding for the matching engine.
//!
//! # Protocol Support
//! - `binary_codec`: High-performance binary protocol (matches C/Zig).
//! - `csv_codec`: Human-readable CSV format.
//! - `fix_codec`: FIX 4.2/4.4 institutional protocol.
//!
//! # Power of Ten Compliance
//! - Zero allocation in binary encode/decode hot path.
//! - Fixed-size buffers for wire messages.
//! - Bounded parsing for CSV.

#![deny(warnings)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]

pub mod binary_codec;
pub mod csv_codec;
pub mod fix_codec;
pub mod wire_types;

// Re-exports for convenient access
pub use binary_codec::{
    // Zero-allocation API
    decode_input,
    decode_output,
    encode_input_to_buf,
    encode_output_to_buf,
    BinaryDecoder,
    BinaryEncoder,
    InputEncodeBuffer,
    OutputEncodeBuffer,
    ProtocolError,
    // Legacy allocating API
    encode_input,
    encode_output,
};

pub use csv_codec::{
    format_output_csv,
    format_output_legacy,
    parse_input_line,
    CsvFormatBuffer,
    CsvParser,
};

pub use fix_codec::{FixDecoder, FixEncoder, FixError, FixVersion};

pub use wire_types::{
    WireInputType, WireOutputType, 
    MAGIC_BYTE, SYMBOL_SIZE,
    MAX_INPUT_WIRE_SIZE, MAX_OUTPUT_WIRE_SIZE,
    NEW_ORDER_WIRE_SIZE, CANCEL_WIRE_SIZE, FLUSH_WIRE_SIZE,
    ACK_WIRE_SIZE, CANCEL_ACK_WIRE_SIZE, TRADE_WIRE_SIZE, 
    TOP_OF_BOOK_WIRE_SIZE, REJECT_WIRE_SIZE,
};

// Compile-time verification
const _: () = assert!(MAX_OUTPUT_WIRE_SIZE == 34);
const _: () = assert!(MAX_INPUT_WIRE_SIZE == 27);
