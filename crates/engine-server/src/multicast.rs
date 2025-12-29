//! Multicast publisher for market data.
//!
//! # Wire Format
//! Each multicast packet contains:
//! - Sequence number (8 bytes, big-endian)
//! - Message length (4 bytes, big-endian)
//! - Message payload (variable, max 34 bytes)
//!
//! # Power of Ten Compliance
//! - Rule 3: Fixed-size packet buffer, no allocation.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;

use engine_core::OutputMessage;
use engine_protocol::binary_codec::encode_output_to_buf;
use engine_protocol::wire_types::MAX_OUTPUT_WIRE_SIZE;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;

use crate::config::Config;
use crate::metrics::Metrics;
use crate::types::MulticastRx;

/// Maximum multicast packet size.
/// 8 (seq) + 4 (len) + 34 (max message) = 46, round up to 64.
const MAX_MCAST_PACKET_SIZE: usize = 64;

// Compile-time verification
const _: () = assert!(8 + 4 + MAX_OUTPUT_WIRE_SIZE <= MAX_MCAST_PACKET_SIZE);

/// Run the multicast publisher.
pub async fn run_multicast_publisher(
    config: Arc<Config>,
    mut rx: MulticastRx,
    metrics: Arc<Metrics>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Create socket with socket2 for multicast options
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;

    // Set multicast options
    socket.set_multicast_ttl_v4(config.multicast_ttl)?;
    socket.set_multicast_loop_v4(false)?; // Don't receive our own packets

    // Bind to any address (we're sending, not receiving)
    let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
    socket.bind(&bind_addr.into())?;

    // Convert to tokio socket
    socket.set_nonblocking(true)?;
    let socket = UdpSocket::from_std(socket.into())?;
    let mcast_addr = SocketAddrV4::new(config.multicast_group, config.multicast_port);

    eprintln!(
        "Multicast publisher started on {}:{} (TTL={})",
        config.multicast_group, config.multicast_port, config.multicast_ttl
    );

    // Fixed-size packet buffer - NO ALLOCATION in loop
    let mut packet = [0u8; MAX_MCAST_PACKET_SIZE];
    let mut seq_num: u64 = 0;

    while let Some(msg) = rx.recv().await {
        // Only publish market data (trades and TOB)
        let should_publish = matches!(
            msg,
            OutputMessage::Trade(_) | OutputMessage::TopOfBook(_)
        );

        if !should_publish {
            continue;
        }

        // Encode message into packet buffer (after header space)
        let msg_offset = 8 + 4; // seq_num + len
        let msg_buf = &mut packet[msg_offset..];

        let msg_len = match encode_output_to_buf(&msg, msg_buf) {
            Ok(len) => len,
            Err(e) => {
                eprintln!("Multicast encode error: {:?}", e);
                continue;
            }
        };

        // Write header
        packet[0..8].copy_from_slice(&seq_num.to_be_bytes());
        packet[8..12].copy_from_slice(&(msg_len as u32).to_be_bytes());

        let total_len = msg_offset + msg_len;

        // Send
        match socket.send_to(&packet[..total_len], mcast_addr).await {
            Ok(_) => {
                Metrics::inc(&metrics.multicast_messages);
                seq_num = seq_num.wrapping_add(1);
            }
            Err(e) => {
                eprintln!("Multicast send error: {}", e);
                metrics.record_send_error();
            }
        }
    }

    eprintln!("Multicast publisher stopped (seq={})", seq_num);
    Ok(())
}
