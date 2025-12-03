# Architecture

This document describes the internal architecture of the Rust Matching Engine, including design principles, data structures, threading model, and performance optimizations.

## Table of Contents

- [Design Principles](#design-principles)
- [Crate Structure](#crate-structure)
- [Data Structures](#data-structures)
- [Threading Model](#threading-model)
- [Message Flow](#message-flow)
- [Memory Management](#memory-management)
- [Performance Optimizations](#performance-optimizations)

---

## Design Principles

### NASA Power of Ten Rules

The engine follows NASA's safety-critical coding rules:

| Rule | Implementation |
|------|----------------|
| 1. Simple control flow | Iterative loops, no recursion |
| 2. Bounded loops | `MAX_MATCH_ITERATIONS = 100,000` |
| 3. No dynamic allocation after init | `Symbol` type, pre-allocated buffers |
| 4. Functions ≤60 lines | Modular design |
| 5. ≥2 assertions per function | `debug_assert!` throughout |
| 6. Smallest scope | Compact structs |
| 7. Check return values | `Option`/`Result` handling |
| 8. Limited macros | Minimal macro usage |
| 9. Limit pointer derefs | Safe Rust patterns |
| 10. Compiler warnings | `#![deny(warnings)]` |

### HFT Low-Latency Principles

- **Cache optimization** — Hot data fits in L1/L2 cache
- **False sharing prevention** — 64-byte alignment between threads
- **Zero-copy where possible** — Slices instead of owned data
- **Bounded operations** — No unbounded loops or allocations
- **Memory locality** — Sequential access patterns

---

## Crate Structure
```
┌─────────────────────────────────────────────────────────────┐
│                      engine-server                          │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────────────┐   │
│  │TCP Listener│UDP Server│Multicast │  Engine Task     │   │
│  └─────────┘ └─────────┘ └─────────┘ └─────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────┴───────────────────────────────┐
│                     engine-protocol                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ Binary Codec │  │  CSV Codec  │  │     FIX Codec       │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────┴───────────────────────────────┐
│                       engine-core                            │
│  ┌──────────┐  ┌───────────┐  ┌───────────┐  ┌──────────┐  │
│  │ OrderBook │  │  Messages  │  │   Order   │  │  Symbol  │  │
│  └──────────┘  └───────────┘  └───────────┘  └──────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### engine-core

Pure, synchronous matching logic with zero external dependencies.

**Key Types:**
- `Symbol` — Fixed 8-byte identifier, `Copy` trait
- `Order` — 64-byte cache-aligned order
- `OrderBook` — Price-time priority book per symbol
- `MatchingEngine` — Multi-symbol engine with routing
- `InputMessage` / `OutputMessage` — Trading messages

### engine-protocol

Wire format encoding/decoding.

**Codecs:**
- `BinaryCodec` — High-performance with magic bytes
- `CsvCodec` — Human-readable for testing
- `FixCodec` — FIX 4.2/4.4 institutional protocol

### engine-server

Async multi-transport server.

**Components:**
- TCP listener with per-client tasks
- UDP server with client tracking
- Multicast publisher for market data
- Central engine task
- Message router

---

## Data Structures

### Symbol (8 bytes)
```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol([u8; 8]);
```

- Fixed size eliminates heap allocation
- `Copy` trait enables zero-cost passing
- Null-padded for shorter symbols (e.g., "IBM" → `[I,B,M,0,0,0,0,0]`)

### Order (64 bytes, cache-aligned)
```rust
#[repr(C, align(64))]
pub struct Order {
    // Hot fields first (offset 0-7)
    pub remaining_qty: u32,    // 0-3: Most frequently accessed
    pub price: u32,            // 4-7: Used in matching
    
    // Identification (offset 8-19)
    pub user_id: u32,          // 8-11
    pub user_order_id: u32,    // 12-15
    pub original_qty: u32,     // 16-19
    
    // Metadata (offset 20-35)
    pub timestamp_ns: u64,     // 20-27
    pub symbol: Symbol,        // 28-35
    
    // Flags (offset 36-37)
    pub side: Side,            // 36
    pub order_type: OrderType, // 37
    
    // Padding (offset 38-63)
    _padding: [u8; 26],
}
```

**Why 64 bytes?**
- Exactly one cache line on most CPUs
- Prevents false sharing between orders
- Predictable memory layout with `#[repr(C)]`

### OrderBook
```rust
pub struct OrderBook {
    symbol: Symbol,
    bids: Vec<PriceLevel>,  // Sorted descending by price
    asks: Vec<PriceLevel>,  // Sorted ascending by price
    // ...
}

struct PriceLevel {
    price: u32,
    orders: Vec<Order>,  // FIFO queue
}
```

**Design choices:**
- `Vec<PriceLevel>` instead of `BTreeMap` for cache locality
- Binary search for price level lookup
- FIFO ordering within price levels

### Message Types
```rust
// Input (client → engine)
pub enum InputMessage {
    NewOrder(NewOrder),
    Cancel(Cancel),
    Flush,
    QueryTopOfBook(TopOfBookQuery),
}

// Output (engine → client)
pub enum OutputMessage {
    Ack(Ack),
    CancelAck(CancelAck),
    Trade(Trade),
    TopOfBook(TopOfBook),
}
```

All message structs use `Symbol` (not `String`) and are `Copy` where possible.

---

## Threading Model
```
┌────────────────────────────────────────────────────────────────┐
│                        Main Thread                              │
│  - Starts Tokio runtime                                        │
│  - Spawns all tasks                                            │
│  - Handles Ctrl+C                                              │
└────────────────────────────────────────────────────────────────┘
         │
         ├──────────────────────────────────────────┐
         │                                          │
         ▼                                          ▼
┌─────────────────────┐                  ┌─────────────────────┐
│   TCP Accept Task   │                  │    UDP Server Task  │
│  - Accept new conns │                  │  - Recv datagrams   │
│  - Spawn client     │                  │  - Track clients    │
│    tasks            │                  │  - Send responses   │
└─────────────────────┘                  └─────────────────────┘
         │                                          │
         ▼                                          │
┌─────────────────────┐                             │
│ Per-Client TCP Task │                             │
│  - Read messages    │                             │
│  - Write responses  │                             │
│  - Protocol decode  │                             │
└─────────────────────┘                             │
         │                                          │
         └──────────────────┬───────────────────────┘
                            │
                            ▼
                 ┌─────────────────────┐
                 │    Engine Task      │
                 │  - Process orders   │
                 │  - Match trades     │
                 │  - Route outputs    │
                 │  - SINGLE THREAD    │
                 └─────────────────────┘
                            │
                            ▼
                 ┌─────────────────────┐
                 │   Multicast Task    │
                 │  - Publish market   │
                 │    data             │
                 └─────────────────────┘
```

**Key Points:**
- Engine task is **single-threaded** — no locking needed for order books
- Client tasks are **independent** — scale with connection count
- **Bounded channels** between tasks prevent memory exhaustion

---

## Message Flow

### Order Submission (TCP/Binary)
```
Client                    TCP Task              Engine Task           Multicast
  │                          │                       │                    │
  │──[Binary Frame]─────────►│                       │                    │
  │                          │                       │                    │
  │                          │──decode_input()──────►│                    │
  │                          │                       │                    │
  │                          │                       │──process()         │
  │                          │                       │  │                 │
  │                          │                       │  ├─► Ack           │
  │                          │                       │  ├─► Trade?        │
  │                          │                       │  └─► TopOfBook?    │
  │                          │                       │                    │
  │                          │◄─────[Ack]────────────│                    │
  │                          │                       │                    │
  │                          │                       │──[Trade]──────────►│
  │                          │                       │──[TOB]────────────►│
  │                          │                       │                    │
  │◄─[Binary Ack]────────────│                       │                    │
  │                          │                       │                    │
```

### Message Routing
```rust
match output_message {
    Ack(_) | CancelAck(_) => {
        // Unicast to originating client only
        send_to_client(originating_client, msg);
    }
    Trade(trade) => {
        // Unicast to both parties
        send_to_user(trade.user_id_buy, msg);
        send_to_user(trade.user_id_sell, msg);
        // Also multicast
        multicast(msg);
    }
    TopOfBook(_) => {
        // Multicast only (market data)
        multicast(msg);
    }
}
```

---

## Memory Management

### Pre-allocation Strategy
```rust
// Engine startup
let mut engine = MatchingEngine::new();
engine.register_symbols([sym("IBM"), sym("AAPL"), sym("GOOG")]);

// Output buffer (reused every message)
let mut outputs: Vec<OutputMessage> = Vec::with_capacity(64);

// Processing loop
loop {
    outputs.clear();  // Reuse, don't reallocate
    engine.process_message(msg, &mut outputs);
    // ...
}
```

### Channel Sizing
```rust
// Bounded channels prevent OOM
let (engine_tx, engine_rx) = mpsc::channel(100_000);  // Engine queue
let (client_tx, client_rx) = mpsc::channel(10_000);   // Per-client
let (mcast_tx, mcast_rx) = mpsc::channel(50_000);     // Multicast
```

### Zero-Allocation Path

The following operations have **zero heap allocations**:

1. Decode binary message → `InputMessage`
2. Process in engine → `Vec<OutputMessage>` (pre-allocated)
3. Route to client → Copy message to channel
4. Encode to binary → Write to pre-allocated buffer

---

## Performance Optimizations

### 1. Cache-Line Alignment
```rust
#[repr(C, align(64))]
pub struct Order { ... }
```

Prevents false sharing when different threads access different orders.

### 2. Hot Fields First
```rust
pub struct Order {
    pub remaining_qty: u32,  // Offset 0 - checked every match iteration
    pub price: u32,          // Offset 4 - used in price comparison
    // ...
}
```

Most-accessed fields at lowest offsets for best cache utilization.

### 3. Sequential Access
```rust
// Good: Sequential iteration through Vec
for level in &self.bids {
    for order in &level.orders {
        // ...
    }
}

// Avoided: BTreeMap with pointer chasing
// btreemap.get(&price)  // Multiple pointer derefs
```

### 4. Symbol as Fixed Array
```rust
// Bad: String (24 bytes + heap)
pub struct Order { symbol: String }

// Good: Fixed array (8 bytes, inline)
pub struct Order { symbol: Symbol }
```

### 5. Bounded Loops
```rust
const MAX_MATCH_ITERATIONS: usize = 100_000;

let mut iterations = 0;
while let Some(order) = self.get_best_order(side) {
    iterations += 1;
    if iterations > MAX_MATCH_ITERATIONS {
        break;  // Safety bound
    }
    // ...
}
```

### 6. Protocol Auto-Detection
```rust
fn detect_protocol(first_bytes: &[u8]) -> Protocol {
    if first_bytes.starts_with(b"MENG") {
        Protocol::Binary
    } else if first_bytes.starts_with(b"8=FIX") {
        Protocol::Fix
    } else {
        Protocol::Csv
    }
}
```

Single peek at connection start, no per-message overhead.

---

## Future Improvements

| Item | Current | Target |
|------|---------|--------|
| Order storage | `Vec<Order>` per level | Order pool with indices |
| HashMap | `std::HashMap` | `FxHashMap` (faster) |
| Cancel lookup | O(n) scan | O(1) via order handle |
| Timestamps | Counter | RDTSC / shared clock |
| Benchmarks | None | Criterion suite |
