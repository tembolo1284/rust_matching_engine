// crates/engine-trading-client/src/types.rs

use serde::{Deserialize, Serialize};

/// Configuration for the trading client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server_addr: String,
    pub user_id: u32,
    pub default_symbol: String,
    pub default_quantity: u32,
    pub enable_sound: bool,
    pub theme: Theme,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_addr: "127.0.0.1:9001".to_string(),
            user_id: 1,
            default_symbol: "AAPL".to_string(),
            default_quantity: 100,
            enable_sound: false,
            theme: Theme::Dark,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Theme {
    Dark,
    Light,
    Blue,
}

/// Market data snapshot
#[derive(Debug, Clone)]
pub struct MarketData {
    pub symbol: String,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub last: Option<f64>,
    pub volume: u64,
    pub high: f64,
    pub low: f64,
    pub open: f64,
}

/// Alert configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: u32,
    pub symbol: String,
    pub condition: AlertCondition,
    pub triggered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertCondition {
    PriceAbove(f64),
    PriceBelow(f64),
    VolumeAbove(u64),
    Executed,
}
