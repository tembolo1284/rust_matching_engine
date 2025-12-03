//! Multicast publisher for market data.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;

use engine_core::OutputMessage;
use engine_protocol::binary_codec::BinaryEncoder;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;

use crate::config::Config;
use crate::metrics::Metrics;
use crate::types::MulticastRx;

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

    // Pre-allocated encoder
    let mut encoder = BinaryEncoder::new();
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

        // Encode message
        let frame = match encoder.encode_output(&msg) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Multicast encode error: {:?}", e);
                continue;
            }
        };

        // Build multicast packet with sequence number
        // Format: [seq_num: u64 BE] [len: u32 BE] [frame]
        let mut packet = Vec::with_capacity(8 + 4 + frame.len());
        packet.extend_from_slice(&seq_num.to_be_bytes());
        packet.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        packet.extend_from_slice(frame);

        // Send
        match socket.send_to(&packet, mcast_addr).await {
            Ok(_) => {
                Metrics::inc(&metrics.multicast_messages);
                seq_num = seq_num.wrapping_add(1);
            }
            Err(e) => {
                eprintln!("Multicast send error: {}", e);
                Metrics::inc(&metrics.send_errors);
            }
        }
    }

    eprintln!("Multicast publisher stopped");
    Ok(())
}
