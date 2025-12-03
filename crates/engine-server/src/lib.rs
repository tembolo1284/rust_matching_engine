//! Multi-protocol TCP/UDP matching engine server.
//!
//! # Supported Transports & Protocols
//!
//! | Transport | Protocol | Direction | Use Case |
//! |-----------|----------|-----------|----------|
//! | TCP | CSV | Bidirectional | Testing, netcat |
//! | TCP | Binary | Bidirectional | High-performance clients |
//! | TCP | FIX 4.2/4.4 | Bidirectional | Institutional connectivity |
//! | UDP | CSV | Bidirectional | Simple UDP clients |
//! | UDP | Binary | Bidirectional | Ultra-low latency |
//! | Multicast | Binary | Server → Clients | Market data broadcast |
//!
//! # Message Routing
//!
//! - **Ack/CancelAck**: Unicast to originating client only
//! - **Trade**: Unicast to buyer + seller + multicast
//! - **TopOfBook**: Multicast only (market data feed)

pub mod config;
pub mod types;
pub mod server;
pub mod metrics;

mod client_tcp;
mod client_udp;
mod engine_task;
mod multicast;
mod router;
mod protocol_detect;

pub use config::Config;
pub use server::run;
