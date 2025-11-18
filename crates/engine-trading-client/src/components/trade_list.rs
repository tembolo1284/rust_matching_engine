// crates/engine-trading-client/src/components/trade_list.rs

use ratatui::{
    backend::Backend,
    layout::Rect,
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

    let table = Table::new(rows)
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
            )))
        .widths(&[
            ratatui::layout::Constraint::Length(10),
            ratatui::layout::Constraint::Length(8),
            ratatui::layout::Constraint::Length(6),
            ratatui::layout::Constraint::Length(8),
            ratatui::layout::Constraint::Min(6),
        ]);

    f.render_widget(table, area);
}
