//! Trading client library with auto-discovery and load testing.

pub mod app;
pub mod components;
pub mod discovery;
pub mod load_test;
pub mod network;
pub mod types;
pub mod ui;

pub use app::App;
pub use discovery::{discover_server, ServerCapabilities};
pub use network::EngineConnection;
pub use types::*;
