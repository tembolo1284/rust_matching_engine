//! Example: query top-of-book for a symbol.
//!
//! Usage:
//!
//! ```bash
//! # Run server
//! cargo run -p engine-server
//!
//! # In another terminal, run this example
//! cargo run --example query_top_of_book -- IBM
//! ```
//!
//! It will:
//! - connect to 127.0.0.1:9000
//! - send a `QueryTopOfBook` request for the given symbol
//! - print the resulting `TopOfBook` messages.

use std::env;
use std::error::Error;

use engine_core::{InputMessage, TopOfBookQuery};
use engine_protocol::{decode_output, encode_input};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let symbol = env::args().nth(1).unwrap_or_else(|| "IBM".to_string());

    let addr = "127.0.0.1:9000";
    println!("Connecting to {}", addr);

    let mut stream = TcpStream::connect(addr).await?;
    println!("Connected.");

    let query = InputMessage::QueryTopOfBook(TopOfBookQuery { symbol: symbol.clone() });

    // Encode and send
    let mut payload = Vec::with_capacity(64);
    encode_input(&query, &mut payload)?;

    let len = payload.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;

    println!("--> Sent QueryTopOfBook for symbol '{}'", symbol);

    // Read a couple of responses (bid + ask TOB).
    for i in 0..2 {
        let mut len_buf = [0u8; 4];
        if let Err(e) = stream.read_exact(&mut len_buf).await {
            eprintln!("Read error / EOF: {:?}", e);
            break;
        }
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        let mut frame = vec![0u8; frame_len];
        stream.read_exact(&mut frame).await?;

        match decode_output(&frame) {
            Ok(msg) => println!("<-- [{}] {:?}", i, msg),
            Err(err) => {
                eprintln!("Decode error: {:?}", err);
                break;
            }
        }
    }

    Ok(())
}

