# Quick Start Guide

This guide shows how to run the matching engine server and test it with various clients.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Building](#building)
- [Server Startup](#server-startup)
- [Testing with Scenarios](#testing-with-scenarios)
- [Testing with CSV (netcat)](#testing-with-csv-netcat)
- [Trading Client (TUI)](#trading-client-tui)
- [Multicast Market Data](#multicast-market-data)
- [Protocol Reference](#protocol-reference)

---

## Prerequisites

- Rust 1.70+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- netcat (`nc`) for CSV testing
- Optional: `socat` for multicast testing

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
  TCP:       0.0.0.0:1234 (CSV, Binary, FIX)
  UDP:       0.0.0.0:1235 (CSV, Binary)
  Multicast: 239.255.0.1:1236 (Binary)

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

## Testing with Scenarios

The scenarios runner is the recommended way to test the engine. It uses the **binary protocol** by default.

### Start server (Terminal 1)

```bash
cargo run --release -p engine-server
```

### Run scenarios (Terminal 2)

```bash
# List available scenarios
cargo run -p engine-server --example scenarios -- --help

# Basic scenarios
cargo run -p engine-server --example scenarios -- 1    # Simple orders
cargo run -p engine-server --example scenarios -- 2    # Matching trade
cargo run -p engine-server --example scenarios -- 3    # Cancel order

# Use CSV protocol instead of binary
cargo run -p engine-server --example scenarios -- --csv 1

# Stress tests (unmatched orders)
cargo run -p engine-server --example scenarios -- 10   # 1K orders
cargo run -p engine-server --example scenarios -- 11   # 10K orders
cargo run -p engine-server --example scenarios -- 12   # 100K orders

# Matching stress tests (orders that trade)
cargo run -p engine-server --example scenarios -- 20   # 1K trades
cargo run -p engine-server --example scenarios -- 21   # 10K trades
cargo run -p engine-server --example scenarios -- 22   # 100K trades
cargo run -p engine-server --example scenarios -- 23   # 250K trades
cargo run -p engine-server --example scenarios -- 24   # 500K trades

# Dual-symbol stress (IBM + NVDA)
cargo run -p engine-server --example scenarios -- 30   # 500K trades
cargo run -p engine-server --example scenarios -- 31   # 1M trades
```

### Example output (Scenario 1)

```
Connecting to 127.0.0.1:1234...
Connected (protocol: binary)
=== Scenario 1: Simple Orders ===

[RECV] A, IBM, 1, 1
[RECV] B, IBM, B, 100, 50
[RECV] A, IBM, 1, 2
[RECV] B, IBM, S, 105, 50

[Flush]
[RECV] C, IBM, 1, 1
[RECV] C, IBM, 1, 2
[RECV] B, IBM, B, -, -
[RECV] B, IBM, S, -, -
```

### Example output (Scenario 2 - Matching Trade)

```
Connecting to 127.0.0.1:1234...
Connected (protocol: binary)
=== Scenario 2: Matching Trade ===

[RECV] A, IBM, 1, 1
[RECV] B, IBM, B, 100, 50
[RECV] A, IBM, 1, 2
[RECV] T, IBM, 1, 1, 1, 2, 100, 50
[RECV] B, IBM, B, -, -

[Flush]
```

---

## Testing with CSV (netcat)

For quick manual testing, use netcat with the CSV protocol.

### Terminal 1: Start server

```bash
cargo run --release -p engine-server
```

### Terminal 2: Connect with netcat

```bash
nc localhost 1234
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

### CSV Message Format

**Input (client → server):**

| Type | Format | Example |
|------|--------|---------|
| New Order | `N, user_id, symbol, price, qty, side, order_id` | `N, 1, IBM, 100, 50, B, 1` |
| Cancel | `C, user_id, order_id` | `C, 1, 1` |
| Flush | `F` | `F` |
| Query TOB | `Q, symbol` | `Q, IBM` |

**Output (server → client):**

| Type | Format | Example |
|------|--------|---------|
| Ack | `A, symbol, user_id, order_id` | `A, IBM, 1, 1` |
| CancelAck | `C, symbol, user_id, order_id` | `C, IBM, 1, 1` |
| Trade | `T, symbol, buy_user, buy_oid, sell_user, sell_oid, price, qty` | `T, IBM, 1, 1, 2, 1, 100, 50` |
| TopOfBook | `B, symbol, side, price, qty` | `B, IBM, B, 100, 50` |
| TOB Eliminated | `B, symbol, side, -, -` | `B, IBM, B, -, -` |

---

## Trading Client (TUI)

Professional terminal UI for interactive trading.

### Start the trading client

```bash
# Terminal 1: Start server
cargo run --release -p engine-server

# Terminal 2: Start trading client
cargo run --release -p engine-trading-client -- \
    --server 127.0.0.1:1234 \
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
  --server <HOST:PORT>   Server address (default: 127.0.0.1:1234)
  --user-id <ID>         Your trader ID (default: 1)
  --symbol <SYM>         Initial symbol (default: AAPL)
  --binary               Use binary protocol (default)
  --csv                  Use CSV protocol
  --debug                Enable debug logging
```

---

## Multicast Market Data

Receive trade and top-of-book updates via UDP multicast.

### Terminal 1: Start server with multicast

```bash
cargo run --release -p engine-server
# Multicast publishing on 239.255.0.1:1236
```

### Terminal 2: Subscribe to multicast

```bash
# Using socat
socat UDP4-RECVFROM:1236,ip-add-membership=239.255.0.1:0.0.0.0,fork -
```

### Terminal 3: Generate trades

```bash
cargo run -p engine-server --example scenarios -- 2
```

You should see the trade appear in Terminal 2.

### Python multicast subscriber

```python
import socket
import struct

MCAST_GROUP = '239.255.0.1'
MCAST_PORT = 1236

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

---

## Protocol Reference

### Binary Protocol

The binary protocol uses a simple frame format with length prefix for TCP:

**TCP Framing:** `[4-byte length BE][frame]`

**Frame Format:**
```
[0]   : magic = 0x4D ('M')
[1]   : msg_type
[2]   : version (1)
[3]   : reserved (0)
[4..] : body (varies by message type)
```

**Message Types:**

| Type | ID | Direction |
|------|----|-----------|
| NewOrder | 0 | Input |
| Cancel | 1 | Input |
| Flush | 2 | Input |
| QueryTOB | 3 | Input |
| Ack | 10 | Output |
| CancelAck | 11 | Output |
| Trade | 12 | Output |
| TopOfBook | 13 | Output |

### Protocol Auto-Detection

The server auto-detects the protocol from the first bytes:
- Starts with `M` + valid msg_type → Binary
- Starts with `8=FIX` → FIX  
- Starts with `N`, `C`, `F`, `Q` → CSV

---

## Summary Table

| Transport | Protocol | Port | Test Command |
|-----------|----------|------|--------------|
| TCP | Binary | 1234 | `cargo run -p engine-server --example scenarios -- 1` |
| TCP | CSV | 1234 | `nc localhost 1234` |
| TCP | FIX | 1234 | FIX client |
| UDP | CSV | 1235 | `nc -u localhost 1235` |
| UDP | Binary | 1235 | Custom client |
| Multicast | Binary | 1236 | `socat` subscriber |

---

## Troubleshooting

### Connection refused

```bash
# Check if server is running
ps aux | grep engine-server

# Check if port is in use
lsof -i :1234
```

### No multicast data

```bash
# Check if multicast is enabled
cargo run -p engine-server  # Look for "Multicast: 239.255.0.1:1236"

# Check firewall
sudo ufw allow 1236/udp

# Check multicast routing
ip maddr show
```

### Protocol not detected correctly

The server auto-detects protocol from the first bytes:
- Binary: First byte is `M` (0x4D) followed by valid msg_type (0-3)
- FIX: Starts with `8=`
- CSV: Starts with `N`, `C`, `F`, `Q`, or other printable ASCII

If having issues, ensure your first message clearly indicates the protocol.
