//! Server capability discovery.

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::timeout;

use engine_protocol::wire_types::MAGIC_BYTE;

use crate::types::{Protocol, Transport};

/// Discovered server capabilities.
#[derive(Debug, Clone)]
pub struct ServerCapabilities {
    pub addr: SocketAddr,
    pub transport: Transport,
    pub protocol: Protocol,
    pub responsive: bool,
}

impl std::fmt::Display for ServerCapabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} via {} using {} protocol",
            self.addr, self.transport, self.protocol
        )
    }
}

/// Discover server capabilities by probing.
pub async fn discover_server(addr: &str) -> Result<ServerCapabilities> {
    let socket_addr: SocketAddr = addr.parse()?;

    // Try TCP first
    if let Ok(caps) = discover_tcp(socket_addr).await {
        return Ok(caps);
    }

    // Try UDP
    if let Ok(caps) = discover_udp(socket_addr).await {
        return Ok(caps);
    }

    Err(anyhow!("Could not connect to server at {}", addr))
}

async fn discover_tcp(addr: SocketAddr) -> Result<ServerCapabilities> {
    let stream = timeout(Duration::from_secs(5), TcpStream::connect(addr)).await??;
    stream.set_nodelay(true)?;
    
    let protocol = probe_protocol_tcp(stream).await?;

    Ok(ServerCapabilities {
        addr,
        transport: Transport::Tcp,
        protocol,
        responsive: true,
    })
}

async fn probe_protocol_tcp(mut stream: TcpStream) -> Result<Protocol> {
    let probe = b"Q, PROBE\n";
    stream.write_all(probe).await?;
    stream.flush().await?;

    let mut buf = [0u8; 256];
    let n = timeout(Duration::from_secs(2), stream.read(&mut buf)).await??;

    if n == 0 {
        return Ok(Protocol::Csv);
    }

    let response = &buf[..n];
    Ok(detect_protocol_from_response(response))
}

async fn discover_udp(addr: SocketAddr) -> Result<ServerCapabilities> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(addr).await?;

    let probe = b"Q, PROBE\n";
    socket.send(probe).await?;

    let mut buf = [0u8; 256];
    let n = timeout(Duration::from_secs(2), socket.recv(&mut buf)).await??;

    if n == 0 {
        return Err(anyhow!("No UDP response"));
    }

    let response = &buf[..n];
    let protocol = detect_protocol_from_response(response);

    Ok(ServerCapabilities {
        addr,
        transport: Transport::Udp,
        protocol,
        responsive: true,
    })
}

fn detect_protocol_from_response(data: &[u8]) -> Protocol {
    if data.is_empty() {
        return Protocol::Csv;
    }

    // Check for binary: magic byte 'M' followed by valid message type
    if data.len() >= 2 && data[0] == MAGIC_BYTE {
        let msg_type = data[1];
        // Valid output types: 'A' (Ack), 'X' (CancelAck), 'T' (Trade), 'B' (TopOfBook), 'R' (Reject)
        if matches!(msg_type, b'A' | b'X' | b'T' | b'B' | b'R') {
            return Protocol::Binary;
        }
    }

    // Check for FIX (starts with "8=FIX")
    if data.len() >= 2 && &data[0..2] == b"8=" {
        return Protocol::Fix;
    }

    // Check for length-prefixed binary (4-byte length then magic byte)
    if data.len() >= 6 {
        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if len > 0 && len < 1000 && data.len() >= 6 {
            if data[4] == MAGIC_BYTE {
                let msg_type = data[5];
                if matches!(msg_type, b'A' | b'X' | b'T' | b'B' | b'R') {
                    return Protocol::Binary;
                }
            }
        }
    }

    Protocol::Csv
}

pub async fn discover_and_print(addr: &str) -> Result<ServerCapabilities> {
    println!("Discovering server at {}...", addr);

    let caps = discover_server(addr).await?;

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║                  SERVER DISCOVERED                        ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Address:   {:44} ║", caps.addr);
    println!("║  Transport: {:44} ║", caps.transport);
    println!("║  Protocol:  {:44} ║", caps.protocol);
    println!("║  Status:    {:44} ║", if caps.responsive { "Responsive ✓" } else { "No response" });
    println!("╚══════════════════════════════════════════════════════════╝");

    Ok(caps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_csv() {
        let response = b"B, IBM, B, 100, 50\n";
        assert_eq!(detect_protocol_from_response(response), Protocol::Csv);
    }

    #[test]
    fn test_detect_binary() {
        // FIXED: Use MAGIC_BYTE, not MAGIC_BYTES
        let response = vec![MAGIC_BYTE, b'A', 0x01, 0x0A, 0x00, 0x10];
        assert_eq!(detect_protocol_from_response(&response), Protocol::Binary);
    }

    #[test]
    fn test_detect_fix() {
        let response = b"8=FIX.4.4\x019=100\x01";
        assert_eq!(detect_protocol_from_response(response), Protocol::Fix);
    }

    #[test]
    fn test_detect_length_prefixed_binary() {
        // FIXED: Use MAGIC_BYTE
        let mut response = vec![0x00, 0x00, 0x00, 0x20]; // length = 32
        response.push(MAGIC_BYTE);
        response.extend_from_slice(&[b'T', 0x00, 0x10]);
        assert_eq!(detect_protocol_from_response(&response), Protocol::Binary);
    }
}
