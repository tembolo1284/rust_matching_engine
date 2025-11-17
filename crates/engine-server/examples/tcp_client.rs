use std::env;
use std::error::Error;
use std::io::{self, Write};
use std::time::Duration;

use engine_core::InputMessage;
use engine_protocol::csv_codec::{format_output_csv, parse_input_line};
use engine_protocol::{decode_output, encode_input};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Where to connect: env override or default.
    let addr = env::var("ENGINE_CLIENT_ADDR").unwrap_or_else(|_| "127.0.0.1:9001".to_string());

    println!("Connecting to {}...", addr);
    let mut stream = TcpStream::connect(&addr).await?;
    println!("Connected.");
    println!("Type CSV commands like:");
    println!("  N, 1, AAPL, 100, 10, B, 1");
    println!("  C, 1, 1");
    println!("  F");
    println!("  Q, AAPL   (query top-of-book)");
    println!("Type 'quit' or 'exit' to leave.\n");

    let stdin = io::stdin();

    loop {
        // Prompt
        print!(">> ");
        io::stdout().flush()?;

        let mut line = String::new();
        let n = stdin.read_line(&mut line)?;
        if n == 0 {
            // EOF
            println!("\nEOF on stdin, exiting client.");
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("quit") || trimmed.eq_ignore_ascii_case("exit") {
            println!("Exiting client.");
            break;
        }

        // Parse CSV into InputMessage
        let input_msg: InputMessage = match parse_input_line(trimmed) {
            Some(m) => m,
            None => {
                eprintln!("Could not parse line as input message. Check CSV format.");
                continue;
            }
        };

        // Encode to binary frame
        let mut payload = Vec::with_capacity(128);
        encode_input(&input_msg, &mut payload).map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("encode error: {:?}", e))
        })?;

        let len = payload.len() as u32;
        let len_bytes = len.to_be_bytes();

        // Send length + payload
        stream.write_all(&len_bytes).await?;
        stream.write_all(&payload).await?;

        // Now read back all responses that arrive shortly after.
        // We'll keep reading frames until a small timeout occurs
        // with no more data.
        loop {
            let mut len_buf = [0u8; 4];

            let read_len_res = timeout(Duration::from_millis(100), stream.read_exact(&mut len_buf)).await;

            let () = match read_len_res {
                Ok(Ok(())) => (),
                Ok(Err(e)) => {
                    eprintln!("Read error (len): {:?}", e);
                    return Ok(());
                }
                Err(_) => {
                    // Timed out waiting for next response → assume we're done for this command.
                    break;
                }
            };

            let frame_len = u32::from_be_bytes(len_buf) as usize;
            if frame_len == 0 {
                continue;
            }

            let mut buf = vec![0u8; frame_len];
            let read_frame_res =
                timeout(Duration::from_millis(100), stream.read_exact(&mut buf)).await;

            let () = match read_frame_res {
                Ok(Ok(())) => (),
                Ok(Err(e)) => {
                    eprintln!("Read error (frame): {:?}", e);
                    return Ok(());
                }
                Err(_) => {
                    eprintln!("Timed out reading frame body.");
                    break;
                }
            };

            // Decode OutputMessage
            let msg = decode_output(&buf).map_err(|e| {
                io::Error::new(io::ErrorKind::Other, format!("decode error: {:?}", e))
            })?;

            // Print as CSV so it matches your old style.
            let line = format_output_csv(&msg);
            println!("<< {}", line);
        }
    }

    Ok(())
}

