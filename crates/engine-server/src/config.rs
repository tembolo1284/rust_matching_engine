//! Server configuration.

use std::env;
use std::net::Ipv4Addr;
use std::time::Duration;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct Config {
    // === TCP Settings ===
    pub tcp_bind_addr: String,
    pub tcp_port: u16,
    pub tcp_enabled: bool,

    // === UDP Settings ===
    pub udp_bind_addr: String,
    pub udp_port: u16,
    pub udp_enabled: bool,

    // === Multicast Settings ===
    pub multicast_group: Ipv4Addr,
    pub multicast_port: u16,
    pub multicast_interface: Ipv4Addr,
    pub multicast_enabled: bool,
    pub multicast_ttl: u32,

    // === FIX Settings ===
    pub fix_port: u16,
    pub fix_enabled: bool,
    pub fix_sender_comp_id: String,
    pub fix_target_comp_id: String,

    // === Connection Limits ===
    pub max_tcp_clients: usize,
    pub max_udp_clients: usize,

    // === Timeouts ===
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub idle_timeout: Duration,

    // === Channel Capacities (bounded!) ===
    pub engine_channel_capacity: usize,
    pub client_channel_capacity: usize,
    pub multicast_channel_capacity: usize,

    // === Buffer Sizes ===
    pub tcp_read_buffer_size: usize,
    pub udp_buffer_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // TCP
            tcp_bind_addr: "0.0.0.0".to_string(),
            tcp_port: 1234,
            tcp_enabled: true,

            // UDP
            udp_bind_addr: "0.0.0.0".to_string(),
            udp_port: 1235,
            udp_enabled: true,

            // Multicast
            multicast_group: Ipv4Addr::new(239, 255, 0, 1),
            multicast_port: 1236,
            multicast_interface: Ipv4Addr::UNSPECIFIED,
            multicast_enabled: true,
            multicast_ttl: 1,

            // FIX
            fix_port: 9003,
            fix_enabled: false,
            fix_sender_comp_id: "ENGINE".to_string(),
            fix_target_comp_id: "CLIENT".to_string(),

            // Limits
            max_tcp_clients: 1024,
            max_udp_clients: 4096,

            // Timeouts
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(300),

            // Channels (BOUNDED for backpressure)
            engine_channel_capacity: 100_000,
            client_channel_capacity: 10_000,
            multicast_channel_capacity: 50_000,

            // Buffers
            tcp_read_buffer_size: 8192,
            udp_buffer_size: 65536,
        }
    }
}

impl Config {
    /// Load from environment variables.
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let mut cfg = Config::default();

        // TCP
        if let Ok(v) = env::var("ENGINE_TCP_ADDR") {
            cfg.tcp_bind_addr = v;
        }
        if let Ok(v) = env::var("ENGINE_TCP_PORT") {
            cfg.tcp_port = v.parse()?;
        }
        if let Ok(v) = env::var("ENGINE_TCP_ENABLED") {
            cfg.tcp_enabled = parse_bool(&v);
        }

        // UDP
        if let Ok(v) = env::var("ENGINE_UDP_ADDR") {
            cfg.udp_bind_addr = v;
        }
        if let Ok(v) = env::var("ENGINE_UDP_PORT") {
            cfg.udp_port = v.parse()?;
        }
        if let Ok(v) = env::var("ENGINE_UDP_ENABLED") {
            cfg.udp_enabled = parse_bool(&v);
        }

        // Multicast
        if let Ok(v) = env::var("ENGINE_MCAST_GROUP") {
            cfg.multicast_group = v.parse()?;
        }
        if let Ok(v) = env::var("ENGINE_MCAST_PORT") {
            cfg.multicast_port = v.parse()?;
        }
        if let Ok(v) = env::var("ENGINE_MCAST_ENABLED") {
            cfg.multicast_enabled = parse_bool(&v);
        }

        // FIX
        if let Ok(v) = env::var("ENGINE_FIX_PORT") {
            cfg.fix_port = v.parse()?;
        }
        if let Ok(v) = env::var("ENGINE_FIX_ENABLED") {
            cfg.fix_enabled = parse_bool(&v);
        }

        // Limits
        if let Ok(v) = env::var("ENGINE_MAX_TCP_CLIENTS") {
            cfg.max_tcp_clients = v.parse()?;
        }
        if let Ok(v) = env::var("ENGINE_MAX_UDP_CLIENTS") {
            cfg.max_udp_clients = v.parse()?;
        }

        // Channels
        if let Ok(v) = env::var("ENGINE_CHANNEL_CAPACITY") {
            cfg.engine_channel_capacity = v.parse()?;
        }

        Ok(cfg)
    }

    /// Load from env + CLI args.
    pub fn from_env_and_args() -> Result<Self, Box<dyn std::error::Error>> {
        let mut cfg = Self::from_env()?;
        let mut args = env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--tcp-port" => {
                    cfg.tcp_port = args.next().ok_or("Missing --tcp-port value")?.parse()?;
                }
                "--udp-port" => {
                    cfg.udp_port = args.next().ok_or("Missing --udp-port value")?.parse()?;
                }
                "--mcast-port" => {
                    cfg.multicast_port = args.next().ok_or("Missing --mcast-port value")?.parse()?;
                }
                "--no-tcp" => cfg.tcp_enabled = false,
                "--no-udp" => cfg.udp_enabled = false,
                "--no-mcast" => cfg.multicast_enabled = false,
                "--fix" => cfg.fix_enabled = true,
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                _ => {}
            }
        }

        Ok(cfg)
    }

    /// TCP socket address string.
    pub fn tcp_addr(&self) -> String {
        format!("{}:{}", self.tcp_bind_addr, self.tcp_port)
    }

    /// UDP socket address string.
    pub fn udp_addr(&self) -> String {
        format!("{}:{}", self.udp_bind_addr, self.udp_port)
    }
}

fn parse_bool(s: &str) -> bool {
    matches!(s.to_lowercase().as_str(), "true" | "1" | "yes")
}

fn print_help() {
    eprintln!("engine-server - Multi-protocol matching engine server");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("  --tcp-port PORT    TCP port (default: 9000)");
    eprintln!("  --udp-port PORT    UDP port (default: 9001)");
    eprintln!("  --mcast-port PORT  Multicast port (default: 9002)");
    eprintln!("  --no-tcp           Disable TCP server");
    eprintln!("  --no-udp           Disable UDP server");
    eprintln!("  --no-mcast         Disable multicast publisher");
    eprintln!("  --fix              Enable FIX gateway");
    eprintln!();
    eprintln!("ENVIRONMENT:");
    eprintln!("  ENGINE_TCP_PORT, ENGINE_UDP_PORT, ENGINE_MCAST_PORT");
    eprintln!("  ENGINE_TCP_ENABLED, ENGINE_UDP_ENABLED, ENGINE_MCAST_ENABLED");
}
