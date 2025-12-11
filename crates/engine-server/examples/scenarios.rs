//! Scenario runner for testing the matching engine.
//!
//! Usage:
//!   cargo run -p engine-server --example scenarios -- <scenario_number> [options]
//!
//! Examples:
//!   cargo run -p engine-server --example scenarios -- 1          # Simple orders
//!   cargo run -p engine-server --example scenarios -- 2          # Matching trade
//!   cargo run -p engine-server --example scenarios -- 3          # Cancel order
//!   cargo run -p engine-server --example scenarios -- 20         # 1K matching stress
//!   cargo run -p engine-server --example scenarios -- 22         # 100K matching stress
//!   cargo run -p engine-server --example scenarios -- --binary 1 # Use binary protocol

use std::env;
use std::error::Error;
use std::time::{Duration, Instant};

use engine_core::OutputMessage;
use engine_protocol::{
    binary_codec::{BinaryDecoder, BinaryEncoder},
    csv_codec::{format_output_csv, parse_input_line},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone)]
struct Config {
    server_addr: String,
    use_binary: bool,
    quiet: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server_addr: env::var("ENGINE_SERVER_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:1234".to_string()),
            use_binary: false,
            quiet: false,
        }
    }
}

// ============================================================================
// Response Statistics
// ============================================================================

#[derive(Debug, Default, Clone)]
struct ResponseStats {
    acks: u64,
    cancel_acks: u64,
    trades: u64,
    top_of_book: u64,
    parse_errors: u64,
}

impl ResponseStats {
    fn total(&self) -> u64 {
        self.acks + self.cancel_acks + self.trades + self.top_of_book
    }

    fn add(&mut self, other: &ResponseStats) {
        self.acks += other.acks;
        self.cancel_acks += other.cancel_acks;
        self.trades += other.trades;
        self.top_of_book += other.top_of_book;
        self.parse_errors += other.parse_errors;
    }

    fn print(&self) {
        eprintln!("\n=== Server Response Summary ===");
        eprintln!("ACKs:            {}", self.acks);
        if self.cancel_acks > 0 {
            eprintln!("Cancel ACKs:     {}", self.cancel_acks);
        }
        if self.trades > 0 {
            eprintln!("Trades:          {}", self.trades);
        }
        eprintln!("Top of Book:     {}", self.top_of_book);
        if self.parse_errors > 0 {
            eprintln!("Parse errors:    {}", self.parse_errors);
        }
        eprintln!("Total messages:  {}", self.total());
    }

    fn print_validation(&self, expected_acks: u64, expected_trades: u64) {
        self.print();
        eprintln!("\n=== Validation ===");

        let acks_pass = self.acks >= expected_acks;
        if acks_pass {
            eprintln!("ACKs:            {}/{} ✓ PASS", self.acks, expected_acks);
        } else {
            let pct = if expected_acks > 0 {
                (self.acks * 100) / expected_acks
            } else {
                0
            };
            eprintln!(
                "ACKs:            {}/{} ({}%) ✗ MISSING {}",
                self.acks,
                expected_acks,
                pct,
                expected_acks - self.acks
            );
        }

        let trades_pass = if expected_trades > 0 {
            if self.trades >= expected_trades {
                eprintln!("Trades:          {}/{} ✓ PASS", self.trades, expected_trades);
                true
            } else {
                let pct = (self.trades * 100) / expected_trades;
                eprintln!(
                    "Trades:          {}/{} ({}%) ✗ MISSING {}",
                    self.trades,
                    expected_trades,
                    pct,
                    expected_trades - self.trades
                );
                false
            }
        } else {
            true
        };

        if acks_pass && trades_pass {
            eprintln!("\n*** TEST PASSED ***");
        } else {
            eprintln!("\n*** TEST FAILED ***");
        }
    }
}

// ============================================================================
// Client Connection
// ============================================================================

struct Client {
    stream: TcpStream,
    encoder: BinaryEncoder,
    decoder: BinaryDecoder,
    use_binary: bool,
    read_buf: Vec<u8>,
}

