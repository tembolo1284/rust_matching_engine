//! Main server orchestration.

use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::mpsc;
use tokio::time;

use crate::client_tcp::handle_tcp_client;
use crate::client_udp::run_udp_server;
use crate::config::Config;
use crate::engine_task::run_engine_loop;
use crate::metrics::Metrics;
use crate::multicast::run_multicast_publisher;
use crate::types::{ClientId, ClientRegistry, EngineRequest};

/// Maximum number of port retries before giving up.
const MAX_PORT_RETRIES: u16 = 10;

/// Bind a TCP listener with automatic port retry on AddrInUse.
async fn bind_tcp_with_retry(
    bind_addr: &str,
    base_port: u16,
) -> Result<(TcpListener, u16), std::io::Error> {
    for offset in 0..MAX_PORT_RETRIES {
        let port = base_port + offset;
        let addr = format!("{}:{}", bind_addr, port);

        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                if offset > 0 {
                    eprintln!(
                        "Note: TCP port {} was in use, bound to {} instead",
                        base_port, port
                    );
                }
                return Ok((listener, port));
            }
            Err(e) if e.kind() == ErrorKind::AddrInUse => {
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(std::io::Error::new(
        ErrorKind::AddrInUse,
        format!(
            "Could not bind TCP after {} retries (ports {}-{})",
            MAX_PORT_RETRIES,
            base_port,
            base_port + MAX_PORT_RETRIES - 1
        ),
    ))
}

/// Bind a UDP socket with automatic port retry on AddrInUse.
pub async fn bind_udp_with_retry(
    bind_addr: &str,
    base_port: u16,
) -> Result<(UdpSocket, u16), std::io::Error> {
    for offset in 0..MAX_PORT_RETRIES {
        let port = base_port + offset;
        let addr = format!("{}:{}", bind_addr, port);

        match UdpSocket::bind(&addr).await {
            Ok(socket) => {
                if offset > 0 {
                    eprintln!(
                        "Note: UDP port {} was in use, bound to {} instead",
                        base_port, port
                    );
                }
                return Ok((socket, port));
            }
            Err(e) if e.kind() == ErrorKind::AddrInUse => {
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(std::io::Error::new(
        ErrorKind::AddrInUse,
        format!(
            "Could not bind UDP after {} retries (ports {}-{})",
            MAX_PORT_RETRIES,
            base_port,
            base_port + MAX_PORT_RETRIES - 1
        ),
    ))
}

/// Run the server with the given configuration.
pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let config = Arc::new(config);
    let metrics = Arc::new(Metrics::new());
    let clients = Arc::new(ClientRegistry::new());

    // Create bounded engine channel
    let (engine_tx, engine_rx) = mpsc::channel::<EngineRequest>(config.engine_channel_capacity);

    // Create multicast channel if enabled
    let multicast_tx = if config.multicast_enabled {
        let (tx, rx) = mpsc::channel(config.multicast_channel_capacity);

        // Spawn multicast publisher
        let mcast_config = config.clone();
        let mcast_metrics = metrics.clone();
        tokio::spawn(async move {
            if let Err(e) = run_multicast_publisher(mcast_config, rx, mcast_metrics).await {
                eprintln!("Multicast publisher error: {}", e);
            }
        });

        Some(tx)
    } else {
        None
    };

    // Spawn engine task
    {
        let clients_clone = clients.clone();
        let metrics_clone = metrics.clone();
        tokio::spawn(async move {
            run_engine_loop(engine_rx, clients_clone, multicast_tx, metrics_clone).await;
        });
    }

    // Spawn UDP server if enabled
    if config.udp_enabled {
        let udp_config = config.clone();
        let udp_clients = clients.clone();
        let udp_engine_tx = engine_tx.clone();
        let udp_metrics = metrics.clone();

        tokio::spawn(async move {
            if let Err(e) = run_udp_server(udp_config, udp_clients, udp_engine_tx, udp_metrics).await {
                eprintln!("UDP server error: {}", e);
            }
        });
    }

    // Run TCP server if enabled (this blocks until shutdown)
    if config.tcp_enabled {
        // Print banner before starting TCP (so we show actual ports)
        print_banner(&config);
        run_tcp_server(config.clone(), clients.clone(), engine_tx.clone(), metrics.clone()).await?;
    } else {
        print_banner(&config);
        // Just wait for shutdown signal
        tokio::signal::ctrl_c().await?;
    }

    // Shutdown
    eprintln!("\n==============================================================");
    eprintln!("Shutting down...");
    eprintln!("==============================================================");

    // Print metrics
    metrics.print_summary();

    // Give tasks time to finish
    time::sleep(Duration::from_millis(100)).await;

    eprintln!("Goodbye!");
    Ok(())
}

async fn run_tcp_server(
    config: Arc<Config>,
    clients: Arc<ClientRegistry>,
    engine_tx: mpsc::Sender<EngineRequest>,
    metrics: Arc<Metrics>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (listener, actual_port) =
        bind_tcp_with_retry(&config.tcp_bind_addr, config.tcp_port).await?;

    eprintln!("TCP server listening on {}:{}", config.tcp_bind_addr, actual_port);

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, peer_addr)) => {
                        let current_count = clients.client_count().await;

                        if current_count >= config.max_tcp_clients {
                            eprintln!("Rejecting {}: max clients reached", peer_addr);
                            continue;
                        }

                        let client_id = ClientId::next();
                        let cfg = config.clone();
                        let cli = clients.clone();
                        let eng = engine_tx.clone();
                        let met = metrics.clone();

                        tokio::spawn(async move {
                            handle_tcp_client(client_id, stream, cfg, cli, eng, met).await;
                        });
                    }
                    Err(e) => {
                        eprintln!("Accept error: {}", e);
                        time::sleep(Duration::from_millis(10)).await;
                    }
                }
            }

            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }
    }

    Ok(())
}

fn print_banner(config: &Config) {
    eprintln!("==============================================================");
    eprintln!("         Matching Engine Server v0.2.0");
    eprintln!("==============================================================");
    eprintln!();
    eprintln!("Transports:");
    if config.tcp_enabled {
        eprintln!("  TCP:       {} (CSV, Binary, FIX)", config.tcp_addr());
    }
    if config.udp_enabled {
        eprintln!("  UDP:       {} (CSV, Binary)", config.udp_addr());
    }
    if config.multicast_enabled {
        eprintln!("  Multicast: {}:{} (Binary)", config.multicast_group, config.multicast_port);
    }
    eprintln!();
    eprintln!("Limits:");
    eprintln!("  Max TCP clients:    {}", config.max_tcp_clients);
    eprintln!("  Engine queue:       {}", config.engine_channel_capacity);
    eprintln!("  Client queue:       {}", config.client_channel_capacity);
    eprintln!();
    eprintln!("==============================================================");
    eprintln!("Ready. Press Ctrl+C to shutdown.");
    eprintln!("==============================================================");
}
