# Quick Start Guide

This guide shows how to run the matching engine server with different transport and protocol combinations.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Building](#building)
- [Server Startup](#server-startup)
- [TCP with CSV](#tcp-with-csv)
- [TCP with Binary](#tcp-with-binary)
- [TCP with FIX](#tcp-with-fix)
- [UDP with CSV](#udp-with-csv)
- [UDP with Binary](#udp-with-binary)
- [Multicast Market Data](#multicast-market-data)
- [Trading Client](#trading-client)

---

## Prerequisites

- Rust 1.70+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- netcat (`nc`) for testing
- Optional: `socat` for UDP testing

## Building
```bash
# Clone and build
git clone <repo>
cd rust_matching_engine

# Debug build
cargo build --workspace

# Release build (recommended for performance)
cargo build --release --workspace
```

---

## Server Startup

### Default (all transports enabled)
```bash
cargo run --release -p engine-server
```

Output:
```
==============================================================
         Matching Engine Server v0.2.0
==============================================================

Transports:
  TCP:       0.0.0.0:9000 (CSV, Binary, FIX)
  UDP:       0.0.0.0:9001 (CSV, Binary)
  Multicast: 239.255.0.1:9002 (Binary)

Limits:
  Max TCP clients:    1024
  Engine queue:       100000
  Client queue:       10000

==============================================================
Ready. Press Ctrl+C to shutdown.
==============================================================
```

### Custom ports
```bash
cargo run -p engine-server -- --tcp-port 7000 --udp-port 7001
```

### Disable specific transports
```bash
# TCP only
cargo run -p engine-server -- --no-udp --no-mcast

# UDP only  
cargo run -p engine-server -- --no-tcp --no-mcast
```

### Environment variables
```bash
ENGINE_TCP_PORT=8000 \
ENGINE_UDP_PORT=8001 \
ENGINE_MAX_TCP_CLIENTS=100 \
cargo run -p engine-server
```

---

## TCP with CSV

The simplest way to interact with the engine. Great for testing with `netcat`.

### Terminal 1: Start server
```bash
cargo run --release -p engine-server
```

### Terminal 2: Connect with netcat
```bash
nc localhost 9000
```

### Send orders interactively
```
N, 1, IBM, 100, 50, B, 1
A, 1, 1, IBM
B, IBM, B, 100, 50

N, 2, IBM, 100, 50, S, 1
A, 2, 1, IBM
T, IBM, 1, 1, 2, 1, 100, 50
B, IBM, B, -, -
```

### Send from file
```bash
# Create test file
cat > orders.csv << 'EOF'
N, 1, AAPL, 150, 100, B, 1
N, 1, AAPL, 151, 50, B, 2
N, 2, AAPL, 150, 75, S, 1
F
EOF

# Send to server
nc localhost 9000 < orders.csv
```

### One-liner order
```bash
echo "N, 1, IBM, 100, 50, B, 1" | nc localhost 9000
```

---

## TCP with Binary

Higher performance than CSV. Protocol is auto-detected.

### Using the example client
```bash
# Terminal 1: Server
cargo run --release -p engine-server

# Terminal 2: Binary client
cargo run -p engine-server --example tcp_client -- --binary
```

### Writing a custom client
```rust
use std::io::{Read, Write};
use std::net::TcpStream;

fn main() {
    let mut stream = TcpStream::connect("127.0.0.1:9000").unwrap();
    
    // Binary NewOrder frame
    let frame: Vec<u8> = vec![
        // Magic "MENG"
        0x4D, 0x45, 0x4E, 0x47,
        // Version 1, Type 0 (NewOrder), Length 25
        0x01, 0x00, 0x00, 0x19,
        // user_id = 1
        0x00, 0x00, 0x00, 0x01,
        // user_order_id = 1
        0x00, 0x00, 0x00, 0x01,
        // price = 10000
        0x00, 0x00, 0x27, 0x10,
        // quantity = 100
        0x00, 0x00, 0x00, 0x64,
        // side = Buy
        0x00,
        // symbol = "IBM"
        0x49, 0x42, 0x4D, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    
    // Length prefix + frame
    let len = (frame.len() as u32).to_be_bytes();
    stream.write_all(&len).unwrap();
    stream.write_all(&frame).unwrap();
    
    // Read response
    let mut response = vec![0u8; 1024];
    let n = stream.read(&mut response).unwrap();
    println!("Response: {:?}", &response[..n]);
}
```

---

## TCP with FIX

FIX 4.2/4.4 for institutional connectivity. Protocol is auto-detected when message starts with `8=FIX`.

### Enable FIX gateway
```bash
cargo run -p engine-server -- --fix
```

### Send FIX message
```bash
# NewOrderSingle (35=D)
printf '8=FIX.4.4\x019=100\x0135=D\x0149=CLIENT\x0156=ENGINE\x0134=1\x0152=20240101-12:00:00.000\x0111=1\x0155=IBM\x0154=1\x0160=20240101-12:00:00.000\x0138=100\x0140=2\x0144=100.00\x0110=000\x01' | nc localhost 9000
```

### FIX message format
```
8=FIX.4.4|9=<len>|35=D|49=CLIENT|56=ENGINE|34=<seq>|52=<time>|
11=<ClOrdID>|55=<Symbol>|54=<Side>|60=<TransactTime>|38=<Qty>|40=<OrdType>|44=<Price>|
10=<checksum>|
```

Note: `|` represents SOH (0x01) delimiter.

---

## UDP with CSV

Low-latency with human-readable format.

### Terminal 1: Start server
```bash
cargo run --release -p engine-server
# UDP listening on port 9001
```

### Terminal 2: Send UDP messages
```bash
# Single order
echo "N, 1, IBM, 100, 50, B, 1" | nc -u localhost 9001

# Keep connection open for responses
nc -u localhost 9001
N, 1, IBM, 100, 50, B, 1
```

### Using socat for bidirectional UDP
```bash
socat - UDP:localhost:9001
N, 1, IBM, 100, 50, B, 1
```

---

## UDP with Binary

Lowest latency option.

### Terminal 1: Start server
```bash
cargo run --release -p engine-server
```

### Terminal 2: Send binary UDP
```bash
# Using xxd to send raw bytes
echo "4D454E470100001900000001000000010000271000000064004942 4D0000000000" | xxd -r -p | nc -u localhost 9001
```

### Python example
```python
import socket
import struct

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)

# Build NewOrder
frame = b'MENG'  # Magic
frame += struct.pack('>BBH', 1, 0, 25)  # version, type, len
frame += struct.pack('>IIIIB', 1, 1, 10000, 100, 0)  # user, order, price, qty, side
frame += b'IBM\x00\x00\x00\x00\x00'  # symbol

# Length prefix + frame
packet = struct.pack('>I', len(frame)) + frame
sock.sendto(packet, ('127.0.0.1', 9001))

# Receive response
data, addr = sock.recvfrom(4096)
print(f"Response: {data.hex()}")
```

---

## Multicast Market Data

Receive trade and top-of-book updates via UDP multicast.

### What is Multicast?

Multicast sends data to multiple recipients simultaneously using a special IP address range (224.0.0.0 - 239.255.255.255). All subscribers to the multicast group receive the same data without the server sending individual copies.

**Use case:** Market data distribution where many clients need the same information.

### Terminal 1: Start server with multicast
```bash
cargo run --release -p engine-server
# Multicast publishing on 239.255.0.1:9002
```

### Terminal 2: Subscribe to multicast
```bash
# Using socat
socat UDP4-RECVFROM:9002,ip-add-membership=239.255.0.1:0.0.0.0,fork -
```

### Terminal 3: Generate trades (which get multicast)
```bash
nc localhost 9000
N, 1, IBM, 100, 50, B, 1
N, 2, IBM, 100, 50, S, 1
```

You should see the trade appear in Terminal 2.

### Python multicast subscriber
```python
import socket
import struct

MCAST_GROUP = '239.255.0.1'
MCAST_PORT = 9002

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
sock.bind(('', MCAST_PORT))

# Join multicast group
mreq = struct.pack('4sl', socket.inet_aton(MCAST_GROUP), socket.INADDR_ANY)
sock.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, mreq)

print(f"Listening for multicast on {MCAST_GROUP}:{MCAST_PORT}")

while True:
    data, addr = sock.recvfrom(65536)
    seq_num = struct.unpack('>Q', data[:8])[0]
    frame_len = struct.unpack('>I', data[8:12])[0]
    frame = data[12:12+frame_len]
    print(f"Seq={seq_num} Len={frame_len} Frame={frame.hex()}")
```

### Multicast packet format
```
[0-7]   Sequence number (u64 BE) - for gap detection
[8-11]  Frame length (u32 BE)
[12-N]  Binary frame (standard format)
```

---

## Trading Client

Professional terminal UI for interactive trading.

### Start the trading client
```bash
# Terminal 1: Start server
cargo run --release -p engine-server

# Terminal 2: Start trading client
cargo run --release -p engine-trading-client -- \
    --server 127.0.0.1:9000 \
    --user-id 1 \
    --symbol IBM
```

### Keyboard shortcuts

| Key | Action |
|-----|--------|
| B | Buy order |
| S | Sell order |
| C | Cancel selected |
| X | Cancel all |
| Tab | Next panel |
| ↑/↓ | Navigate |
| Q | Quit |

### Command-line options
```bash
cargo run -p engine-trading-client -- --help

Options:
  --server <HOST:PORT>   Server address (default: 127.0.0.1:9000)
  --user-id <ID>         Your trader ID (default: 1)
  --symbol <SYM>         Initial symbol (default: AAPL)
  --binary               Use binary protocol (default: CSV)
  --debug                Enable debug logging
```

---

## Summary Table

| Transport | Protocol | Port | Command |
|-----------|----------|------|---------|
| TCP | CSV | 9000 | `nc localhost 9000` |
| TCP | Binary | 9000 | Custom client |
| TCP | FIX | 9000 | FIX client |
| UDP | CSV | 9001 | `nc -u localhost 9001` |
| UDP | Binary | 9001 | Custom client |
| Multicast | Binary | 9002 | `socat` subscriber |

---

## Troubleshooting

### Connection refused
```bash
# Check if server is running
ps aux | grep engine-server

# Check if port is in use
lsof -i :9000
```

### No multicast data
```bash
# Check if multicast is enabled
cargo run -p engine-server  # Look for "Multicast: 239.255.0.1:9002"

# Check firewall
sudo ufw allow 9002/udp

# Check multicast routing
ip maddr show
```

### Protocol not detected correctly

The server auto-detects protocol from the first bytes:
- Starts with `MENG` → Binary
- Starts with `8=FIX` → FIX  
- Starts with `N`, `C`, `F`, `Q` → CSV

If having issues, ensure your first message clearly indicates the protocol.
