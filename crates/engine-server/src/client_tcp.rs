//! TCP client handler supporting CSV, Binary, and FIX protocols.

use std::sync::Arc;
use std::time::Duration;

use engine_core::{InputMessage, OutputMessage};
use engine_protocol::{binary_codec, csv_codec, fix_codec};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::config::Config;
use crate::metrics::Metrics;
use crate::protocol_detect::detect_protocol;
use crate::types::{
    ClientId, ClientInfo, ClientRegistry, EngineRequest, EngineTx, Protocol, Transport,
};

/// Handle a single TCP client connection.
pub async fn handle_tcp_client(
    client_id: ClientId,
    stream: TcpStream,
    config: Arc<Config>,
    clients: Arc<ClientRegistry>,
    engine_tx: EngineTx,
    metrics: Arc<Metrics>,
) {
    let peer_addr = stream.peer_addr().unwrap_or_else(|_| "unknown".parse().unwrap());

    Metrics::inc(&metrics.tcp_connections_total);
    Metrics::inc(&metrics.tcp_connections_active);

    // Set TCP options
    let _ = stream.set_nodelay(true);

    // Split stream
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::with_capacity(config.tcp_read_buffer_size, read_half);

    // Peek to detect protocol
    let mut peek_buf = [0u8; 8];
    let protocol = match reader.fill_buf().await {
        Ok(buf) => {
            let len = buf.len().min(8);
            peek_buf[..len].copy_from_slice(&buf[..len]);
            let hex: String = peek_buf[..len]
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");
            eprintln!("{}: peeked {} bytes: {}", client_id, len, hex);
            detect_protocol(&peek_buf[..len])
        }
        Err(_) => Protocol::Csv,
    };

    eprintln!("{}: connected from {} using {:?}", client_id, peer_addr, protocol);

    // Create outbound channel (bounded!)
    let (out_tx, mut out_rx): (mpsc::Sender<OutputMessage>, mpsc::Receiver<OutputMessage>) =
        mpsc::channel(config.client_channel_capacity);

    // Register client
    let client_info = ClientInfo {
        id: client_id,
        addr: peer_addr,
        transport: Transport::Tcp,
        protocol,
        user_id: None,
    };
    clients.register(client_info, out_tx).await;

    // Spawn writer task
    let writer_protocol = protocol;
    let writer_metrics = metrics.clone();
    let write_timeout = config.write_timeout;

    let writer_handle = tokio::spawn(async move {
        let mut encoder = binary_codec::BinaryEncoder::new();
        let mut fix_encoder = fix_codec::FixEncoder::new(
            fix_codec::FixVersion::Fix44,
            "ENGINE",
            "CLIENT",
        );

        while let Some(msg) = out_rx.recv().await {
            let result = match writer_protocol {
                Protocol::Csv => {
                    let line = csv_codec::format_output_csv(&msg);
                    let data = format!("{}\n", line);
                    timeout(write_timeout, write_half.write_all(data.as_bytes())).await
                }
                Protocol::Binary => {
                    match encoder.encode_output(&msg) {
                        Ok(frame) => {
                            // Length prefix + frame
                            let len = (frame.len() as u32).to_be_bytes();
                            let mut buf = Vec::with_capacity(4 + frame.len());
                            buf.extend_from_slice(&len);
                            buf.extend_from_slice(frame);
                            timeout(write_timeout, write_half.write_all(&buf)).await
                        }
                        Err(_) => continue,
                    }
                }
                Protocol::Fix => {
                    match fix_encoder.encode_output(&msg) {
                        Ok(frame) => {
                            timeout(write_timeout, write_half.write_all(&frame)).await
                        }
                        Err(_) => continue,
                    }
                }
            };

            match result {
                Ok(Ok(_)) => Metrics::inc(&writer_metrics.messages_sent),
                _ => {
                    Metrics::inc(&writer_metrics.send_errors);
                    break;
                }
            }
        }
    });

    // Reader loop
    let read_result = match protocol {
        Protocol::Csv => {
            read_csv_loop(client_id, &mut reader, &engine_tx, &metrics, config.read_timeout).await
        }
        Protocol::Binary => {
            read_binary_loop(client_id, &mut reader, &engine_tx, &metrics, config.read_timeout).await
        }
        Protocol::Fix => {
            read_fix_loop(client_id, &mut reader, &engine_tx, &metrics, config.read_timeout).await
        }
    };

    if let Err(e) = read_result {
        // Don't log "connection reset" as an error - it's normal client disconnect
        if e.contains("Connection reset") || e.contains("eof") {
            eprintln!("{}: client disconnected", client_id);
        } else {
            eprintln!("{}: read error: {}", client_id, e);
        }
    }
    // Cleanup
    clients.unregister(client_id).await;
    writer_handle.abort();

    Metrics::dec(&metrics.tcp_connections_active);
    eprintln!("{}: disconnected", client_id);
}

async fn read_csv_loop<R: AsyncBufReadExt + Unpin>(
    client_id: ClientId,
    reader: &mut R,
    engine_tx: &EngineTx,
    metrics: &Arc<Metrics>,
    read_timeout: Duration,
) -> Result<(), String> {
    let mut line = String::with_capacity(256);

    loop {
        line.clear();

        let read_result = timeout(read_timeout, reader.read_line(&mut line)).await;

        match read_result {
            Ok(Ok(0)) => return Ok(()), // EOF
            Ok(Ok(_)) => {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }

                match csv_codec::parse_input_line(trimmed) {
                    Some(msg) => {
                        let user_id = extract_user_id(&msg);
                        let request = EngineRequest {
                            client_id,
                            user_id,
                            msg,
                        };

                        if engine_tx.send(request).await.is_err() {
                            return Err("engine channel closed".to_string());
                        }
                    }
                    None => {
                        Metrics::inc(&metrics.decode_errors);
                        eprintln!("{}: invalid CSV: {}", client_id, trimmed);
                    }
                }
            }
            Ok(Err(e)) => return Err(format!("read error: {}", e)),
            Err(_) => return Err("read timeout".to_string()),
        }
    }
}

