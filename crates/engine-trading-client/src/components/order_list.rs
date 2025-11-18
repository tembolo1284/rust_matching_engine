// crates/engine-trading-client/src/components/order_list.rs

use ratatui::{
    backend::Backend,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
    Frame,
};

use crate::app::{App, OrderStatus};
use engine_core::Side;

pub fn draw_order_list<B: Backend>(f: &mut Frame<B>, area: Rect, app: &App) {
    let header = Row::new(vec!["ID", "Side", "Price", "Qty", "Filled", "Status"])
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
            Cell::from(order.order_id.to_string()).style(style),
            Cell::from(format!("{:?}", order.side)).style(side_style),
            Cell::from(format_price(order.price)).style(style),
            Cell::from(order.quantity.to_string()).style(style),
            Cell::from(order.filled_qty.to_string()).style(style),
            Cell::from(format!("{:?}", order.status)).style(status_style),
        ])
    }).collect();

    let table = Table::new(rows)
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
            )))
        .widths(&[
            ratatui::layout::Constraint::Length(8),
            ratatui::layout::Constraint::Length(6),
            ratatui::layout::Constraint::Length(8),
            ratatui::layout::Constraint::Length(6),
            ratatui::layout::Constraint::Length(6),
            ratatui::layout::Constraint::Min(10),
        ]);

    f.render_widget(table, area);
}

fn format_price(price: u32) -> String {
    if price == 0 {
        "MARKET".to_string()
    } else {
        format!("{:.2}", price as f64 / 100.0)
    }
}
