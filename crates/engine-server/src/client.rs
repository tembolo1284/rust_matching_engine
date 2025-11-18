// crates/engine-server/src/client.rs
// Update to handle BOTH CSV and binary protocols

use std::error::Error;

use engine_core::OutputMessage;
use engine_protocol::binary_codec;  // Import the module
use engine_protocol::csv_codec;     // Also import CSV codec
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{tcp::OwnedWriteHalf, TcpStream};

use crate::types::{ClientId, ClientRegistry, EngineRequest, EngineTx, OutboundRx};

/// Run the client I/O loop for a single connection.
pub async fn run_client(
    client_id: ClientId,
    stream: TcpStream,
    engine_tx: EngineTx,
    mut out_rx: OutboundRx,
    clients: ClientRegistry,
) -> Result<(), Box<dyn Error>> {
    let _peer_addr = stream.peer_addr().ok();

    // Split stream
    let (mut read_stream, write_stream) = stream.into_split();

    // Writer task: consume OutputMessages and write responses
    let _writer_handle = tokio::spawn(async move {
        let mut write_stream = write_stream;

        while let Some(msg) = out_rx.recv().await {
            // For now, use CSV format for compatibility with netcat
            // Later we can detect protocol type per client
            if let Err(e) = write_csv_message(&mut write_stream, &msg).await {
                eprintln!("Client {} write error: {:?}", client_id.0, e);
                break;
            }
        }
    });

    // Try to detect protocol by peeking at first byte
    let mut first_byte = [0u8; 1];
    let protocol = if let Ok(_) = read_stream.peek(&mut first_byte).await {
        if first_byte[0] == b'N' || first_byte[0] == b'C' || first_byte[0] == b'F' || first_byte[0] == b'Q' {
            // Looks like CSV (N=NewOrder, C=Cancel, F=Flush, Q=Query)
            Protocol::Csv
        } else {
            // Assume binary
            Protocol::Binary
        }
    } else {
        Protocol::Csv // Default to CSV for netcat compatibility
    };

    eprintln!("Client {} using {:?} protocol", client_id.0, protocol);

    // Reader loop based on protocol
    match protocol {
        Protocol::Csv => {
            run_csv_reader(client_id, read_stream, engine_tx, clients).await
        }
        Protocol::Binary => {
            run_binary_reader(client_id, read_stream, engine_tx, clients).await
        }
    }
}

#[derive(Debug)]
enum Protocol {
    Csv,
    Binary,
}

async fn run_csv_reader(
    client_id: ClientId,
    mut read_stream: tokio::net::tcp::OwnedReadHalf,
    engine_tx: EngineTx,
    clients: ClientRegistry,
) -> Result<(), Box<dyn Error>> {
    let mut buffer = Vec::new();
    let mut temp_buf = [0u8; 1024];

    loop {
        // Read available data
        match read_stream.read(&mut temp_buf).await {
            Ok(0) => {
                // EOF - client disconnected
                eprintln!("Client {} disconnected", client_id.0);
                break;
            }
            Ok(n) => {
                buffer.extend_from_slice(&temp_buf[..n]);
                
                // Process complete lines
                while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line = buffer.drain(..=newline_pos).collect::<Vec<u8>>();
                    let line_str = String::from_utf8_lossy(&line);
                    let line_str = line_str.trim();
                    
                    if line_str.is_empty() {
                        continue;
                    }
                    
                    eprintln!("Client {} CSV: {}", client_id.0, line_str);
                    
                    // Parse CSV line
                    if let Some(input_msg) = csv_codec::parse_input_line(line_str) {
                        let req = EngineRequest {
                            client_id,
                            msg: input_msg,
                        };
                        
                        if engine_tx.send(req).is_err() {
                            eprintln!("Engine channel closed");
                            break;
                        }
                    } else {
                        eprintln!("Client {} invalid CSV: {}", client_id.0, line_str);
                    }
                }
            }
            Err(e) => {
                eprintln!("Client {} read error: {:?}", client_id.0, e);
                break;
            }
        }
    }

    // Remove client from registry
    {
        let mut guard = clients.write().await;
        guard.remove(&client_id);
    }

    Ok(())
}

async fn run_binary_reader(
    client_id: ClientId,
    mut read_stream: tokio::net::tcp::OwnedReadHalf,
    engine_tx: EngineTx,
    clients: ClientRegistry,
) -> Result<(), Box<dyn Error>> {
    loop {
        // Read length prefix (u32 BE)
        let mut len_buf = [0u8; 4];
        if let Err(e) = read_stream.read_exact(&mut len_buf).await {
            eprintln!("Client {} disconnected: {:?}", client_id.0, e);
            break;
        }

        let frame_len = u32::from_be_bytes(len_buf) as usize;
        if frame_len == 0 {
            continue;
        }

        let mut frame = vec![0u8; frame_len];
        if let Err(e) = read_stream.read_exact(&mut frame).await {
            eprintln!("Client {} read error: {:?}", client_id.0, e);
            break;
        }

        match binary_codec::decode_input(&frame) {
            Ok(input_msg) => {
                eprintln!("Client {} binary msg: {:?}", client_id.0, input_msg);
                
                let req = EngineRequest {
                    client_id,
                    msg: input_msg,
                };
                
                if engine_tx.send(req).is_err() {
                    eprintln!("Engine channel closed");
                    break;
                }
            }
            Err(err) => {
                eprintln!("Client {} decode error: {:?}", client_id.0, err);
                break;
            }
        }
    }

    // Remove client from registry
    {
        let mut guard = clients.write().await;
        guard.remove(&client_id);
    }

    Ok(())
}

async fn write_csv_message(
    stream: &mut OwnedWriteHalf,
    msg: &OutputMessage,
) -> Result<(), Box<dyn Error>> {
    // Use the legacy CSV format for netcat compatibility
    let csv_line = csv_codec::format_output_legacy(msg);
    let data = format!("{}\n", csv_line);
    
    stream.write_all(data.as_bytes()).await?;
    stream.flush().await?;
    
    eprintln!("Sent CSV: {}", csv_line.trim());
    
    Ok(())
}

async fn write_binary_message(
    stream: &mut OwnedWriteHalf,
    msg: &OutputMessage,
) -> Result<(), Box<dyn Error>> {
    let mut payload = Vec::with_capacity(128);
    
    binary_codec::encode_output(msg, &mut payload)
        .map_err(|e| format!("encode error: {:?}", e))?;

    let len = payload.len() as u32;
    let len_bytes = len.to_be_bytes();

    stream.write_all(&len_bytes).await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;

    Ok(())
}
