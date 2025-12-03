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

use engine_core::{InputMessage, Symbol, TopOfBookQuery};
use engine_protocol::{decode_output, encode_input};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let symbol_str = env::args().nth(1).unwrap_or_else(|| "IBM".to_string());
    let symbol = Symbol::from_str(&symbol_str);
    let addr = "127.0.0.1:9000";

    println!("Connecting to {}...", addr);
    let mut stream = TcpStream::connect(addr).await?;
    println!("Connected. Querying top-of-book for {}", symbol);

    let query = InputMessage::QueryTopOfBook(TopOfBookQuery::new(symbol));

    // Encode and send
    let mut buf = Vec::new();
    encode_input(&query, &mut buf)?;

    // Length-prefix for TCP framing
    let len = (buf.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(&buf).await?;
    stream.flush().await?;

    // Read responses (expect 2: bid and ask)
    for _ in 0..2 {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        let mut frame = vec![0u8; len];
        stream.read_exact(&mut frame).await?;

        let msg = decode_output(&frame)?;
        println!("Response: {:?}", msg);
    }

    Ok(())
}