async fn read_binary_loop<R: AsyncReadExt + Unpin>(
    client_id: ClientId,
    reader: &mut R,
    engine_tx: &EngineTx,
    metrics: &Arc<Metrics>,
    read_timeout: Duration,
) -> Result<(), String> {
    let decoder = binary_codec::BinaryDecoder::new();
    let mut len_buf = [0u8; 4];
    let mut payload_buf = vec![0u8; 256];

    // eprintln!("{}: entering binary read loop", client_id);

    loop {
        // Read 4-byte length prefix
        let len_result = timeout(read_timeout, reader.read_exact(&mut len_buf)).await;

        match len_result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Ok(Err(e)) => return Err(format!("read error: {}", e)),
            Err(_) => return Err("read timeout".to_string()),
        }

        let frame_len = u32::from_be_bytes(len_buf) as usize;

        // Debug: show length prefix
        // eprintln!("{}: len_prefix bytes: {:02X} {:02X} {:02X} {:02X} = {} bytes",
           //  client_id, len_buf[0], len_buf[1], len_buf[2], len_buf[3], frame_len);

        // Sanity check frame length
        if frame_len == 0 {
            eprintln!("{}: zero-length frame, skipping", client_id);
            continue;
        }
        if frame_len > 65536 {
            return Err(format!("frame too large: {} bytes", frame_len));
        }

        // Resize buffer if needed
        if payload_buf.len() < frame_len {
            payload_buf.resize(frame_len, 0);
        }

        // Read frame payload
        let payload_result = timeout(
            read_timeout,
            reader.read_exact(&mut payload_buf[..frame_len]),
        )
        .await;

        match payload_result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(format!("payload read error: {}", e)),
            Err(_) => return Err("payload read timeout".to_string()),
        }

        // Decode message - returns (InputMessage, usize), extract just the message
        match decoder.decode_input(&payload_buf[..frame_len]) {
            Ok((msg, _bytes_consumed)) => {
                let user_id = extract_user_id(&msg);
                let request = EngineRequest {
                    client_id,
                    user_id,
                    msg,
                };

                if engine_tx.send(request).await.is_err() {
                    return Err("engine channel closed".to_string());
                }
            }
            Err(e) => {
                Metrics::inc(&metrics.decode_errors);
                // Debug: show raw bytes for diagnosis
                let hex: String = payload_buf[..frame_len.min(32)]
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(" ");
                eprintln!("{}: decode error: {:?}", client_id, e);
                eprintln!("{}: raw bytes (len={}): {}", client_id, frame_len, hex);
            }
        }
    }
}

async fn read_fix_loop<R: AsyncBufReadExt + Unpin>(
    client_id: ClientId,
    reader: &mut R,
    engine_tx: &EngineTx,
    metrics: &Arc<Metrics>,
    read_timeout: Duration,
) -> Result<(), String> {
    let decoder = fix_codec::FixDecoder::new(fix_codec::FixVersion::Fix44);
    let mut buf = Vec::with_capacity(4096);

    loop {
        buf.clear();

        // Read until SOH (0x01) delimiter - FIX messages end with checksum field
        // For simplicity, read line-by-line and look for complete messages
        let read_result = timeout(read_timeout, read_fix_message(reader, &mut buf)).await;

        match read_result {
            Ok(Ok(0)) => return Ok(()), // EOF
            Ok(Ok(_)) => {
                match decoder.decode_input(&buf) {
                    Ok(msg) => {
                        let user_id = extract_user_id(&msg);
                        let request = EngineRequest {
                            client_id,
                            user_id,
                            msg,
                        };

                        if engine_tx.send(request).await.is_err() {
                            return Err("engine channel closed".to_string());
                        }
                    }
                    Err(e) => {
                        Metrics::inc(&metrics.decode_errors);
                        eprintln!("{}: FIX decode error: {:?}", client_id, e);
                    }
                }
            }
            Ok(Err(e)) => return Err(format!("read error: {}", e)),
            Err(_) => return Err("read timeout".to_string()),
        }
    }
}

/// Read a complete FIX message (terminated by checksum field).
async fn read_fix_message<R: AsyncReadExt + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> std::io::Result<usize> {
    // FIX messages: 8=FIX.4.4|9=len|...|10=xxx|
    // Read byte by byte looking for 10=XXX pattern followed by SOH
    let mut total = 0;
    let mut temp = [0u8; 1];

    loop {
        let n = reader.read(&mut temp).await?;
        if n == 0 {
            return Ok(0); // EOF
        }

        buf.push(temp[0]);
        total += 1;

        // Check if we have a complete message (ends with 10=XXX<SOH>)
        if buf.len() >= 7 {
            let len = buf.len();
            // Look for |10=XXX| pattern
            if buf[len - 1] == 0x01 && len >= 7 {
                // Check for 10= pattern
                if let Some(pos) = buf.windows(3).rposition(|w| w == b"10=") {
                    if pos + 7 <= len {
                        return Ok(total);
                    }
                }
            }
        }

        // Safety limit
        if total > 8192 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "FIX message too large",
            ));
        }
    }
}

/// Extract user_id from an input message for routing.
fn extract_user_id(msg: &InputMessage) -> u32 {
    match msg {
        InputMessage::NewOrder(order) => order.user_id,
        InputMessage::Cancel(cancel) => cancel.user_id,
        InputMessage::Flush => 0,
        InputMessage::QueryTopOfBook(_) => 0,
    }
}
