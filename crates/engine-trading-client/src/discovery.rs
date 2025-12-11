//! Server capability discovery.
//!
//! Automatically detects:
//! - Transport: TCP or UDP
//! - Protocol: CSV, Binary, or FIX

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
    /// Server address.
    pub addr: SocketAddr,
    /// Detected transport.
    pub transport: Transport,
    /// Detected protocol.
    pub protocol: Protocol,
    /// Server responded to probe.
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
///
/// Tries TCP first, then UDP. Sends a probe message and analyzes response.
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

/// Try to discover via TCP.
async fn discover_tcp(addr: SocketAddr) -> Result<ServerCapabilities> {
    // Connect with timeout
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

/// Probe TCP connection to determine protocol.
async fn probe_protocol_tcp(mut stream: TcpStream) -> Result<Protocol> {
    // Send a CSV probe (works with all protocols as it's human readable)
    // Using QueryTopOfBook which is harmless
    let probe = b"Q, PROBE\n";
    stream.write_all(probe).await?;
    stream.flush().await?;

    // Read response with timeout
    let mut buf = [0u8; 256];
    let n = timeout(Duration::from_secs(2), stream.read(&mut buf)).await??;

    if n == 0 {
        // Server didn't respond, but connected - assume CSV
        return Ok(Protocol::Csv);
    }

    let response = &buf[..n];
    Ok(detect_protocol_from_response(response))
}

/// Try to discover via UDP.
async fn discover_udp(addr: SocketAddr) -> Result<ServerCapabilities> {
    // Bind to any local port
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(addr).await?;

    // Send probe
    let probe = b"Q, PROBE\n";
    socket.send(probe).await?;

    // Wait for response
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

/// Detect protocol from server response.
fn detect_protocol_from_response(data: &[u8]) -> Protocol {
    if data.is_empty() {
        return Protocol::Csv;
    }

    // Check for binary: magic byte 'M' followed by valid message type
    if data.len() >= 2 && data[0] == MAGIC_BYTE {
        let msg_type = data[1];
        // Valid output types: 'A' (Ack), 'X' (CancelAck), 'T' (Trade), 'B' (TopOfBook)
        if matches!(msg_type, b'A' | b'X' | b'T' | b'B') {
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
                if matches!(msg_type, b'A' | b'X' | b'T' | b'B') {
                    return Protocol::Binary;
                }
            }
        }
    }

    // Default to CSV
    Protocol::Csv
}

/// Discover and print results.
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
        let mut response = MAGIC_BYTES.to_vec();
        response.extend_from_slice(&[0x01, 0x0A, 0x00, 0x10]);
        assert_eq!(detect_protocol_from_response(&response), Protocol::Binary);
    }

    #[test]
    fn test_detect_fix() {
        let response = b"8=FIX.4.4\x019=100\x01";
        assert_eq!(detect_protocol_from_response(response), Protocol::Fix);
    }

    #[test]
    fn test_detect_length_prefixed_binary() {
        let mut response = vec![0x00, 0x00, 0x00, 0x20]; // length = 32
        response.extend_from_slice(&MAGIC_BYTES);
        response.extend_from_slice(&[0x01, 0x0A, 0x00, 0x10]);
        assert_eq!(detect_protocol_from_response(&response), Protocol::Binary);
    }
}
