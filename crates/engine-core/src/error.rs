//! Error types for the core matching engine.
//!
//! Right now the core engine API is designed to be infallible for
//! normal operations (invalid input should generally be filtered out
//! at the parsing / protocol layer).
//!
//! This module is a placeholder for future extensions where you might
//! want to return rich errors from certain admin operations.

/// Placeholder error type for the engine.
///
/// Currently unused, but kept for future-proofing in case we add
/// admin APIs (e.g. “drop symbol”, “replay snapshot”, etc.) that
/// can fail for well-defined reasons.
#[derive(Debug)]
pub enum EngineError {
    /// The requested symbol does not exist.
    UnknownSymbol(String),

    /// A generic internal error (e.g. invariant violation).
    Internal(String),
}

