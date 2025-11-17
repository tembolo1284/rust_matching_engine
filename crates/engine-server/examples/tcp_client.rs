// crates/engine-server/examples/tcp_client.rs
//! Simple interactive TCP client for the matching engine.
//!
//! - Reads CSV lines from stdin (same format as the old UDP/CSV interface).
//! - Uses `engine-protocol` to encode them to binary frames.
//! - Sends them to the server over TCP.
//! - Reads binary frames back from the server, decodes them, and
//!   prints CSV-style output.
//!
//! This is your “new netcat” for the Rust server.

use std::io::{self, BufRead};

use anyhow::Result;
use engine_core::OutputMessage;
use engine_protocol::{
    csv_codec::{format_output_csv, parse_input_line},
    decode_output,
    encode_input,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<()> {
    // ---------------------------------------------------------------------
    // Connect to the server
    // ---------------------------------------------------------------------
    // For now, hard-code the address; you can make this a CLI arg later.
    let addr = std::env::var("ENGINE_CLIENT_ADDR").unwrap_or_else(|_| "127.0.0.1:9000".to_string());
    eprintln!("Connecting to {}", addr);
    let mut stream = TcpStream::connect(&addr).await?;
    eprintln!("Connected.");

    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();

    loop {
        eprint!(">> ");
        io::Write::flush(&mut io::stderr())?;

        let mut line = String::new();
        let n = stdin_lock.read_line(&mut line)?;
        if n == 0 {
            // EOF
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("quit") || line.eq_ignore_ascii_case("exit") {
            break;
        }

        // -----------------------------------------------------------------
        // Parse CSV → InputMessage
        // -----------------------------------------------------------------
        let Some(input_msg) = parse_input_line(line) else {
            eprintln!("Could not parse input line as a valid message.");
            continue;
        };

        // -----------------------------------------------------------------
        // Encode to binary, frame with length prefix, send
        // -----------------------------------------------------------------
        let mut payload = Vec::with_capacity(128);
        encode_input(&input_msg, &mut payload)?; // `?` works because we return `anyhow::Result`

        let len = (payload.len() as u32).to_be_bytes();
        stream.write_all(&len).await?;
        stream.write_all(&payload).await?;
        stream.flush().await?;

        // -----------------------------------------------------------------
        // Read responses.
        //
        // For simplicity, we read *one* frame and print it; the server may
        // send multiple events (ACK + TOB, or trades), so we loop until
        // there is nothing immediately available.
        // -----------------------------------------------------------------
        // First frame:
        if let Some(msg) = read_one_output(&mut stream).await? {
            print_output(&msg);
        } else {
            eprintln!("Server closed connection.");
            break;
        }

        // Then, greedily read any immediately-available additional frames.
        // (Non-blocking-ish: we use a short read_exact with peek.)
        loop {
            // Peek at the stream: if there is no more data, break.
            let mut peek_buf = [0u8; 1];
            match stream.peek(&mut peek_buf).await {
                Ok(0) => {
                    // Connection closed.
                    eprintln!("Server closed connection.");
                    return Ok(());
                }
                Ok(_) => {
                    // There is data; read another frame.
                    if let Some(msg) = read_one_output(&mut stream).await? {
                        print_output(&msg);
                    } else {
                        eprintln!("Server closed connection.");
                        return Ok(());
                    }
                }
                Err(_) => {
                    // Treat peek error as "no more immediate data".
                    break;
                }
            }
        }
    }

    Ok(())
}

// Read a single length-prefixed frame and decode it into an OutputMessage.
// Returns Ok(None) if the server closed the connection cleanly.
async fn read_one_output(stream: &mut TcpStream) -> Result<Option<OutputMessage>> {
    let mut len_buf = [0u8; 4];
    if let Err(e) = stream.read_exact(&mut len_buf).await {
        // If it's EOF, return None; otherwise treat as error.
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            return Ok(None);
        } else {
            return Err(e.into());
        }
    }

    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len == 0 {
        // Empty frame; skip.
        return Ok(None);
    }

    let mut buf = vec![0u8; frame_len];
    stream.read_exact(&mut buf).await?;
    let msg = decode_output(&buf)?;
    Ok(Some(msg))
}

fn print_output(msg: &OutputMessage) {
    let line = format_output_csv(msg);
    println!("<< {}", line);
}

