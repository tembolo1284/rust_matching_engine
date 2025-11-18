// crates/engine-trading-client/src/components/positions.rs

use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
    Frame,
};

use crate::app::App;

pub fn draw_positions(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec!["Symbol", "Qty", "Avg Price", "P&L"])
        .style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app.positions.values().map(|pos| {
        let qty_style = if pos.quantity > 0 {
            Style::default().fg(Color::Green)
        } else if pos.quantity < 0 {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Gray)
        };

        let pnl = pos.realized_pnl + pos.unrealized_pnl;
        let pnl_style = if pnl > 0.0 {
            Style::default().fg(Color::Green)
        } else if pnl < 0.0 {
            Style::default().fg(Color::Red)
        } else {
            Style::default()
        };

        Row::new(vec![
            Cell::from(pos.symbol.clone()),
            Cell::from(pos.quantity.to_string()).style(qty_style),
            Cell::from(format!("{:.2}", pos.avg_price)),
            Cell::from(format!("{:+.2}", pnl)).style(pnl_style),
        ])
    }).collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Min(10),
    ];

    let table = Table::new(rows, widths)  // Fixed: pass widths as second argument
        .header(header)
        .block(Block::default()
            .title(" Positions ")
            .borders(Borders::ALL));

    f.render_widget(table, area);
}