impl Client {
    async fn connect(config: &Config) -> Result<Self, Box<dyn Error>> {
        eprintln!("Connecting to {}...", config.server_addr);
        let stream = TcpStream::connect(&config.server_addr).await?;
        stream.set_nodelay(true)?;
        eprintln!("Connected (protocol: {})", if config.use_binary { "binary" } else { "csv" });

        Ok(Client {
            stream,
            encoder: BinaryEncoder::new(),
            decoder: BinaryDecoder::new(),
            use_binary: config.use_binary,
            read_buf: vec![0u8; 4096],
        })
    }

    async fn send_new_order(
        &mut self,
        user_id: u32,
        symbol: &str,
        price: u32,
        quantity: u32,
        side: char,
        order_id: u32,
    ) -> Result<(), Box<dyn Error>> {
        if self.use_binary {
            self.send_binary_new_order(user_id, symbol, price, quantity, side, order_id)
                .await
        } else {
            self.send_csv_new_order(user_id, symbol, price, quantity, side, order_id)
                .await
        }
    }

    async fn send_csv_new_order(
        &mut self,
        user_id: u32,
        symbol: &str,
        price: u32,
        quantity: u32,
        side: char,
        order_id: u32,
    ) -> Result<(), Box<dyn Error>> {
        let line = format!(
            "N, {}, {}, {}, {}, {}, {}\n",
            user_id, symbol, price, quantity, side, order_id
        );
        self.stream.write_all(line.as_bytes()).await?;
        Ok(())
    }

    async fn send_binary_new_order(
        &mut self,
        user_id: u32,
        symbol: &str,
        price: u32,
        quantity: u32,
        side: char,
        order_id: u32,
    ) -> Result<(), Box<dyn Error>> {
        // Parse via CSV codec then encode to binary
        let csv_line = format!(
            "N, {}, {}, {}, {}, {}, {}",
            user_id, symbol, price, quantity, side, order_id
        );
        let msg = parse_input_line(&csv_line).ok_or("Failed to parse order")?;
        let frame = self.encoder.encode_input(&msg)?;

        // Send length-prefixed frame
        let len = (frame.len() as u32).to_be_bytes();
        self.stream.write_all(&len).await?;
        self.stream.write_all(frame).await?;
        Ok(())
    }

    async fn send_cancel(&mut self, user_id: u32, order_id: u32) -> Result<(), Box<dyn Error>> {
        if self.use_binary {
            let csv_line = format!("C, {}, {}", user_id, order_id);
            let msg = parse_input_line(&csv_line).ok_or("Failed to parse cancel")?;
            let frame = self.encoder.encode_input(&msg)?;
            let len = (frame.len() as u32).to_be_bytes();
            self.stream.write_all(&len).await?;
            self.stream.write_all(frame).await?;
        } else {
            let line = format!("C, {}, {}\n", user_id, order_id);
            self.stream.write_all(line.as_bytes()).await?;
        }
        Ok(())
    }

    async fn send_flush(&mut self) -> Result<(), Box<dyn Error>> {
        if self.use_binary {
            let msg = parse_input_line("F").ok_or("Failed to parse flush")?;
            let frame = self.encoder.encode_input(&msg)?;
            let len = (frame.len() as u32).to_be_bytes();
            self.stream.write_all(&len).await?;
            self.stream.write_all(frame).await?;
        } else {
            self.stream.write_all(b"F\n").await?;
        }
        Ok(())
    }

    /// Try to receive a message with a short timeout (non-blocking style)
    async fn try_recv(&mut self) -> Option<OutputMessage> {
        let result = timeout(Duration::from_millis(1), self.recv_one()).await;
        match result {
            Ok(Ok(msg)) => Some(msg),
            _ => None,
        }
    }

    /// Receive one message (blocking)
    async fn recv_one(&mut self) -> Result<OutputMessage, Box<dyn Error>> {
        if self.use_binary {
            self.recv_binary_one().await
        } else {
            self.recv_csv_one().await
        }
    }

