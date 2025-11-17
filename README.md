# Rust Matching Engine  
A multi-symbol, multi-client, TCP-based, high-performance trade matching engine inspired by your original C++ UDP/CSV order-book implementation — redesigned in Rust for scalability, safety, and extensibility.

This engine supports:

- **Binary protocol** for efficient wire communication  
- **CSV protocol** (for compatibility & easy testing)  
- **Multiple TCP clients connected simultaneously**  
- **Real-time broadcasting** of Acks / Trades / Top-of-Book updates  
- **Full order books per symbol**  
- **Flush with CancelAck generation**  
- **QueryTopOfBook event**  
- **Beautiful startup and shutdown status banners**  
- **Per-client outbound channels with backpressure-free fanout**  

---

## Project Structure

rust_matching_engine/

├── Cargo.toml # Workspace manifest

├── crates/

│ ├── engine-core/ # Matching logic (order book, matching, TOB)

│ ├── engine-protocol/ # Binary + CSV codecs

│ ├── engine-server/ # TCP server, client registry, engine task

│ └── engine-udp-adapter/ # Placeholder (future UDP support)

└── tests/ # Integration tests

## Architecture Overview

### 1. engine-core — pure matching logic

Contains all the real work:

- OrderBook
- Order structs
- MatchingEngine
- OutputMessage (Ack, CancelAck, Trade, TopOfBook)
- NewOrder / Cancel / QueryTopOfBook
- Flush (clears book + emits cancel acks)

Completely synchronous and deterministic.

---

### 2. engine-protocol — encoding/decoding

Supports both:

#### CSV protocol (easy for testing)

N, 1, IBM, 10, 100, B, 1

C, 1, 1

Q, IBM

F

#### Binary protocol (length-prefixed)
Used for efficient transmission over TCP.

---

### 3. engine-server — async TCP server

- Tokio-based async architecture  
- Per-client tasks  
- Central engine task  
- Broadcast pub-sub fanout  
- Graceful shutdown  
- Statistics collection  
- Auto-port fallback (9000 → 9001 → 9002)  

---

## Build Instructions

### Build whole workspace:

cargo build --workspace

### Release:

cargo build --release

## Running the Server

### Default:

cargo run -p engine-server

### Explicit bind address:

ENGINE_BIND_ADDR=127.0.0.1 ENGINE_PORT=9000 cargo run -p engine-server

### Auto-port fallback

If port 9000 is taken:

9000 -> 9001 -> 9002

## Startup Banner

==============================================================
Order Book - TCP Matching Engine

Bind address: 0.0.0.0

TCP Port: 9001

Max clients: 1024

Note: bound after 2 attempts (port bumped due to AddrInUse).

Queue Configuration:

Engine request queue: Tokio mpsc::unbounded_channel()

Client outbound queues: Tokio mpsc::unbounded_channel() per client

Starting tasks...

Engine task: started

TCP listener: starting on 0.0.0.0:9001

TCP listener ready on 0.0.0.0:9001 (press Ctrl+C to shutdown gracefully)

## Running the Example TCP Client

ENGINE_CLIENT_ADDR=127.0.0.1:9001 cargo run -p engine-server --example tcp_client

N, 1, IBM, 10, 100, B, 1

N, 2, IBM, 9, 50, S, 2

Q, IBM

F

Output format:

- `>>` sent to engine  

- `<<` received from engine  

Example session:

N, 1, IBM, 10, 100, B, 1

<< A, 1, 1, IBM

<< B, IBM, B, 10, 100

N, 2, IBM, 9, 50, 2

<< A, 2, 2, IBM

<< T, IBM, 1, 1, 2, 2, 10, 50

<< B, IBM, B, 10, 50

## Using netcat (CSV inpt file or single commands)

### Send one order:

echo "N, 1, IBM, 10, 100, B, 1" | nc 127.0.0.1 9001 &

### Send an entire CSV file:

nc 127.0.0.1 9001 < data/inputFile.csv

Or:

cat data/inputFile.csv | nc 127.0.0.1 9001 &

## Running Tests

### All tests:

cargo test

## Shutdown and Statistics

Press **Ctrl+C** and you'll see:

==============================================================

Shutting down engine...

Requests received: 42

Outputs generated: 71

Goodbye!



