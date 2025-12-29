//! Multi-protocol matching engine server.
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────┐     ┌───────────────┐     ┌───────────────┐
//! │ TCP Clients   │────▶│               │────▶│ Client        │
//! └───────────────┘     │   Engine      │     │ Channels      │
//! ┌───────────────┐     │   Task        │     └───────────────┘
//! │ UDP Clients   │────▶│               │────▶┌───────────────┐
//! └───────────────┘     └───────────────┘     │ Multicast     │
//!                                             └───────────────┘
//! ```
//!
//! # Supported Protocols
//! - CSV: Human-readable format
//! - Binary: High-performance format (Zig/C compatible)
//! - FIX 4.4: Institutional connectivity
//!
//! # Power of Ten Compliance
//! - Bounded channels for backpressure
//! - Pre-allocated buffers where possible
//! - Explicit error handling

#![deny(warnings)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]

pub mod client_tcp;
pub mod client_udp;
pub mod config;
pub mod engine_task;
pub mod metrics;
pub mod multicast;
pub mod protocol_detect;
pub mod router;
pub mod server;
pub mod types;

pub use config::Config;
pub use metrics::Metrics;
pub use server::run;
pub use types::{ClientId, ClientRegistry, Protocol, Transport};
