// crates/engine-trading-client/src/components/trade_list.rs

use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
    Frame,
};

use crate::app::App;
use engine_core::Side;

pub fn draw_trade_list(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec!["Time", "Symbol", "Side", "Price", "Qty"])
        .style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app.recent_trades.iter().take(10).map(|trade| {
        let side_style = match trade.side {
            Side::Buy => Style::default().fg(Color::Green),
            Side::Sell => Style::default().fg(Color::Red),
        };

        Row::new(vec![
            Cell::from(trade.timestamp.format("%H:%M:%S").to_string()),
            Cell::from(trade.symbol.clone()),
            Cell::from(format!("{:?}", trade.side)).style(side_style),
            Cell::from(format!("{:.2}", trade.price as f64 / 100.0)),
            Cell::from(trade.quantity.to_string()),
        ])
    }).collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(6),
        Constraint::Length(8),
        Constraint::Min(6),
    ];

    let table = Table::new(rows, widths)  // Fixed: pass widths as second argument
        .header(header)
        .block(Block::default()
            .title(" Recent Trades ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(
                if matches!(app.current_panel, crate::app::Panel::Trades) {
                    Color::Yellow
                } else {
                    Color::White
                }
            )));

    f.render_widget(table, area);
}
