//! Load testing module.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use engine_core::{InputMessage, NewOrder, Side, Symbol};
use rand::prelude::*;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::RwLock;
use tokio::time::sleep;

use crate::network::{EngineConnection, NetworkEvent};
use crate::types::{LoadTestScenario, LoadTestStats, Protocol, Transport};

/// Run a load test scenario.
pub async fn run_load_test(
    server_addr: &str,
    transport: Transport,
    protocol: Protocol,
    scenario: LoadTestScenario,
    user_id: u32,
) -> Result<LoadTestStats> {
    println!("\n══════════════════════════════════════════════════════════");
    println!("  LOAD TEST: {}", scenario.name);
    println!("══════════════════════════════════════════════════════════");
    println!("  Server:     {}", server_addr);
    println!("  Transport:  {:?}", transport);
    println!("  Protocol:   {:?}", protocol);
    println!("  Orders:     {}", scenario.total_orders);
    if let Some(rate) = scenario.orders_per_second {
        println!("  Rate:       {}/sec (throttled)", rate);
    } else {
        println!("  Rate:       Unthrottled (burst)");
    }
    println!("  Symbols:    {:?}", scenario.symbols.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    println!("══════════════════════════════════════════════════════════\n");

    // Channels
    let (msg_tx, msg_rx) = mpsc::channel::<InputMessage>(100_000);
    let (event_tx, mut event_rx) = mpsc::channel::<NetworkEvent>(100_000);

    // Stats
    let stats = Arc::new(RwLock::new(LoadTestStats::new()));

    // Connect
    let mut conn = EngineConnection::new(server_addr, transport, protocol, event_tx);
    conn.connect().await?;

    // Spawn network handler
    let network_handle = tokio::spawn(async move {
        conn.run(msg_rx).await;
    });

    // Spawn stats collector
    let stats_clone = stats.clone();
    let collector_handle = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let mut s = stats_clone.write().await;
            match event {
                NetworkEvent::Message(msg) => {
                    match msg {
                        engine_core::OutputMessage::Ack(_) => s.acks_received += 1,
                        engine_core::OutputMessage::Trade(_) => s.trades_received += 1,
                        _ => {}
                    }
                }
                NetworkEvent::LatencySample { latency_us, .. } => {
                    s.record_latency(latency_us);
                }
                NetworkEvent::Error(_) => s.errors += 1,
                _ => {}
            }
        }
    });

    // Generate and send orders
    {
        let mut s = stats.write().await;
        s.start_time = Some(Instant::now());
    }

    let mut rng = rand::thread_rng();
    let mut order_id: u32 = 1;

    // Calculate delay between orders for throttling
    let delay = scenario.orders_per_second.map(|rate| Duration::from_nanos(1_000_000_000 / rate));

    // Progress tracking
    let progress_interval = (scenario.total_orders / 10).max(1000);
    let start = Instant::now();

    for i in 0..scenario.total_orders {
        // Generate random order
        let symbol = scenario.symbols.choose(&mut rng).unwrap().clone();
        let price = rng.gen_range(scenario.price_range.0..=scenario.price_range.1);
        let quantity = rng.gen_range(scenario.qty_range.0..=scenario.qty_range.1);
        let side = if rng.gen::<f64>() < scenario.buy_ratio {
            Side::Buy
        } else {
            Side::Sell
        };

        let order = InputMessage::NewOrder(NewOrder::new(
            user_id,
            order_id,
            symbol,
            price,
            quantity,
            side,
        ));

        // Send order
        if msg_tx.send(order).await.is_err() {
            break;
        }

        {
            let mut s = stats.write().await;
            s.orders_sent += 1;
        }

        order_id += 1;

        // Throttle if needed
        if let Some(d) = delay {
            sleep(d).await;
        }

        // Print progress
        if i > 0 && i % progress_interval == 0 {
            let elapsed = start.elapsed().as_secs_f64();
            let rate = i as f64 / elapsed;
            print!(
                "\r  Progress: {:>6.1}% ({:>10} orders, {:>10.0} orders/sec)",
                (i as f64 / scenario.total_orders as f64) * 100.0,
                i,
                rate
            );
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
    }

    // Wait for remaining acks (with timeout)
    println!("\n  Waiting for remaining responses...");
    sleep(Duration::from_secs(2)).await;

    {
        let mut s = stats.write().await;
        s.end_time = Some(Instant::now());
    }

    // Cleanup
    network_handle.abort();
    collector_handle.abort();

    // Return stats
    let final_stats = stats.read().await.clone();
    Ok(final_stats)
}

/// Interactive load test menu.
pub async fn interactive_menu(
    server_addr: &str,
    transport: Transport,
    protocol: Protocol,
    user_id: u32,
) -> Result<()> {
    let scenarios = LoadTestScenario::presets();

    loop {
        println!("\n╔══════════════════════════════════════════════════════════╗");
        println!("║                    LOAD TEST MENU                        ║");
        println!("╠══════════════════════════════════════════════════════════╣");
        
        for (i, s) in scenarios.iter().enumerate() {
            let throttle = if s.orders_per_second.is_some() { "throttled" } else { "burst" };
            println!("║  {:>2}. {:40} ({:>9}) ║", i + 1, s.name, throttle);
        }
        
        println!("║   0. Exit                                                ║");
        println!("╚══════════════════════════════════════════════════════════╝");
        
        print!("\nSelect scenario (0-{}): ", scenarios.len());
        use std::io::Write;
        let _ = std::io::stdout().flush();

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        
        let choice: usize = match input.trim().parse() {
            Ok(n) => n,
            Err(_) => continue,
        };

        if choice == 0 {
            break;
        }

        if choice > scenarios.len() {
            println!("Invalid choice");
            continue;
        }

        let scenario = scenarios[choice - 1].clone();
        
        match run_load_test(server_addr, transport, protocol, scenario, user_id).await {
            Ok(stats) => {
                stats.print_summary();
            }
            Err(e) => {
                println!("Load test failed: {}", e);
            }
        }

        println!("\nPress Enter to continue...");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
    }

    Ok(())
}

// Re-export LoadTestStats clone for convenience
impl Clone for LoadTestStats {
    fn clone(&self) -> Self {
        Self {
            orders_sent: self.orders_sent,
            acks_received: self.acks_received,
            trades_received: self.trades_received,
            errors: self.errors,
            start_time: self.start_time,
            end_time: self.end_time,
            latency_histogram: self.latency_histogram.as_ref().map(|h| h.clone()),
        }
    }
}
