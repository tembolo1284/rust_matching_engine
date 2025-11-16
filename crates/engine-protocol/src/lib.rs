//! engine-protocol
//!
//! Wire-level encoding/decoding for the matching engine.
//!
//! This crate is responsible for turning logical engine messages
//! (`engine_core::InputMessage` / `OutputMessage`) into bytes and
//! back again.
//!
//! - [`binary_codec`] : binary wire protocol (for multi-client TCP)
//! - [`csv_codec`]    : CSV compatibility (for tools / replay)

pub mod wire_types;
pub mod binary_codec;
pub mod csv_codec;

pub use binary_codec::{
    ProtocolError,
    decode_input,
    encode_input,
    decode_output,
    encode_output,
};