    async fn recv_csv_one(&mut self) -> Result<OutputMessage, Box<dyn Error>> {
        // Read until newline
        let mut line = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            self.stream.read_exact(&mut byte).await?;
            if byte[0] == b'\n' {
                break;
            }
            line.push(byte[0]);
        }
        let line_str = String::from_utf8_lossy(&line);
        parse_csv_output(&line_str)
    }

    async fn recv_binary_one(&mut self) -> Result<OutputMessage, Box<dyn Error>> {
        // Read length prefix
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await?;
        let frame_len = u32::from_be_bytes(len_buf) as usize;

        if frame_len > self.read_buf.len() {
            self.read_buf.resize(frame_len, 0);
        }

        self.stream.read_exact(&mut self.read_buf[..frame_len]).await?;
        let msg = self.decoder.decode_output(&self.read_buf[..frame_len])?;
        Ok(msg)
    }

    /// Drain all pending responses with timeout
    async fn drain_responses(&mut self, timeout_ms: u64) -> ResponseStats {
        let mut stats = ResponseStats::default();
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut consecutive_empty = 0u32;

        while Instant::now() < deadline {
            if let Some(msg) = self.try_recv().await {
                count_message(&mut stats, &msg);
                consecutive_empty = 0;
            } else {
                consecutive_empty += 1;
                if consecutive_empty > 100 {
                    // ~100ms of no data = done
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }

        stats
    }
}

fn count_message(stats: &mut ResponseStats, msg: &OutputMessage) {
    match msg {
        OutputMessage::Ack(_) => stats.acks += 1,
        OutputMessage::CancelAck(_) => stats.cancel_acks += 1,
        OutputMessage::Trade(_) => stats.trades += 1,
        OutputMessage::TopOfBook(_) => stats.top_of_book += 1,
    }
}

fn parse_csv_output(line: &str) -> Result<OutputMessage, Box<dyn Error>> {
    // Simple CSV output parser
    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if parts.is_empty() {
        return Err("Empty line".into());
    }

    match parts[0] {
        "A" => {
            // A, user_id, order_id, symbol
            if parts.len() < 4 {
                return Err("Invalid Ack".into());
            }
            let user_id: u32 = parts[1].parse()?;
            let order_id: u32 = parts[2].parse()?;
            let symbol = engine_core::Symbol::from_str(parts[3]);
            Ok(OutputMessage::Ack(engine_core::Ack::new(user_id, order_id, symbol)))
        }
        "X" => {
            // X, user_id, order_id, symbol (CancelAck)
            if parts.len() < 4 {
                return Err("Invalid CancelAck".into());
            }
            let user_id: u32 = parts[1].parse()?;
            let order_id: u32 = parts[2].parse()?;
            let symbol = engine_core::Symbol::from_str(parts[3]);
            Ok(OutputMessage::CancelAck(engine_core::CancelAck::new(
                user_id, order_id, symbol,
            )))
        }
        "T" => {
            // T, symbol, buy_user, buy_order, sell_user, sell_order, price, qty
            if parts.len() < 8 {
                return Err("Invalid Trade".into());
            }
            let symbol = engine_core::Symbol::from_str(parts[1]);
            let buy_user: u32 = parts[2].parse()?;
            let buy_order: u32 = parts[3].parse()?;
            let sell_user: u32 = parts[4].parse()?;
            let sell_order: u32 = parts[5].parse()?;
            let price: u32 = parts[6].parse()?;
            let qty: u32 = parts[7].parse()?;
            Ok(OutputMessage::Trade(engine_core::Trade::new(
                symbol, buy_user, buy_order, sell_user, sell_order, price, qty,
            )))
        }
        "B" => {
            // B, symbol, side, price, qty (TopOfBook)
            if parts.len() < 5 {
                return Err("Invalid TopOfBook".into());
            }
            let symbol = engine_core::Symbol::from_str(parts[1]);
            let side = match parts[2] {
                "B" => engine_core::Side::Buy,
                "S" => engine_core::Side::Sell,
                _ => return Err("Invalid side".into()),
            };
            // Handle eliminated case (price/qty might be "-")
            if parts[3] == "-" {
                Ok(OutputMessage::TopOfBook(engine_core::TopOfBook::eliminated(
                    symbol, side,
                )))
            } else {
                let price: u32 = parts[3].parse()?;
                let qty: u32 = parts[4].parse()?;
                Ok(OutputMessage::TopOfBook(engine_core::TopOfBook::active(
                    symbol, side, price, qty,
                )))
            }
        }
        _ => Err(format!("Unknown message type: {}", parts[0]).into()),
    }
}

// ============================================================================
// Scenarios
// ============================================================================

async fn run_scenario(client: &mut Client, scenario: u32, quiet: bool) -> Result<(), Box<dyn Error>> {
    match scenario {
        1 => run_scenario_1(client).await,
        2 => run_scenario_2(client).await,
        3 => run_scenario_3(client).await,
        10 => run_stress_test(client, 1_000, quiet).await,
        11 => run_stress_test(client, 10_000, quiet).await,
        12 => run_stress_test(client, 100_000, quiet).await,
        20 => run_matching_stress(client, 1_000, quiet).await,
        21 => run_matching_stress(client, 10_000, quiet).await,
        22 => run_matching_stress(client, 100_000, quiet).await,
        23 => run_matching_stress(client, 250_000, quiet).await,
        24 => run_matching_stress(client, 500_000, quiet).await,
        30 => run_dual_processor_stress(client, 500_000, quiet).await,
        31 => run_dual_processor_stress(client, 1_000_000, quiet).await,
        _ => {
            print_available_scenarios();
            Err("Unknown scenario".into())
        }
    }
}

fn print_available_scenarios() {
    eprintln!("Available scenarios:");
    eprintln!("\nBasic: 1 (orders), 2 (trade), 3 (cancel)");
    eprintln!("\nUnmatched: 10 (1K), 11 (10K), 12 (100K)");
    eprintln!("\nMatching (single symbol - IBM):");
    eprintln!("  20 - 1K trades");
    eprintln!("  21 - 10K trades");
    eprintln!("  22 - 100K trades");
    eprintln!("  23 - 250K trades");
    eprintln!("  24 - 500K trades");
    eprintln!("\nDual-Symbol (IBM + NVDA):");
    eprintln!("  30 - 500K trades  (250K each)");
    eprintln!("  31 - 1M trades    (500K each)");
}

// ============================================================================
// Basic Scenarios
// ============================================================================

async fn run_scenario_1(client: &mut Client) -> Result<(), Box<dyn Error>> {
    eprintln!("=== Scenario 1: Simple Orders ===\n");

    client.send_new_order(1, "IBM", 100, 50, 'B', 1).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    recv_and_print(client).await;

    client.send_new_order(1, "IBM", 105, 50, 'S', 2).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    recv_and_print(client).await;

    Ok(())
}

async fn run_scenario_2(client: &mut Client) -> Result<(), Box<dyn Error>> {
    eprintln!("=== Scenario 2: Matching Trade ===\n");

    client.send_new_order(1, "IBM", 100, 50, 'B', 1).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    recv_and_print(client).await;

    client.send_new_order(1, "IBM", 100, 50, 'S', 2).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    recv_and_print(client).await;

    Ok(())
}

async fn run_scenario_3(client: &mut Client) -> Result<(), Box<dyn Error>> {
    eprintln!("=== Scenario 3: Cancel Order ===\n");

    client.send_new_order(1, "IBM", 100, 50, 'B', 1).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    recv_and_print(client).await;

    client.send_cancel(1, 1).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    recv_and_print(client).await;

    Ok(())
}

async fn recv_and_print(client: &mut Client) {
    for _ in 0..20 {
        if let Some(msg) = client.try_recv().await {
            let csv = format_output_csv(&msg);
            eprintln!("[RECV] {}", csv);
        } else {
            break;
        }
    }
}

// ============================================================================
// Stress Tests
// ============================================================================

async fn run_stress_test(client: &mut Client, count: u64, quiet: bool) -> Result<(), Box<dyn Error>> {
    eprintln!("=== Unmatched Stress: {} Orders ===\n", count);

    client.send_flush().await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = client.drain_responses(500).await;

    let batch_size = if count >= 100_000 { 500 } else if count >= 10_000 { 200 } else { 100 };
    let delay_ms = if count >= 100_000 { 10 } else if count >= 10_000 { 5 } else { 2 };

    if !quiet {
        eprintln!("Throttle: {}/batch, {}ms delay", batch_size, delay_ms);
    }

    let mut send_errors = 0u64;
    let progress_interval = count / 10;
    let mut last_progress = 0u64;
    let start_time = Instant::now();

    for i in 0..count {
        let price = 100 + (i % 100) as u32;
        if client
            .send_new_order(1, "IBM", price, 10, 'B', (i + 1) as u32)
            .await
            .is_err()
        {
            send_errors += 1;
            continue;
        }

        if !quiet && progress_interval > 0 && i > 0 && i / progress_interval > last_progress {
            last_progress = i / progress_interval;
            let pct = (i * 100) / count;
            eprintln!("  {}%", pct);
        }

        if i > 0 && i % batch_size == 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    let elapsed = start_time.elapsed();
    eprintln!("\n=== Send Results ===");
    eprintln!("Orders sent:     {}", count - send_errors);
    eprintln!("Send errors:     {}", send_errors);
    eprintln!("Total time:      {:.3}s", elapsed.as_secs_f64());

    let expected_acks = count - send_errors;
    eprintln!("\nDraining responses...");
    let stats = client.drain_responses(15_000).await;
    stats.print_validation(expected_acks, 0);

    Ok(())
}

async fn run_matching_stress(client: &mut Client, trades: u64, quiet: bool) -> Result<(), Box<dyn Error>> {
    let orders = trades * 2;

    if trades >= 100_000 {
        eprintln!("\n╔══════════════════════════════════════════════════════════╗");
        eprintln!("║  ★★★ MATCHING STRESS TEST ★★★                            ║");
        eprintln!("║  {} TRADES ({} ORDERS)                             ║", trades, orders);
        eprintln!("╚══════════════════════════════════════════════════════════╝\n");
    } else {
        eprintln!("=== Matching Stress Test: {} Trades ===\n", trades);
    }

    client.send_flush().await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = client.drain_responses(500).await;

    let pairs_per_batch = if trades >= 100_000 { 200 } else if trades >= 10_000 { 150 } else { 100 };
    let delay_ms = if trades >= 100_000 { 15 } else if trades >= 10_000 { 10 } else { 5 };

    if !quiet {
        eprintln!(
            "Throttling: {} pairs/batch, {}ms delay (interleaved recv)",
            pairs_per_batch, delay_ms
        );
    }

    let mut send_errors = 0u64;
    let mut pairs_sent = 0u64;
    let mut running_stats = ResponseStats::default();
    let progress_interval = trades / 10;
    let mut last_progress = 0u64;

    let start_time = Instant::now();

    for i in 0..trades {
        let price = 100 + (i % 50) as u32;
        let buy_oid = ((i * 2 + 1) % 0xFFFFFFFF) as u32;
        let sell_oid = ((i * 2 + 2) % 0xFFFFFFFF) as u32;

        if client
            .send_new_order(1, "IBM", price, 10, 'B', buy_oid)
            .await
            .is_err()
        {
            send_errors += 1;
            continue;
        }

        if client
            .send_new_order(1, "IBM", price, 10, 'S', sell_oid)
            .await
            .is_err()
        {
            send_errors += 1;
            continue;
        }

        pairs_sent += 1;

        if !quiet && progress_interval > 0 && i > 0 && i / progress_interval > last_progress {
            last_progress = i / progress_interval;
            let pct = (i * 100) / trades;
            let elapsed = start_time.elapsed().as_millis();
            let rate = if elapsed > 0 {
                pairs_sent * 1000 / elapsed as u64
            } else {
                0
            };
            eprintln!(
                "  {}% | {} pairs | {} ms | {} trades/sec | recv'd: {}",
                pct,
                pairs_sent,
                elapsed,
                rate,
                running_stats.total()
            );
        }

        if i > 0 && i % pairs_per_batch == 0 {
            // Drain aggressively
            for _ in 0..(pairs_per_batch * 6) {
                if let Some(msg) = client.try_recv().await {
                    count_message(&mut running_stats, &msg);
                } else {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    let elapsed = start_time.elapsed();
    let orders_sent = pairs_sent * 2;

    eprintln!("\n=== Send Results ===");
    eprintln!("Trade pairs:     {}", pairs_sent);
    eprintln!("Orders sent:     {}", orders_sent);
    eprintln!("Send errors:     {}", send_errors);
    eprintln!("Total time:      {:.3}s", elapsed.as_secs_f64());

    if elapsed.as_secs() > 0 {
        let throughput = orders_sent / elapsed.as_secs();
        let trade_rate = pairs_sent / elapsed.as_secs();
        eprintln!("\n=== Throughput ===");
        eprintln!("Orders/sec:      {}", throughput);
        eprintln!("Trades/sec:      {}", trade_rate);
    }

    eprintln!("\nReceived during send: {} messages", running_stats.total());

    let expected_acks = orders_sent;
    let expected_trades = pairs_sent;
    let expected_total = expected_acks + expected_trades + expected_trades * 2;
    let remaining = expected_total.saturating_sub(running_stats.total());
    eprintln!("Final drain (expecting ~{} more)...", remaining);

    let drain_timeout = if trades >= 100_000 { 120_000 } else { 60_000 };
    let final_stats = client.drain_responses(drain_timeout).await;

    let mut total_stats = ResponseStats::default();
    total_stats.add(&running_stats);
    total_stats.add(&final_stats);

    total_stats.print_validation(expected_acks, expected_trades);

    client.send_flush().await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    Ok(())
}

async fn run_dual_processor_stress(
    client: &mut Client,
    trades: u64,
    quiet: bool,
) -> Result<(), Box<dyn Error>> {
    let orders = trades * 2;
    let trades_per_symbol = trades / 2;

    eprintln!("\n╔══════════════════════════════════════════════════════════╗");
    eprintln!("║  ★★★ DUAL-SYMBOL STRESS TEST ★★★                         ║");
    eprintln!("║  {} TRADES ({} ORDERS)                             ║", trades, orders);
    eprintln!("║  IBM:  {} trades                                   ║", trades_per_symbol);
    eprintln!("║  NVDA: {} trades                                   ║", trades_per_symbol);
    eprintln!("╚══════════════════════════════════════════════════════════╝\n");

    client.send_flush().await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = client.drain_responses(500).await;

    let pairs_per_batch = 200u64;
    let delay_ms = 15u64;

    if !quiet {
        eprintln!(
            "Throttling: {} pairs/batch, {}ms delay (interleaved recv)",
            pairs_per_batch, delay_ms
        );
    }

    let symbols = ["IBM", "NVDA"];
    let mut send_errors = 0u64;
    let mut pairs_sent = 0u64;
    let mut running_stats = ResponseStats::default();
    let progress_interval = trades / 10;
    let mut last_progress = 0u64;

    let start_time = Instant::now();

    for i in 0..trades {
        let symbol = symbols[(i % 2) as usize];
        let price = 100 + (i % 50) as u32;
        let buy_oid = ((i * 2 + 1) % 0xFFFFFFFF) as u32;
        let sell_oid = ((i * 2 + 2) % 0xFFFFFFFF) as u32;

        if client
            .send_new_order(1, symbol, price, 10, 'B', buy_oid)
            .await
            .is_err()
        {
            send_errors += 1;
            continue;
        }

        if client
            .send_new_order(1, symbol, price, 10, 'S', sell_oid)
            .await
            .is_err()
        {
            send_errors += 1;
            continue;
        }

        pairs_sent += 1;

        if !quiet && progress_interval > 0 && i > 0 && i / progress_interval > last_progress {
            last_progress = i / progress_interval;
            let pct = (i * 100) / trades;
            let elapsed = start_time.elapsed().as_millis();
            let rate = if elapsed > 0 {
                pairs_sent * 1000 / elapsed as u64
            } else {
                0
            };
            eprintln!(
                "  {}% | {} pairs | {} ms | {} trades/sec | recv'd: {}",
                pct,
                pairs_sent,
                elapsed,
                rate,
                running_stats.total()
            );
        }

        if i > 0 && i % pairs_per_batch == 0 {
            for _ in 0..(pairs_per_batch * 6) {
                if let Some(msg) = client.try_recv().await {
                    count_message(&mut running_stats, &msg);
                } else {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    let elapsed = start_time.elapsed();
    let orders_sent = pairs_sent * 2;

    eprintln!("\n=== Send Results ===");
    eprintln!("Trade pairs:     {}", pairs_sent);
    eprintln!("Orders sent:     {}", orders_sent);
    eprintln!("Send errors:     {}", send_errors);
    eprintln!("Total time:      {:.3}s", elapsed.as_secs_f64());

    if elapsed.as_secs() > 0 {
        let throughput = orders_sent / elapsed.as_secs();
        let trade_rate = pairs_sent / elapsed.as_secs();
        eprintln!("\n=== Throughput ===");
        eprintln!("Orders/sec:      {}", throughput);
        eprintln!("Trades/sec:      {}", trade_rate);
    }

    eprintln!("\nReceived during send: {} messages", running_stats.total());

    let expected_acks = orders_sent;
    let expected_trades = pairs_sent;
    let expected_total = expected_acks + expected_trades + expected_trades * 2;
    let remaining = expected_total.saturating_sub(running_stats.total());
    eprintln!("Final drain (expecting ~{} more)...", remaining);

    let drain_timeout = if trades >= 1_000_000 { 600_000 } else { 300_000 };
    let final_stats = client.drain_responses(drain_timeout).await;

    let mut total_stats = ResponseStats::default();
    total_stats.add(&running_stats);
    total_stats.add(&final_stats);

    total_stats.print_validation(expected_acks, expected_trades);

    client.send_flush().await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    Ok(())
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();

    let mut config = Config::default();
    let mut scenario: Option<u32> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--binary" | "-b" => config.use_binary = true,
            "--quiet" | "-q" => config.quiet = true,
            "--server" | "-s" => {
                i += 1;
                if i < args.len() {
                    config.server_addr = args[i].clone();
                }
            }
            "--help" | "-h" => {
                eprintln!("Usage: scenarios [OPTIONS] <SCENARIO_NUMBER>");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  -b, --binary        Use binary protocol (default: CSV)");
                eprintln!("  -q, --quiet         Reduce output verbosity");
                eprintln!("  -s, --server ADDR   Server address (default: 127.0.0.1:1234)");
                eprintln!("  -h, --help          Show this help");
                eprintln!();
                print_available_scenarios();
                return Ok(());
            }
            s => {
                if let Ok(n) = s.parse::<u32>() {
                    scenario = Some(n);
                } else {
                    eprintln!("Unknown argument: {}", s);
                }
            }
        }
        i += 1;
    }

    let scenario = match scenario {
        Some(s) => s,
        None => {
            eprintln!("Error: No scenario number provided\n");
            print_available_scenarios();
            return Err("No scenario specified".into());
        }
    };

    let mut client = Client::connect(&config).await?;
    run_scenario(&mut client, scenario, config.quiet).await
}
