//! engine-server
//!
//! Multi-client async TCP server for the Rust matching engine.
//!
//! This crate glues together:
//! - `engine-core`
//! - `engine-protocol`
//! and exposes a `server::run(Config)` entrypoint.

pub mod config;
pub mod types;
pub mod server;

