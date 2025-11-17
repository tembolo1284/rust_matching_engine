//! engine-server
//!
//! Multi-client async TCP server for the matching engine.

pub mod config;
pub mod types;
pub mod server;

// these are internal modules, not re-exported
mod client;
mod engine_task;

