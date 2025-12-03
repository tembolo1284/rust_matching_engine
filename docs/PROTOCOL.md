# Protocol Specification

This document describes the wire protocols supported by the Rust Matching Engine.

## Table of Contents

- [Overview](#overview)
- [Binary Protocol](#binary-protocol)
- [CSV Protocol](#csv-protocol)
- [FIX Protocol](#fix-protocol)
- [Multicast Protocol](#multicast-protocol)

---

## Overview

| Protocol | Transport | Latency | Human Readable | Use Case |
|----------|-----------|---------|----------------|----------|
| Binary | TCP, UDP, Multicast | Lowest | No | Production HFT |
| CSV | TCP, UDP | High | Yes | Testing, debugging |
| FIX 4.2/4.4 | TCP | Medium | Partially | Institutional |

---

## Binary Protocol

### Frame Format

All binary messages use the following framing:
```
┌─────────────────────────────────────────────────────────────┐
│ Offset │ Size │ Field        │ Description                  │
├────────┼──────┼──────────────┼──────────────────────────────┤
│ 0      │ 4    │ magic        │ 0x4D454E47 ("MENG")          │
│ 4      │ 1    │ version      │ Protocol version (1)         │
│ 5      │ 1    │ msg_type     │ Message type ID              │
│ 6      │ 2    │ payload_len  │ Payload length (BE u16)      │
│ 8      │ N    │ payload      │ Message-specific data        │
└─────────────────────────────────────────────────────────────┘
```

**Magic Bytes:** `0x4D 0x45 0x4E 0x47` = ASCII "MENG" (Matching ENGine)

### TCP Framing

For TCP, add a 4-byte length prefix before the frame:
```
┌────────────┬─────────────────────────────────┐
│ len (u32)  │ frame (magic + header + payload)│
└────────────┴─────────────────────────────────┘
```

### Message Types

#### Input Messages (Client → Server)

| Type ID | Name | Description |
|---------|------|-------------|
| 0 | NewOrder | Submit new order |
| 1 | Cancel | Cancel existing order |
| 2 | Flush | Clear all order books |
| 3 | QueryTopOfBook | Request current TOB |

#### Output Messages (Server → Client)

| Type ID | Name | Description |
|---------|------|-------------|
| 10 | Ack | Order acknowledged |
| 11 | CancelAck | Cancel acknowledged |
| 12 | Trade | Trade executed |
| 13 | TopOfBook | Best bid/ask update |

### Payload Formats

#### NewOrder (type=0)
```
Offset  Size  Field           Type      Description
0       4     user_id         u32 BE    User identifier
4       4     user_order_id   u32 BE    Client order ID
8       4     price           u32 BE    Price in ticks (0=market)
12      4     quantity        u32 BE    Order quantity
16      1     side            u8        0=Buy, 1=Sell
17      8     symbol          [u8; 8]   Symbol (null-padded)
```

Total payload: 25 bytes

#### Cancel (type=1)
```
Offset  Size  Field           Type      Description
0       4     user_id         u32 BE    User identifier
4       4     user_order_id   u32 BE    Order to cancel
```

Total payload: 8 bytes

#### Flush (type=2)

No payload.

#### QueryTopOfBook (type=3)
```
Offset  Size  Field           Type      Description
0       8     symbol          [u8; 8]   Symbol to query
```

Total payload: 8 bytes

#### Ack (type=10)
```
Offset  Size  Field           Type      Description
0       4     user_id         u32 BE    User identifier
4       4     user_order_id   u32 BE    Acknowledged order
8       8     symbol          [u8; 8]   Symbol
```

Total payload: 16 bytes

#### CancelAck (type=11)

Same format as Ack.

#### Trade (type=12)
```
Offset  Size  Field             Type      Description
0       8     symbol            [u8; 8]   Symbol
8       4     user_id_buy       u32 BE    Buyer user ID
12      4     user_order_id_buy u32 BE    Buyer order ID
16      4     user_id_sell      u32 BE    Seller user ID
20      4     user_order_id_sell u32 BE   Seller order ID
24      4     price             u32 BE    Trade price
28      4     quantity          u32 BE    Trade quantity
```

Total payload: 32 bytes

#### TopOfBook (type=13)
```
Offset  Size  Field           Type      Description
0       8     symbol          [u8; 8]   Symbol
8       1     side            u8        0=Bid, 1=Ask
9       1     eliminated      u8        1=No liquidity
10      4     price           u32 BE    Best price
14      4     total_quantity  u32 BE    Total qty at price
```

Total payload: 18 bytes

### Example: Encoding a NewOrder
```
Order: Buy 100 IBM @ 150.00 (user=1, order_id=42)

Hex dump:
4D 45 4E 47    # Magic "MENG"
01             # Version 1
00             # Type 0 (NewOrder)
00 19          # Payload length 25
00 00 00 01    # user_id = 1
00 00 00 2A    # user_order_id = 42
00 00 3A 98    # price = 15000 (150.00 * 100)
00 00 00 64    # quantity = 100
00             # side = Buy
49 42 4D 00 00 00 00 00  # symbol = "IBM\0\0\0\0\0"
```

---

## CSV Protocol

Human-readable text format, one message per line.

### Input Format
```
# New Order
N, <user_id>, <symbol>, <price>, <qty>, <side>, <order_id>

# Cancel
C, <user_id>, <order_id>

# Flush
F

# Query Top of Book
Q, <symbol>
```

**Side:** `B` = Buy, `S` = Sell

### Output Format
```
# Ack
A, <user_id>, <order_id>, <symbol>

# CancelAck  
C, <user_id>, <order_id>, <symbol>

# Trade
T, <symbol>, <buy_uid>, <buy_oid>, <sell_uid>, <sell_oid>, <price>, <qty>

# TopOfBook
B, <symbol>, <side>, <price>, <qty>

# TopOfBook (eliminated)
B, <symbol>, <side>, -, -
```

### Example Session
```
# Client sends:
N, 1, IBM, 100, 50, B, 1
N, 2, IBM, 100, 50, S, 1

# Server responds:
A, 1, 1, IBM
B, IBM, B, 100, 50
A, 2, 1, IBM
T, IBM, 1, 1, 2, 1, 100, 50
B, IBM, B, -, -
```

### Legacy Format

For compatibility with older systems, a legacy output format is available:
```
A, <user_id>, <order_id>           # No symbol
T, <buy_uid>, <buy_oid>, <sell_uid>, <sell_oid>, <price>, <qty>
B, <side>, <price>, <qty>          # No symbol
```

---

## FIX Protocol

FIX 4.2 and 4.4 support for institutional connectivity.

### Supported Message Types

| MsgType (35=) | Name | Direction |
|---------------|------|-----------|
| D | NewOrderSingle | Client → Server |
| F | OrderCancelRequest | Client → Server |
| 8 | ExecutionReport | Server → Client |

### Key Tags

| Tag | Name | Description |
|-----|------|-------------|
| 8 | BeginString | "FIX.4.2" or "FIX.4.4" |
| 9 | BodyLength | Message body length |
| 10 | CheckSum | 3-digit checksum |
| 11 | ClOrdID | Client order ID |
| 17 | ExecID | Execution ID |
| 35 | MsgType | Message type |
| 37 | OrderID | Server order ID |
| 38 | OrderQty | Order quantity |
| 39 | OrdStatus | Order status |
| 40 | OrdType | 1=Market, 2=Limit |
| 44 | Price | Limit price |
| 49 | SenderCompID | Sender ID |
| 54 | Side | 1=Buy, 2=Sell |
| 55 | Symbol | Instrument |
| 56 | TargetCompID | Target ID |
| 150 | ExecType | 0=New, 4=Canceled, F=Trade |

### Example: NewOrderSingle
```
8=FIX.4.4|9=XXX|35=D|49=CLIENT|56=ENGINE|34=1|52=20240101-12:00:00.000|
11=12345|55=IBM|54=1|60=20240101-12:00:00.000|38=100|40=2|44=150.00|10=XXX|
```

### Example: ExecutionReport (Trade)
```
8=FIX.4.4|9=XXX|35=8|49=ENGINE|56=CLIENT|34=1|52=20240101-12:00:00.001|
37=12345|11=12345|17=E1|150=F|39=2|55=IBM|54=1|31=150.00|32=100|151=0|14=100|6=150.00|10=XXX|
```

---

## Multicast Protocol

UDP multicast for market data broadcast.

### Configuration

- **Default Group:** 239.255.0.1
- **Default Port:** 9002
- **TTL:** 1 (local network)

### Packet Format
```
┌─────────────────────────────────────────────────────────────┐
│ Offset │ Size │ Field        │ Description                  │
├────────┼──────┼──────────────┼──────────────────────────────┤
│ 0      │ 8    │ seq_num      │ Sequence number (u64 BE)     │
│ 8      │ 4    │ frame_len    │ Binary frame length (u32 BE) │
│ 12     │ N    │ frame        │ Standard binary frame        │
└─────────────────────────────────────────────────────────────┘
```

### Published Messages

Only market data messages are multicast:

- **Trade** — All executed trades
- **TopOfBook** — Best bid/ask updates

### Gap Detection

Clients should track `seq_num` to detect gaps:
```rust
let expected_seq = last_seq + 1;
if packet.seq_num != expected_seq {
    // Gap detected: missed (packet.seq_num - expected_seq) messages
    request_retransmit(expected_seq, packet.seq_num - 1);
}
last_seq = packet.seq_num;
```

### Joining the Multicast Group
```rust
use std::net::{Ipv4Addr, UdpSocket};

let socket = UdpSocket::bind("0.0.0.0:9002")?;
socket.join_multicast_v4(
    &Ipv4Addr::new(239, 255, 0, 1),
    &Ipv4Addr::UNSPECIFIED,
)?;

let mut buf = [0u8; 65536];
loop {
    let (len, _) = socket.recv_from(&mut buf)?;
    process_multicast_packet(&buf[..len]);
}
```

---

## Error Handling

### Binary Protocol Errors

| Error | Description |
|-------|-------------|
| InvalidMagic | Magic bytes don't match "MENG" |
| VersionMismatch | Unsupported protocol version |
| Truncated | Incomplete message |
| UnknownMessageType | Invalid message type ID |
| InvalidSymbol | Empty or too long symbol |
| InvalidField | Field value out of range |

### CSV Protocol Errors

Invalid lines are silently ignored. Comments start with `#`.

### FIX Protocol Errors

| Error | Description |
|-------|-------------|
| MissingField | Required tag missing |
| InvalidField | Tag value invalid |
| ChecksumMismatch | Checksum verification failed |
