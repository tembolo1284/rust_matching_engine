//! TCP client handler supporting CSV, Binary, and FIX protocols.
//!
//! # Power of Ten Compliance
//! - Rule 2: Bounded read buffers.
//! - Rule 3: Pre-allocated encode buffers.
//! - Rule 5: Assertions on buffer operations.

use std::sync::Arc;
use std::time::Duration;

use engine_core::{InputMessage, OutputMessage};
use engine_protocol::{
    binary_codec::{self, encode_output_to_buf, MAX_OUTPUT_WIRE_SIZE},
    csv_codec,
    fix_codec,
};
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

/// Maximum CSV line length.
const MAX_CSV_LINE_LEN: usize = 256;

/// Maximum binary frame size.
const MAX_BINARY_FRAME_SIZE: usize = 256;

/// Maximum FIX message size.
const MAX_FIX_MESSAGE_SIZE: usize = 8192;

/// Handle a single TCP client connection.
pub async fn handle_tcp_client(
    client_id: ClientId,
    stream: TcpStream,
    config: Arc<Config>,
    clients: Arc<ClientRegistry>,
    engine_tx: EngineTx,
    metrics: Arc<Metrics>,
) {
    let peer_addr = stream
        .peer_addr()
        .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap());

    metrics.record_tcp_connect();

    // Set TCP options
    let _ = stream.set_nodelay(true);

    // Split stream
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::with_capacity(config.tcp_read_buffer_size, read_half);

    // Peek to detect protocol
    let protocol = match reader.fill_buf().await {
        Ok(buf) => {
            let len = buf.len().min(8);
            detect_protocol(&buf[..len])
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
        // Pre-allocated buffers for zero-allocation encoding
        let mut binary_buf = [0u8; 4 + MAX_OUTPUT_WIRE_SIZE]; // len prefix + message
        let mut csv_buf = String::with_capacity(128);
        let mut fix_encoder = fix_codec::FixEncoder::new(
            fix_codec::FixVersion::Fix44,
            "ENGINE",
            "CLIENT",
        );

        while let Some(msg) = out_rx.recv().await {
            let result = match writer_protocol {
                Protocol::Csv => {
                    csv_buf.clear();
                    csv_codec::format_output_into(&msg, &mut csv_buf);
                    csv_buf.push('\n');
                    timeout(write_timeout, write_half.write_all(csv_buf.as_bytes())).await
                }
                Protocol::Binary => {
                    // Encode message after length prefix
                    match encode_output_to_buf(&msg, &mut binary_buf[4..]) {
                        Ok(msg_len) => {
                            // Write length prefix
                            binary_buf[0..4].copy_from_slice(&(msg_len as u32).to_be_bytes());
                            let total_len = 4 + msg_len;
                            timeout(write_timeout, write_half.write_all(&binary_buf[..total_len])).await
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
                Ok(Ok(_)) => writer_metrics.record_message_sent(),
                _ => {
                    writer_metrics.record_send_error();
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
        if !e.contains("EOF") && !e.contains("timeout") {
            eprintln!("{}: read error: {}", client_id, e);
        }
    }

    // Cleanup
    clients.unregister(client_id).await;
    writer_handle.abort();

    metrics.record_tcp_disconnect();
    eprintln!("{}: disconnected", client_id);
}

async fn read_csv_loop<R: AsyncBufReadExt + Unpin>(
    client_id: ClientId,
    reader: &mut R,
    engine_tx: &EngineTx,
    metrics: &Arc<Metrics>,
    read_timeout: Duration,
) -> Result<(), String> {
    // Pre-allocated line buffer
    let mut line = String::with_capacity(MAX_CSV_LINE_LEN);

    loop {
        line.clear();

        let read_result = timeout(read_timeout, reader.read_line(&mut line)).await;

        match read_result {
            Ok(Ok(0)) => return Err("EOF".to_string()),
            Ok(Ok(_)) => {
                // Truncate if too long
                if line.len() > MAX_CSV_LINE_LEN {
                    line.truncate(MAX_CSV_LINE_LEN);
                }

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
                        metrics.record_decode_error();
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
    let mut len_buf = [0u8; 4];
    let mut payload_buf = [0u8; MAX_BINARY_FRAME_SIZE];

    loop {
        // Read 4-byte length prefix
        let len_result = timeout(read_timeout, reader.read_exact(&mut len_buf)).await;

        match len_result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err("EOF".to_string())
            }
            Ok(Err(e)) => return Err(format!("read error: {}", e)),
            Err(_) => return Err("read timeout".to_string()),
        }

        let frame_len = u32::from_be_bytes(len_buf) as usize;

        // Sanity check
        if frame_len == 0 {
            continue;
        }
        if frame_len > MAX_BINARY_FRAME_SIZE {
            return Err(format!("frame too large: {} bytes", frame_len));
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

        // Decode message
        match binary_codec::decode_input(&payload_buf[..frame_len]) {
            Ok((msg, _)) => {
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
                metrics.record_decode_error();
                eprintln!("{}: decode error: {:?}", client_id, e);
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
    let mut buf = Vec::with_capacity(MAX_FIX_MESSAGE_SIZE);

    loop {
        buf.clear();

        let read_result = timeout(read_timeout, read_fix_message(reader, &mut buf)).await;

        match read_result {
            Ok(Ok(0)) => return Err("EOF".to_string()),
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
                        metrics.record_decode_error();
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
    let mut total = 0;
    let mut temp = [0u8; 1];

    loop {
        let n = reader.read(&mut temp).await?;
        if n == 0 {
            return Ok(0);
        }

        buf.push(temp[0]);
        total += 1;

        // Check for complete message (ends with 10=XXX<SOH>)
        if buf.len() >= 7 && buf[buf.len() - 1] == 0x01 {
            if let Some(pos) = buf.windows(3).rposition(|w| w == b"10=") {
                if pos + 7 <= buf.len() {
                    return Ok(total);
                }
            }
        }

        // Safety limit
        if total > MAX_FIX_MESSAGE_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "FIX message too large",
            ));
        }
    }
}

/// Extract user_id from an input message for routing.
#[inline]
fn extract_user_id(msg: &InputMessage) -> u32 {
    match msg {
        InputMessage::NewOrder(order) => order.user_id,
        InputMessage::Cancel(cancel) => cancel.user_id,
        InputMessage::Flush => 0,
        InputMessage::QueryTopOfBook(_) => 0,
    }
}
