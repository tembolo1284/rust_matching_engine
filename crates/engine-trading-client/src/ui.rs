// crates/engine-trading-client/src/ui.rs

use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::{App, Panel};
use crate::components::{
    order_book::draw_order_book,
    order_entry::draw_order_entry,
    order_list::draw_order_list,
    trade_list::draw_trade_list,
    positions::draw_positions,
    status_bar::draw_status_bar,
    help::draw_help,
};

pub fn draw<B: Backend>(f: &mut Frame<B>, app: &App) {
    // Main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // Header
            Constraint::Min(10),     // Main content
            Constraint::Length(3),   // Status bar
        ])
        .split(f.size());

    // Draw header
    draw_header(f, chunks[0], app);

    // Draw main content area
    draw_main_content(f, chunks[1], app);

    // Draw status bar
    draw_status_bar(f, chunks[2], app);

    // Draw help overlay if active
    if app.show_help {
        draw_help(f, centered_rect(60, 60, f.size()));
    }
}

fn draw_header<B: Backend>(f: &mut Frame<B>, area: Rect, app: &App) {
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(area);

    // Left: Symbol and connection status
    let connection_symbol = if app.connected { "✓" } else { "✗" };
    let connection_color = if app.connected { Color::Green } else { Color::Red };
    
    let left_text = vec![
        Span::styled(&app.current_symbol, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" - "),
        Span::raw("Connected "),
        Span::styled(connection_symbol, Style::default().fg(connection_color)),
    ];
    
    let left_paragraph = Paragraph::new(Line::from(left_text))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(left_paragraph, header_chunks[0]);

    // Center: Market data
    let center_text = format!(
        "Trades: {} | Volume: {} | Msgs: {}",
        app.total_trades,
        format_volume(app.total_volume),
        app.message_count
    );
    let center_paragraph = Paragraph::new(center_text)
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(center_paragraph, header_chunks[1]);

    // Right: Help hints
    let help_text = "[F1]Help [Tab]Panel [/]Search";
    let right_paragraph = Paragraph::new(help_text)
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(right_paragraph, header_chunks[2]);
}

fn draw_main_content<B: Backend>(f: &mut Frame<B>, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),  // Left panel
            Constraint::Percentage(30),  // Center panel
            Constraint::Percentage(30),  // Right panel
        ])
        .split(area);

    // Left panel - Order Book
    draw_order_book(f, chunks[0], app);

    // Center panel - My Orders or Order Entry
    match app.current_panel {
        Panel::OrderEntry => draw_order_entry(f, chunks[1], app),
        _ => draw_order_list(f, chunks[1], app),
    }

    // Right panel - Split between Trades and Positions
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(chunks[2]);

    draw_trade_list(f, right_chunks[0], app);
    draw_positions(f, right_chunks[1], app);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn format_volume(volume: u64) -> String {
    if volume >= 1_000_000 {
        format!("{:.1}M", volume as f64 / 1_000_000.0)
    } else if volume >= 1_000 {
        format!("{:.1}K", volume as f64 / 1_000.0)
    } else {
        volume.to_string()
    }
}
