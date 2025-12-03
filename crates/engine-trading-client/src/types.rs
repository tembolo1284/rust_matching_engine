//! Shared types for the trading client.

use chrono::{DateTime, Local};
use engine_core::{Side, Symbol};

/// Transport type discovered from server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Udp,
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transport::Tcp => write!(f, "TCP"),
            Transport::Udp => write!(f, "UDP"),
        }
    }
}

/// Protocol type discovered from server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Csv,
    Binary,
    Fix,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::Csv => write!(f, "CSV"),
            Protocol::Binary => write!(f, "Binary"),
            Protocol::Fix => write!(f, "FIX"),
        }
    }
}

/// Client order representation.
#[derive(Debug, Clone)]
pub struct Order {
    pub order_id: u32,
    pub symbol: Symbol,
    pub side: Side,
    pub price: u32,
    pub quantity: u32,
    pub filled_qty: u32,
    pub status: OrderStatus,
    pub timestamp: DateTime<Local>,
}

/// Order status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum OrderStatus {
    Pending,
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
}

impl std::fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderStatus::Pending => write!(f, "PENDING"),
            OrderStatus::Open => write!(f, "OPEN"),
            OrderStatus::PartiallyFilled => write!(f, "PARTIAL"),
            OrderStatus::Filled => write!(f, "FILLED"),
            OrderStatus::Cancelled => write!(f, "CANCELLED"),
            OrderStatus::Rejected => write!(f, "REJECTED"),
        }
    }
}

/// Trade record.
#[derive(Debug, Clone)]
pub struct Trade {
    pub symbol: Symbol,
    pub price: u32,
    pub quantity: u32,
    pub side: Side,
    pub timestamp: DateTime<Local>,
}

/// Position in a symbol.
#[derive(Debug, Clone, Default)]
pub struct Position {
    pub symbol: Symbol,
    pub quantity: i64,
    pub avg_price: f64,
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,
}

/// Order book state.
#[derive(Debug, Default)]
pub struct OrderBookState {
    pub bids: Vec<(u32, u32)>, // (price, quantity)
    pub asks: Vec<(u32, u32)>,
    pub last_update: Option<DateTime<Local>>,
}

/// Load test scenario configuration.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct LoadTestScenario {
    pub name: String,
    pub total_orders: u64,
    pub orders_per_second: Option<u64>, // None = unthrottled
    pub symbols: Vec<Symbol>,
    pub price_range: (u32, u32),
    pub qty_range: (u32, u32),
    pub buy_ratio: f64, // 0.5 = 50% buys
}

#[allow(dead_code)]
impl LoadTestScenario {
    /// Create predefined scenarios.
    pub fn presets() -> Vec<Self> {
        let symbols: Vec<Symbol> = ["IBM", "AAPL", "GOOG", "MSFT", "TSLA"]
            .iter()
            .map(|s| Symbol::from_str(s))
            .collect();

        vec![
            // Throttled scenarios
            Self {
                name: "1K @ 1K/s".to_string(),
                total_orders: 1_000,
                orders_per_second: Some(1_000),
                symbols: symbols.clone(),
                price_range: (9000, 11000),
                qty_range: (1, 100),
                buy_ratio: 0.5,
            },
            Self {
                name: "50K @ 10K/s".to_string(),
                total_orders: 50_000,
                orders_per_second: Some(10_000),
                symbols: symbols.clone(),
                price_range: (9000, 11000),
                qty_range: (1, 100),
                buy_ratio: 0.5,
            },
            Self {
                name: "100K @ 50K/s".to_string(),
                total_orders: 100_000,
                orders_per_second: Some(50_000),
                symbols: symbols.clone(),
                price_range: (9000, 11000),
                qty_range: (1, 100),
                buy_ratio: 0.5,
            },
            Self {
                name: "1M @ 100K/s".to_string(),
                total_orders: 1_000_000,
                orders_per_second: Some(100_000),
                symbols: symbols.clone(),
                price_range: (9000, 11000),
                qty_range: (1, 100),
                buy_ratio: 0.5,
            },
            // Unthrottled burst scenarios
            Self {
                name: "1K burst".to_string(),
                total_orders: 1_000,
                orders_per_second: None,
                symbols: symbols.clone(),
                price_range: (9000, 11000),
                qty_range: (1, 100),
                buy_ratio: 0.5,
            },
            Self {
                name: "100K burst".to_string(),
                total_orders: 100_000,
                orders_per_second: None,
                symbols: symbols.clone(),
                price_range: (9000, 11000),
                qty_range: (1, 100),
                buy_ratio: 0.5,
            },
            Self {
                name: "1M burst".to_string(),
                total_orders: 1_000_000,
                orders_per_second: None,
                symbols: symbols.clone(),
                price_range: (9000, 11000),
                qty_range: (1, 100),
                buy_ratio: 0.5,
            },
            // Extreme scenarios
            Self {
                name: "10M @ 500K/s".to_string(),
                total_orders: 10_000_000,
                orders_per_second: Some(500_000),
                symbols: symbols.clone(),
                price_range: (9000, 11000),
                qty_range: (1, 100),
                buy_ratio: 0.5,
            },
            Self {
                name: "100M @ 1M/s".to_string(),
                total_orders: 100_000_000,
                orders_per_second: Some(1_000_000),
                symbols,
                price_range: (9000, 11000),
                qty_range: (1, 100),
                buy_ratio: 0.5,
            },
        ]
    }
}

