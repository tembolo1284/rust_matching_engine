//! Standalone load testing binary.

use anyhow::Result;
use clap::Parser;
use engine_trading_client::{
    discovery::discover_and_print,
    load_test::{interactive_menu, run_load_test},
    types::LoadTestScenario,
};

#[derive(Parser)]
#[clap(name = "load-test")]
#[clap(about = "Load testing tool for the matching engine")]
struct Cli {
    /// Server address
    #[clap(short, long, default_value = "127.0.0.1:9000")]
    server: String,

    /// User ID for orders
    #[clap(short, long, default_value = "1")]
    user_id: u32,

    /// Run specific scenario by name (e.g., "1K burst", "1M @ 100K/s")
    #[clap(short = 'n', long)]
    scenario: Option<String>,

    /// List available scenarios
    #[clap(short, long)]
    list: bool,

    /// Interactive mode
    #[clap(short, long)]
    interactive: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // List scenarios
    if cli.list {
        println!("\nAvailable Load Test Scenarios:");
        println!("══════════════════════════════════════════════════════════");
        for s in LoadTestScenario::presets() {
            let throttle = if let Some(rate) = s.orders_per_second {
                format!("{}/s", rate)
            } else {
                "burst".to_string()
            };
            println!("  {:30} {:>12} orders @ {:>12}", s.name, s.total_orders, throttle);
        }
        return Ok(());
    }

    // Discover server
    let caps = discover_and_print(&cli.server).await?;

    if cli.interactive {
        // Interactive menu
        interactive_menu(&cli.server, caps.transport, caps.protocol, cli.user_id).await?;
    } else if let Some(scenario_name) = cli.scenario {
        // Run specific scenario
        let scenarios = LoadTestScenario::presets();
        let scenario = scenarios
            .into_iter()
            .find(|s| s.name.to_lowercase() == scenario_name.to_lowercase())
            .ok_or_else(|| anyhow::anyhow!("Scenario '{}' not found", scenario_name))?;

        let stats = run_load_test(
            &cli.server,
            caps.transport,
            caps.protocol,
            scenario,
            cli.user_id,
        )
        .await?;

        stats.print_summary();
    } else {
        // Default: run smallest scenario as demo
        let scenario = LoadTestScenario::presets().remove(0);
        let stats = run_load_test(
            &cli.server,
            caps.transport,
            caps.protocol,
            scenario,
            cli.user_id,
        )
        .await?;

        stats.print_summary();
    }

    Ok(())
}
