# Rust Matching Engine

A high-performance, multi-protocol, multi-transport order matching engine built in Rust following NASA Power of Ten safety-critical coding rules and HFT low-latency principles.

## Features

- **Zero-allocation hot path** — Pre-allocated buffers, fixed-size types, no heap allocation during trading
- **Cache-optimized** — 64-byte aligned orders, sequential memory access, L1/L2 cache friendly
- **Multi-transport** — TCP, UDP, and Multicast support
- **Multi-protocol** — CSV (human-readable), Binary (high-performance), FIX 4.2/4.4 (institutional)
- **Bounded channels** — Backpressure handling prevents memory exhaustion
- **Protocol auto-detection** — Automatically detects CSV/Binary/FIX from first bytes
- **Smart message routing** — Acks to originator, trades to both parties, market data via multicast

## Quick Start
```bash
# Build
cargo build --release

# Run server with all transports enabled
cargo run -p engine-server

# Test with netcat (CSV over TCP)
echo "N, 1, IBM, 100, 50, B, 1" | nc localhost 9000
```

See [QUICK_START.md](docs/QUICK_START.md) for detailed examples of all transport/protocol combinations.

## Project Structure
```
rust_matching_engine/
├── crates/
│   ├── engine-core/          # Matching logic (zero-allocation)
│   ├── engine-protocol/      # Binary, CSV, FIX codecs
│   ├── engine-server/        # Multi-transport async server
│   └── engine-trading-client/ # Terminal UI client
├── docs/
│   ├── ARCHITECTURE.md       # System design details
│   ├── PROTOCOL.md           # Wire protocol specifications
│   └── QUICK_START.md        # Launch examples
└── tests/                    # Integration tests
```

## Transport & Protocol Matrix

| Transport | CSV | Binary | FIX 4.2/4.4 | Use Case |
|-----------|:---:|:------:|:-----------:|----------|
| TCP | ✓ | ✓ | ✓ | General purpose, reliable delivery |
| UDP | ✓ | ✓ | ✗ | Ultra-low latency |
| Multicast | ✗ | ✓ | ✗ | Market data broadcast |

## Message Routing

| Message Type | Routing | Multicast |
|--------------|---------|:---------:|
| Ack | Originating client only | ✗ |
| CancelAck | Originating client only | ✗ |
| Trade | Buyer + Seller | ✓ |
| TopOfBook | — | ✓ |

## Performance Characteristics

| Metric | Value |
|--------|-------|
| Order struct size | 64 bytes (cache-line aligned) |
| Symbol size | 8 bytes (fixed, Copy) |
| Message size | 16-40 bytes |
| Hot path allocations | 0 |
| Channel backpressure | Bounded (configurable) |

## Configuration

### Environment Variables
```bash
# TCP
ENGINE_TCP_ADDR=0.0.0.0
ENGINE_TCP_PORT=9000
ENGINE_TCP_ENABLED=true

# UDP  
ENGINE_UDP_ADDR=0.0.0.0
ENGINE_UDP_PORT=9001
ENGINE_UDP_ENABLED=true

# Multicast
ENGINE_MCAST_GROUP=239.255.0.1
ENGINE_MCAST_PORT=9002
ENGINE_MCAST_ENABLED=true

# Limits
ENGINE_MAX_TCP_CLIENTS=1024
ENGINE_CHANNEL_CAPACITY=100000
```

### Command Line
```bash
cargo run -p engine-server -- --tcp-port 9000 --udp-port 9001 --no-mcast
cargo run -p engine-server -- --help
```

## Building
```bash
# Debug build
cargo build --workspace

# Release build (optimized)
cargo build --release --workspace

# Run tests
cargo test --workspace
```

## Documentation

- [ARCHITECTURE.md](docs/ARCHITECTURE.md) — System design, data structures, threading model
- [PROTOCOL.md](docs/PROTOCOL.md) — Wire format specifications for all protocols
- [QUICK_START.md](docs/QUICK_START.md) — Step-by-step examples for every transport/protocol

## Trading Client

A professional terminal UI for live trading:
```bash
cargo run -p engine-trading-client -- --server 127.0.0.1:9000 --symbol IBM
```

See the [Trading Client section](docs/QUICK_START.md#trading-client) for details.

