// crates/engine-trading-client/src/components/order_list.rs

use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
    Frame,
};

use crate::app::{App, OrderStatus};
use engine_core::Side;

pub fn draw_order_list(f: &mut Frame, area: Rect, app: &App) {
    // Updated header to include Time and Symbol
    let header = Row::new(vec!["Time", "ID", "Sym", "Side", "Price", "Qty", "Fill", "Status"])
        .style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app.my_orders.values().enumerate().map(|(i, order)| {
        let style = if i == app.selected_order_index {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };

        let side_style = match order.side {
            Side::Buy => style.fg(Color::Green),
            Side::Sell => style.fg(Color::Red),
        };

        let status_style = match order.status {
            OrderStatus::Pending => style.fg(Color::Yellow),
            OrderStatus::Open => style.fg(Color::Blue),
            OrderStatus::PartiallyFilled => style.fg(Color::Cyan),
            OrderStatus::Filled => style.fg(Color::Green),
            OrderStatus::Cancelled => style.fg(Color::DarkGray),
        };

        Row::new(vec![
            Cell::from(order.timestamp.format("%H:%M:%S").to_string()).style(style),  // Added timestamp
            Cell::from(order.order_id.to_string()).style(style),
            Cell::from(order.symbol.clone()).style(style),  // Added symbol
            Cell::from(format!("{:?}", order.side)).style(side_style),
            Cell::from(format_price(order.price)).style(style),
            Cell::from(order.quantity.to_string()).style(style),
            Cell::from(order.filled_qty.to_string()).style(style),
            Cell::from(format!("{:?}", order.status)).style(status_style),
        ])
    }).collect();

    let widths = [
        Constraint::Length(8),   // Time
        Constraint::Length(6),   // ID
        Constraint::Length(6),   // Symbol
        Constraint::Length(5),   // Side
        Constraint::Length(7),   // Price
        Constraint::Length(5),   // Qty
        Constraint::Length(5),   // Filled
        Constraint::Min(8),      // Status
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default()
            .title(" My Orders ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(
                if matches!(app.current_panel, crate::app::Panel::Orders) {
                    Color::Yellow
                } else {
                    Color::White
                }
            )));

    f.render_widget(table, area);
}

fn format_price(price: u32) -> String {
    if price == 0 {
        "MARKET".to_string()
    } else {
        format!("{:.2}", price as f64 / 100.0)
    }
}
