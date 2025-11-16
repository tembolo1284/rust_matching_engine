//! Simple example TCP client for the matching engine server.
//!
//! Usage (from workspace root):
//!
//! ```bash
//! # In one terminal, run the server
//! cargo run -p engine-server
//!
//! # In another terminal, run the example client
//! cargo run --example tcp_client
//! ```
//!
//! The client will:
//! - connect to 127.0.0.1:9000
//! - send one BUY and one SELL in "IBM"
//! - read a few responses and print them.

use std::error::Error;
use std::time::Duration;

use engine_core::{InputMessage, NewOrder, Side};
use engine_protocol::{decode_output, encode_input};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let addr = "127.0.0.1:9000";
    println!("Connecting to {}", addr);

    let mut stream = TcpStream::connect(addr).await?;
    println!("Connected.");

    // Helper to send one InputMessage.
    async fn send_msg(stream: &mut TcpStream, msg: &InputMessage) -> Result<(), Box<dyn Error>> {
        let mut payload = Vec::with_capacity(128);
        encode_input(msg, &mut payload)?;

        let len = payload.len() as u32;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&payload).await?;
        stream.flush().await?;
        Ok(())
    }

    // 1) Send a BUY order
    let buy = InputMessage::NewOrder(NewOrder {
        user_id: 1,
        symbol: "IBM".to_string(),
        price: 10,
        quantity: 100,
        side: Side::Buy,
        user_order_id: 1,
    });

    println!("--> Sending BUY order: {:?}", buy);
    send_msg(&mut stream, &buy).await?;

    // 2) Send a SELL order that matches
    let sell = InputMessage::NewOrder(NewOrder {
        user_id: 2,
        symbol: "IBM".to_string(),
        price: 9, // crosses 10 bid
        quantity: 100,
        side: Side::Sell,
        user_order_id: 42,
    });

    println!("--> Sending SELL order: {:?}", sell);
    send_msg(&mut stream, &sell).await?;

    // 3) Read a few responses (acks + trade + TOB)
    for i in 0..5 {
        // length prefix
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

    // Let things settle a bit (not strictly necessary).
    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("Client done.");
    Ok(())
}