/// Load test statistics.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct LoadTestStats {
    pub orders_sent: u64,
    pub acks_received: u64,
    pub trades_received: u64,
    pub errors: u64,
    pub start_time: Option<std::time::Instant>,
    pub end_time: Option<std::time::Instant>,
    pub latency_histogram: Option<hdrhistogram::Histogram<u64>>,
}

#[allow(dead_code)]
impl LoadTestStats {
    pub fn new() -> Self {
        Self {
            latency_histogram: Some(
                hdrhistogram::Histogram::new_with_bounds(1, 60_000_000, 3).unwrap()
            ),
            ..Default::default()
        }
    }

    pub fn elapsed_secs(&self) -> f64 {
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => end.duration_since(start).as_secs_f64(),
            (Some(start), None) => start.elapsed().as_secs_f64(),
            _ => 0.0,
        }
    }

    pub fn orders_per_second(&self) -> f64 {
        let elapsed = self.elapsed_secs();
        if elapsed > 0.0 {
            self.orders_sent as f64 / elapsed
        } else {
            0.0
        }
    }

    pub fn record_latency(&mut self, latency_us: u64) {
        if let Some(ref mut hist) = self.latency_histogram {
            let _ = hist.record(latency_us);
        }
    }

    pub fn latency_percentile(&self, p: f64) -> u64 {
        self.latency_histogram
            .as_ref()
            .map(|h| h.value_at_percentile(p))
            .unwrap_or(0)
    }

    pub fn print_summary(&self) {
        println!("\n══════════════════════════════════════════════════════════");
        println!("                    LOAD TEST RESULTS                      ");
        println!("══════════════════════════════════════════════════════════");
        println!();
        println!("Orders:");
        println!("  Sent:     {:>12}", format_number(self.orders_sent));
        println!("  Acks:     {:>12}", format_number(self.acks_received));
        println!("  Trades:   {:>12}", format_number(self.trades_received));
        println!("  Errors:   {:>12}", format_number(self.errors));
        println!();
        println!("Performance:");
        println!("  Duration: {:>12.2} seconds", self.elapsed_secs());
        println!("  Rate:     {:>12.0} orders/sec", self.orders_per_second());
        println!();
        println!("Latency (microseconds):");
        println!("  p50:      {:>12} μs", self.latency_percentile(50.0));
        println!("  p90:      {:>12} μs", self.latency_percentile(90.0));
        println!("  p99:      {:>12} μs", self.latency_percentile(99.0));
        println!("  p99.9:    {:>12} μs", self.latency_percentile(99.9));
        println!("  max:      {:>12} μs", self.latency_percentile(100.0));
        println!("══════════════════════════════════════════════════════════");
    }
}

/// Format a number with K/M suffixes for readability.
#[allow(dead_code)]
fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
