//! Simple interactive TCP client for the matching engine.
//!
//! Usage:
//!   # terminal 1 (server):
//!   $ cargo run -p engine-server
//!
//!   # terminal 2 (client):
//!   $ ENGINE_CLIENT_ADDR=127.0.0.1:9001 cargo run -p engine-server --example tcp_client
//!
//! Then type lines like:
//!   N, 1, AAPL, 100, 10, B, 1
//!   N, 2, AAPL,  99, 10, S, 2
//!   Q, AAPL
//!   F
//!
//! Each line is parsed with the CSV codec, encoded to the binary protocol,
//! sent to the server, and any outputs from the engine are printed as CSV.

use std::env;
use std::error::Error;

use engine_core::OutputMessage;
use engine_protocol::{
    csv_codec::{format_output_csv, parse_input_line},
    decode_output, encode_input,
};
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Where to connect? Default to 127.0.0.1:9000 unless overridden.
    let addr = env::var("ENGINE_CLIENT_ADDR").unwrap_or_else(|_| "127.0.0.1:9000".to_string());

    println!("Connecting to {}...", addr);
    let stream = TcpStream::connect(&addr).await?;
    println!("Connected.");

    // Split into read/write halves.
    let (mut read_half, mut write_half) = stream.into_split();

    // ---------------- Reader task ----------------
    //
    // Continuously read frames: [len: u32 BE][payload bytes],
    // decode OutputMessage and print as CSV.
    let reader_task = tokio::spawn(async move {
        loop {
            // Read length prefix
            let mut len_buf = [0u8; 4];
            if let Err(e) = read_half.read_exact(&mut len_buf).await {
                eprintln!("[client] read length error / EOF: {:?}", e);
                break;
            }
            let frame_len = u32::from_be_bytes(len_buf) as usize;
            if frame_len == 0 {
                eprintln!("[client] got zero-length frame, ignoring");
                continue;
            }

            // Read payload
            let mut payload = vec![0u8; frame_len];
            if let Err(e) = read_half.read_exact(&mut payload).await {
                eprintln!("[client] read payload error / EOF: {:?}", e);
                break;
            }

            // Decode OutputMessage
            match decode_output(&payload) {
                Ok(msg) => {
                    print_engine_output(&msg);
                }
                Err(e) => {
                    eprintln!("[client] decode_output error: {:?}", e);
                    // Could break, but we just keep going for now.
                }
            }
        }
        eprintln!("[client] reader task exiting");
    });

    // ---------------- Writer / stdin loop ----------------
    //
    // Read CSV lines from stdin, parse into InputMessage, encode to binary,
    // and send to the server.
    let stdin = io::stdin();
    let mut stdin_reader = BufReader::new(stdin);
    let mut line = String::new();

    println!("Type CSV commands (e.g. `N, 1, AAPL, 100, 10, B, 1`).");
    println!("Empty line or EOF to exit.\n");

    loop {
        line.clear();
        print!(">> ");

        // Flush stdout so prompt appears immediately (async flush!)
        if let Err(e) = io::stdout().flush().await {
            eprintln!("[client] stdout flush error: {:?}", e);
        }

        let n = stdin_reader.read_line(&mut line).await?;
        if n == 0 {
            // EOF
            println!("\nEOF, shutting down client...");
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            println!("(empty line – exiting)");
            break;
        }

        if trimmed.eq_ignore_ascii_case("quit") || trimmed.eq_ignore_ascii_case("exit") {
            println!("Exiting on user request.");
            break;
        }

        // Parse CSV → InputMessage
        let maybe_msg = parse_input_line(trimmed);
        let input_msg = match maybe_msg {
            None => {
                eprintln!("[client] could not parse line as input message: {:?}", trimmed);
                continue;
            }
            Some(m) => m,
        };

        // Encode to binary payload (InputMessage → bytes).
        let mut payload = Vec::with_capacity(128);
        if let Err(e) = encode_input(&input_msg, &mut payload) {
            eprintln!("[client] encode_input error: {:?}", e);
            continue;
        }

        // Send length prefix + payload.
        let len = payload.len() as u32;
        let len_bytes = len.to_be_bytes();
        if let Err(e) = write_half.write_all(&len_bytes).await {
            eprintln!("[client] write length error: {:?}", e);
            break;
        }
        if let Err(e) = write_half.write_all(&payload).await {
            eprintln!("[client] write payload error: {:?}", e);
            break;
        }
        if let Err(e) = write_half.flush().await {
            eprintln!("[client] flush error: {:?}", e);
            break;
        }
    }

    // Dropping write_half signals EOF to server.
    drop(write_half);

    // Wait for reader to finish.
    let _ = reader_task.await;

    Ok(())
}

/// Pretty-print an OutputMessage as CSV, matching your original C++ formatting.
fn print_engine_output(msg: &OutputMessage) {
    let csv = format_output_csv(msg);
    println!("<< {}", csv);
}

