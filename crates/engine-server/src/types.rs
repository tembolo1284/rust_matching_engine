//! Shared types for the engine TCP server.
//!
//! This module defines:
//! - `ClientId`: a lightweight handle for connected clients
//! - channel aliases between clients and the engine loop
//! - `EngineRequest`: messages flowing from clients to the engine

use std::collections::HashMap;
use std::sync::Arc;

use engine_core::{InputMessage, OutputMessage};
use tokio::sync::mpsc;
use tokio::sync::RwLock;

/// Identifier for a connected client.
///
/// This is intentionally opaque; we just guarantee uniqueness
/// over the lifetime of the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub u64);

/// Outbound messages from the engine to a given client.
pub type OutboundTx = mpsc::UnboundedSender<OutputMessage>;
pub type OutboundRx = mpsc::UnboundedReceiver<OutputMessage>;

/// Registry of connected clients and their outbound channels.
///
/// - Key: `ClientId`
/// - Value: `OutboundTx` to send `OutputMessage`s to that client.
pub type ClientRegistry = Arc<RwLock<HashMap<ClientId, OutboundTx>>>;

/// Message flowing from a client task into the central engine task.
#[derive(Debug)]
pub struct EngineRequest {
    pub client_id: ClientId,
    pub msg: InputMessage,
}

/// Channel from clients → engine task.
pub type EngineTx = mpsc::UnboundedSender<EngineRequest>;
pub type EngineRx = mpsc::UnboundedReceiver<EngineRequest>;

