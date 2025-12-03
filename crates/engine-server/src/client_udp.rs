//! UDP client handler supporting CSV and Binary protocols.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_core::{InputMessage, OutputMessage};
use engine_protocol::{binary_codec, csv_codec};
#[allow(unused_imports)]
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::metrics::Metrics;
use crate::protocol_detect::detect_protocol;
use crate::server::bind_udp_with_retry;
use crate::types::{
    ClientId, ClientInfo, ClientRegistry, EngineRequest, EngineTx, Protocol, Transport,
};

/// UDP client tracking entry.
struct UdpClient {
    client_id: ClientId,
    protocol: Protocol,
    last_seen: Instant,
}

/// Run the UDP server.
pub async fn run_udp_server(
    config: Arc<Config>,
    clients: Arc<ClientRegistry>,
    engine_tx: EngineTx,
    metrics: Arc<Metrics>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (socket, actual_port) =
        bind_udp_with_retry(&config.udp_bind_addr, config.udp_port).await?;
    let socket = Arc::new(socket);

    eprintln!("UDP server listening on {}:{}", config.udp_bind_addr, actual_port);

    // Track UDP "clients" by address
    let mut udp_clients: HashMap<SocketAddr, UdpClient> = HashMap::new();

    // Pre-allocated buffers
    let mut recv_buf = vec![0u8; config.udp_buffer_size];

    // Cleanup interval
    let cleanup_interval = Duration::from_secs(60);
    let mut last_cleanup = Instant::now();

    loop {
        // Receive datagram
        let (len, peer_addr) = match socket.recv_from(&mut recv_buf).await {
            Ok(result) => result,
            Err(e) => {
                eprintln!("UDP recv error: {}", e);
                continue;
            }
        };

        let data = &recv_buf[..len];

        // Get or create client entry
        let client_entry = udp_clients.entry(peer_addr).or_insert_with(|| {
            let client_id = ClientId::next();
            let protocol = detect_protocol(data);

            eprintln!("UDP client {} from {} using {:?}", client_id, peer_addr, protocol);
            Metrics::inc(&metrics.udp_clients_active);

            // Create outbound channel
            let (out_tx, mut out_rx) = mpsc::channel::<OutputMessage>(config.client_channel_capacity);

            // Register in client registry
            let client_info = ClientInfo {
                id: client_id,
                addr: peer_addr,
                transport: Transport::Udp,
                protocol,
                user_id: None,
            };

            // Spawn task to handle outbound messages for this UDP client
            let socket_clone = Arc::clone(&socket);
            let client_protocol = protocol;
            let metrics_clone = metrics.clone();

            tokio::spawn(async move {
                let mut encoder = binary_codec::BinaryEncoder::new();

                while let Some(msg) = out_rx.recv().await {
                    let send_result = match client_protocol {
                        Protocol::Csv => {
                            let line = format!("{}\n", csv_codec::format_output_csv(&msg));
                            socket_clone.send_to(line.as_bytes(), peer_addr).await
                        }
                        Protocol::Binary => {
                            if let Ok(frame) = encoder.encode_output(&msg) {
                                let len_bytes = (frame.len() as u32).to_be_bytes();
                                let mut buf = Vec::with_capacity(4 + frame.len());
                                buf.extend_from_slice(&len_bytes);
                                buf.extend_from_slice(frame);
                                socket_clone.send_to(&buf, peer_addr).await
                            } else {
                                continue;
                            }
                        }
                        Protocol::Fix => continue, // FIX not supported over UDP
                    };

                    match send_result {
                        Ok(_) => Metrics::inc(&metrics_clone.messages_sent),
                        Err(_) => Metrics::inc(&metrics_clone.send_errors),
                    }
                }
            });

            // Register client
            let clients_clone = clients.clone();
            tokio::spawn(async move {
                clients_clone.register(client_info, out_tx).await;
            });

            UdpClient {
                client_id,
                protocol,
                last_seen: Instant::now(),
            }
        });

        client_entry.last_seen = Instant::now();
        let client_id = client_entry.client_id;
        let protocol = client_entry.protocol;

        // Parse message based on protocol
        let parse_result = match protocol {
            Protocol::Csv => {
                let text = String::from_utf8_lossy(data);
                csv_codec::parse_input_line(text.trim())
            }
            Protocol::Binary => {
                // Skip length prefix if present (UDP doesn't need it but client might send it)
                let payload = if data.len() > 4 && &data[0..4] != b"MENG" {
                    &data[4..]
                } else {
                    data
                };

                binary_codec::decode_input(payload).ok()
            }
            Protocol::Fix => None, // FIX not supported over UDP
        };

        match parse_result {
            Some(msg) => {
                let user_id = match &msg {
                    InputMessage::NewOrder(o) => o.user_id,
                    InputMessage::Cancel(c) => c.user_id,
                    _ => 0,
                };

                let request = EngineRequest {
                    client_id,
                    user_id,
                    msg,
                };

                if engine_tx.send(request).await.is_err() {
                    eprintln!("Engine channel closed");
                    break;
                }
            }
            None => {
                Metrics::inc(&metrics.decode_errors);
            }
        }

        // Periodic cleanup of stale UDP clients
        if last_cleanup.elapsed() > cleanup_interval {
            let timeout = config.idle_timeout;
            let stale: Vec<SocketAddr> = udp_clients
                .iter()
                .filter(|(_, c)| c.last_seen.elapsed() > timeout)
                .map(|(addr, _)| *addr)
                .collect();

            for addr in stale {
                if let Some(client) = udp_clients.remove(&addr) {
                    eprintln!("UDP client {} timed out", client.client_id);
                    clients.unregister(client.client_id).await;
                    Metrics::dec(&metrics.udp_clients_active);
                }
            }

            last_cleanup = Instant::now();
        }
    }

    Ok(())
}
