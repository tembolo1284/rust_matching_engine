// crates/engine-trading-client/src/main.rs

mod app;
mod ui;
mod components;
mod network;
mod types;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    Terminal,
};
use std::{io, time::Duration};
use tokio::sync::mpsc;
use tracing::{info, error};
use engine_core::{InputMessage, OutputMessage};

use crate::app::{App, InputMode};
use crate::network::EngineConnection;

#[derive(Parser)]
#[clap(name = "trading-client")]
#[clap(about = "Professional trading terminal for the matching engine")]
struct Cli {
    /// Server address
    #[clap(short, long, default_value = "127.0.0.1:9001")]
    server: String,

    /// User ID for trading
    #[clap(short, long, default_value = "1")]
    user_id: u32,

    /// Starting symbol to trade
    #[clap(short = 'y', long, default_value = "AAPL")]
    symbol: String,

    /// Enable debug logging
    #[clap(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    if cli.debug {
        tracing_subscriber::fmt::init();
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run
    let app = App::new(cli.user_id, &cli.symbol);
    let res = run_app(&mut terminal, app, &cli.server).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
    server_addr: &str,
) -> Result<()> {
    // Create channels for network communication
    let (tx_to_network, rx_from_app) = mpsc::unbounded_channel::<InputMessage>();
    let (tx_to_app, mut rx_from_network) = mpsc::unbounded_channel::<OutputMessage>();
    
    // Give app the sender to network
    app.set_network_sender(tx_to_network);
    
    // Create network connection
    let mut connection = EngineConnection::new(server_addr, tx_to_app);
    
    // Connect to server
    info!("Connecting to {}...", server_addr);
    connection.connect().await?;
    app.set_connected(true);
    
    // Spawn network handler
    let network_handle = tokio::spawn(async move {
        connection.run(rx_from_app).await;
    });

    loop {
        // Draw UI
        terminal.draw(|f| ui::draw(f, &app))?;

        // Handle events with timeout
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match app.input_mode {
                    InputMode::Normal => match key.code {
                        // Global hotkeys
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            app.should_quit = true;
                        }
                        KeyCode::Tab => {
                            app.next_panel();
                        }
                        KeyCode::BackTab => {
                            app.prev_panel();
                        }
                        
                        // Order entry hotkeys
                        KeyCode::Char('b') | KeyCode::Char('B') => {
                            app.start_order_entry(engine_core::Side::Buy);
                        }
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            app.start_order_entry(engine_core::Side::Sell);
                        }
                        KeyCode::Char('m') | KeyCode::Char('M') => {
                            app.toggle_market_order();
                        }
                        KeyCode::Char('c') | KeyCode::Char('C') => {
                            app.cancel_selected_order();
                        }
                        KeyCode::Char('x') | KeyCode::Char('X') => {
                            app.cancel_all_orders();
                        }
                        
                        // Navigation
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.move_selection_up();
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.move_selection_down();
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            app.move_selection_left();
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            app.move_selection_right();
                        }
                        
                        // Symbol switching
                        KeyCode::Char('/') => {
                            app.start_symbol_search();
                        }
                        
                        // View toggles
                        KeyCode::F(1) => {
                            app.toggle_help();
                        }
                        KeyCode::F(2) => {
                            app.toggle_chart();
                        }
                        KeyCode::F(3) => {
                            app.toggle_depth();
                        }
                        
                        _ => {}
                    },
                    
                    InputMode::Editing => match key.code {
                        KeyCode::Enter => {
                            app.submit_input();
                        }
                        KeyCode::Esc => {
                            app.cancel_input();
                        }
                        KeyCode::Backspace => {
                            app.delete_char();
                        }
                        KeyCode::Char(c) => {
                            app.enter_char(c);
                        }
                        _ => {}
                    },
                }
            }
        }

        // Process network messages - FIX: use rx_from_network instead of rx
        while let Ok(msg) = rx_from_network.try_recv() {
            app.handle_engine_message(msg);
        }

        // Check if should quit
        if app.should_quit {
            break;
        }
    }

    // Cleanup
    network_handle.abort();
    Ok(())
}
