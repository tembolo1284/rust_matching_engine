//! Per-client TCP handler.
//!
//! Responsibilities:
//! - Read length-prefixed binary frames from the socket.
//! - Decode them into `InputMessage` via `engine-protocol`.
//! - Send `EngineRequest` into the central engine loop.
//! - Concurrently receive `OutputMessage`s for this client and
//!   write them back as length-prefixed binary frames.

use std::error::Error;
use std::sync::Arc;

use engine_core::OutputMessage;
use engine_protocol::{decode_input, encode_output};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::select;
use tokio::sync::mpsc;
use tokio::sync::RwLock;

use crate::types::{ClientId, ClientRegistry, EngineRequest, EngineTx, OutboundRx};

/// Run the client I/O loop for a single connection.
///
/// - `client_id`: unique identifier for this connection.
/// - `stream`: the TCP stream.
/// - `engine_tx`: channel to send `EngineRequest`s to the engine task.
/// - `out_rx`: channel receiving `OutputMessage`s destined for this client.
/// - `clients`: shared registry to allow removal on disconnect.
pub async fn run_client(
    client_id: ClientId,
    mut stream: TcpStream,
    engine_tx: EngineTx,
    mut out_rx: OutboundRx,
    clients: ClientRegistry,
) -> Result<(), Box<dyn Error>> {
    let peer_addr = stream.peer_addr().ok();

    // Split stream into read/write halves if needed; here we just clone the stream for simplicity.
    let mut read_stream = stream.try_clone()?;
    let mut write_stream = stream;

    // Writer task: consume `OutputMessage`s and write frames.
    let writer_handle = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if let Err(e) = write_message(&mut write_stream, &msg).await {
                eprintln!("Client {} write error: {:?}", client_id.0, e);
                break;
            }
        }
    });

    // Reader loop: read frames, decode, and forward to engine.
    loop {
        // Read length prefix (u32 BE).
        let mut len_buf = [0u8; 4];
        if let Err(e) = read_stream.read_exact(&mut len_buf).await {
            // EOF or error means disconnect.
            eprintln!(
                "Client {} {:?} read error/EOF: {:?}",
                client_id.0, peer_addr, e
            );
            break;
        }
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        if frame_len == 0 {
            // Ignore empty frames (or treat as protocol error if you prefer).
            continue;
        }

        let mut frame = vec![0u8; frame_len];
        if let Err(e) = read_stream.read_exact(&mut frame).await {
            eprintln!(
                "Client {} {:?} read frame error: {:?}",
                client_id.0, peer_addr, e
            );
            break;
        }

        match decode_input(&frame) {
            Ok(input_msg) => {
                // Forward to engine.
                let req = EngineRequest {
                    client_id,
                    msg: input_msg,
                };
                if engine_tx.send(req).is_err() {
                    eprintln!("Engine channel closed, shutting down client {}", client_id.0);
                    break;
                }
            }
            Err(err) => {
                eprintln!("Client {} protocol decode error: {:?}", client_id.0, err);
                // You can choose to drop the connection or just ignore this frame.
                break;
            }
        }
    }

    // Remove client from registry.
    {
        let mut guard = clients.write().await;
        guard.remove(&client_id);
    }

    // Dropping out_rx will cause writer task to finish.
    // Wait for writer to shut down.
    let _ = writer_handle.await;

    Ok(())
}

async fn write_message(
    stream: &mut TcpStream,
    msg: &OutputMessage,
) -> Result<(), Box<dyn Error>> {
    let mut payload = Vec::with_capacity(128);
    encode_output(msg, &mut payload)?;

    let len = payload.len() as u32;
    let len_bytes = len.to_be_bytes();

    stream.write_all(&len_bytes).await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;

    Ok(())
}

