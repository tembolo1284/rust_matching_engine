//! Shared types for the engine server.
//!
//! # Power of Ten Compliance
//! - Rule 3: Bounded collections where possible.
//! - Rule 5: Assertions on state transitions.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use engine_core::{InputMessage, OutputMessage};
use rustc_hash::FxHashMap;
use tokio::sync::{mpsc, RwLock};

// =============================================================================
// Client Identification
// =============================================================================

/// Unique client identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub u64);

impl ClientId {
    /// Generate the next unique client ID.
    pub fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        debug_assert!(id > 0, "client ID overflow");
        ClientId(id)
    }

    /// Create from raw value (for testing).
    #[cfg(test)]
    pub const fn from_raw(id: u64) -> Self {
        ClientId(id)
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Client({})", self.0)
    }
}

// =============================================================================
// Transport and Protocol
// =============================================================================

/// Transport type for a client connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Udp,
}

/// Protocol type for message encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Csv,
    Binary,
    Fix,
}

// =============================================================================
// Client Information
// =============================================================================

/// Information about a connected client.
#[derive(Debug, Clone)]
pub struct ClientInfo {
    /// Unique client identifier.
    pub id: ClientId,
    /// Client's socket address.
    pub addr: SocketAddr,
    /// Transport type.
    pub transport: Transport,
    /// Protocol type.
    pub protocol: Protocol,
    /// User ID (for order routing) - set after first order.
    pub user_id: Option<u32>,
}

// =============================================================================
// Channel Types
// =============================================================================

/// Channel for sending outputs to a client.
/// OutputMessage is Copy, so no allocation on send.
pub type OutboundTx = mpsc::Sender<OutputMessage>;
pub type OutboundRx = mpsc::Receiver<OutputMessage>;

/// Channel for sending requests to the engine.
pub type EngineTx = mpsc::Sender<EngineRequest>;
pub type EngineRx = mpsc::Receiver<EngineRequest>;

/// Channel for multicast publishing.
pub type MulticastTx = mpsc::Sender<OutputMessage>;
pub type MulticastRx = mpsc::Receiver<OutputMessage>;

// =============================================================================
// Engine Request
// =============================================================================

/// Request from a client to the engine.
#[derive(Debug)]
pub struct EngineRequest {
    /// Originating client.
    pub client_id: ClientId,
    /// User ID from the message (for routing responses).
    pub user_id: u32,
    /// The input message.
    pub msg: InputMessage,
}

// Verify EngineRequest is reasonably sized
const _: () = assert!(std::mem::size_of::<EngineRequest>() <= 64);

// =============================================================================
// Client Registry
// =============================================================================

/// Client entry in the registry.
pub struct ClientEntry {
    /// Client information.
    pub info: ClientInfo,
    /// Outbound channel sender.
    pub tx: OutboundTx,
}

/// Thread-safe client registry.
///
/// Uses FxHashMap for faster hashing than std HashMap.
#[derive(Default)]
pub struct ClientRegistry {
    /// ClientId → ClientEntry
    clients: RwLock<FxHashMap<ClientId, ClientEntry>>,
    /// UserId → ClientId mapping for routing responses.
    user_to_client: RwLock<FxHashMap<u32, ClientId>>,
}

impl ClientRegistry {
    /// Create a new registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with pre-allocated capacity.
    pub fn with_capacity(client_capacity: usize, user_capacity: usize) -> Self {
        let mut clients = FxHashMap::default();
        clients.reserve(client_capacity);

        let mut user_to_client = FxHashMap::default();
        user_to_client.reserve(user_capacity);

        ClientRegistry {
            clients: RwLock::new(clients),
            user_to_client: RwLock::new(user_to_client),
        }
    }

    /// Register a new client.
    pub async fn register(&self, info: ClientInfo, tx: OutboundTx) {
        debug_assert!(info.id.0 != 0, "invalid client ID");

        let client_id = info.id;
        let entry = ClientEntry { info, tx };

        let mut clients = self.clients.write().await;
        clients.insert(client_id, entry);
    }

    /// Unregister a client.
    pub async fn unregister(&self, client_id: ClientId) {
        let mut clients = self.clients.write().await;
        if let Some(entry) = clients.remove(&client_id) {
            drop(clients); // Release lock before acquiring next

            // Also remove user mapping if present.
            if let Some(user_id) = entry.info.user_id {
                let mut user_map = self.user_to_client.write().await;
                user_map.remove(&user_id);
            }
        }
    }

    /// Associate a user ID with a client.
    pub async fn set_user_id(&self, client_id: ClientId, user_id: u32) {
        if user_id == 0 {
            return; // Don't track zero user IDs
        }

        // Update client info
        {
            let mut clients = self.clients.write().await;
            if let Some(entry) = clients.get_mut(&client_id) {
                entry.info.user_id = Some(user_id);
            }
        }

        // Update user → client mapping
        {
            let mut user_map = self.user_to_client.write().await;
            user_map.insert(user_id, client_id);
        }
    }

    /// Get the client ID for a user ID.
    pub async fn get_client_for_user(&self, user_id: u32) -> Option<ClientId> {
        let user_map = self.user_to_client.read().await;
        user_map.get(&user_id).copied()
    }

    /// Send a message to a specific client.
    ///
    /// Note: OutputMessage is Copy, so this doesn't allocate.
    pub async fn send_to_client(&self, client_id: ClientId, msg: OutputMessage) -> bool {
        let clients = self.clients.read().await;
        if let Some(entry) = clients.get(&client_id) {
            // try_send to avoid blocking the engine
            match entry.tx.try_send(msg) {
                Ok(_) => true,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    // Channel full - client is slow
                    false
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    // Client disconnected
                    false
                }
            }
        } else {
            false
        }
    }

    /// Send a message to a user (by user_id).
    pub async fn send_to_user(&self, user_id: u32, msg: OutputMessage) -> bool {
        if let Some(client_id) = self.get_client_for_user(user_id).await {
            self.send_to_client(client_id, msg).await
        } else {
            false
        }
    }

    /// Broadcast to all clients.
    pub async fn broadcast(&self, msg: OutputMessage) {
        let clients = self.clients.read().await;
        for entry in clients.values() {
            let _ = entry.tx.try_send(msg); // Non-blocking
        }
    }

    /// Get current client count.
    pub async fn client_count(&self) -> usize {
        let clients = self.clients.read().await;
        clients.len()
    }

    /// Get all client IDs.
    pub async fn client_ids(&self) -> Vec<ClientId> {
        let clients = self.clients.read().await;
        clients.keys().copied().collect()
    }
}

// =============================================================================
// Server State
// =============================================================================

/// Shared state for the server.
pub struct ServerState {
    /// Client registry.
    pub clients: Arc<ClientRegistry>,
    /// Engine request channel.
    pub engine_tx: EngineTx,
    /// Multicast channel.
    pub multicast_tx: Option<MulticastTx>,
}
