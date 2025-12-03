//! Server metrics and statistics.

use std::sync::atomic::{AtomicU64, Ordering};

/// Server-wide metrics.
#[derive(Debug, Default)]
pub struct Metrics {
    // Connection metrics
    pub tcp_connections_total: AtomicU64,
    pub tcp_connections_active: AtomicU64,
    pub udp_clients_active: AtomicU64,

    // Message metrics
    pub messages_received: AtomicU64,
    pub messages_processed: AtomicU64,
    pub messages_sent: AtomicU64,
    pub multicast_messages: AtomicU64,

    // Error metrics
    pub decode_errors: AtomicU64,
    pub send_errors: AtomicU64,
    pub channel_full_drops: AtomicU64,

    // Engine metrics
    pub orders_received: AtomicU64,
    pub trades_executed: AtomicU64,
    pub cancels_received: AtomicU64,
}

impl Metrics {
    /// Create new metrics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment a counter.
    #[inline]
    pub fn inc(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Add to a counter.
    #[inline]
    pub fn add(counter: &AtomicU64, value: u64) {
        counter.fetch_add(value, Ordering::Relaxed);
    }

    /// Decrement a counter.
    #[inline]
    pub fn dec(counter: &AtomicU64) {
        counter.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get counter value.
    #[inline]
    pub fn get(counter: &AtomicU64) -> u64 {
        counter.load(Ordering::Relaxed)
    }

    /// Print summary statistics.
    pub fn print_summary(&self) {
        eprintln!("==============================================================");
        eprintln!("Server Metrics Summary");
        eprintln!("==============================================================");
        eprintln!("Connections:");
        eprintln!("  TCP total:      {}", Self::get(&self.tcp_connections_total));
        eprintln!("  TCP active:     {}", Self::get(&self.tcp_connections_active));
        eprintln!("  UDP clients:    {}", Self::get(&self.udp_clients_active));
        eprintln!();
        eprintln!("Messages:");
        eprintln!("  Received:       {}", Self::get(&self.messages_received));
        eprintln!("  Processed:      {}", Self::get(&self.messages_processed));
        eprintln!("  Sent:           {}", Self::get(&self.messages_sent));
        eprintln!("  Multicast:      {}", Self::get(&self.multicast_messages));
        eprintln!();
        eprintln!("Trading:");
        eprintln!("  Orders:         {}", Self::get(&self.orders_received));
        eprintln!("  Trades:         {}", Self::get(&self.trades_executed));
        eprintln!("  Cancels:        {}", Self::get(&self.cancels_received));
        eprintln!();
        eprintln!("Errors:");
        eprintln!("  Decode errors:  {}", Self::get(&self.decode_errors));
        eprintln!("  Send errors:    {}", Self::get(&self.send_errors));
        eprintln!("  Channel drops:  {}", Self::get(&self.channel_full_drops));
        eprintln!("==============================================================");
    }
}
