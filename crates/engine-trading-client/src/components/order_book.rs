// crates/engine-trading-client/src/components/order_book.rs

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

use crate::app::App;

pub fn draw_order_book(f: &mut Frame, area: Rect, app: &App) {
    let book = app.order_books.get(&app.current_symbol);
    
    let block = Block::default()
        .title(" Order Book ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(
            if matches!(app.current_panel, crate::app::Panel::OrderBook) {
                Color::Yellow
            } else {
                Color::White
            }
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(book) = book {
        // Split into bid and ask sides
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(inner);

        draw_bids(f, chunks[0], &book.bids, app.selected_bid_index);
        draw_asks(f, chunks[1], &book.asks, app.selected_ask_index);
    } else {
        let no_data = Paragraph::new("No order book data")
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);
        f.render_widget(no_data, inner);
    }
}

fn draw_bids(f: &mut Frame, area: Rect, bids: &[(u32, u32)], selected: usize) {
    let header = Row::new(vec!["Size", "Bid"])
        .style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = bids.iter().enumerate().map(|(i, (price, qty))| {
        let style = if i == selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };

        Row::new(vec![
            Cell::from(qty.to_string()),
            Cell::from(format_price(*price)),
        ])
        .style(style.fg(Color::Green))
    }).collect();

    let widths = [
        Constraint::Percentage(50),
        Constraint::Percentage(50),
    ];

    let table = Table::new(rows, widths)  // Fixed: pass widths as second argument
        .header(header)
        .block(Block::default().title("BIDS").borders(Borders::TOP));

    f.render_widget(table, area);
}

fn draw_asks(f: &mut Frame, area: Rect, asks: &[(u32, u32)], selected: usize) {
    let header = Row::new(vec!["Ask", "Size"])
        .style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = asks.iter().enumerate().map(|(i, (price, qty))| {
        let style = if i == selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };

        Row::new(vec![
            Cell::from(format_price(*price)),
            Cell::from(qty.to_string()),
        ])
        .style(style.fg(Color::Red))
    }).collect();

    let widths = [
        Constraint::Percentage(50),
        Constraint::Percentage(50),
    ];

    let table = Table::new(rows, widths)  // Fixed: pass widths as second argument
        .header(header)
        .block(Block::default().title("ASKS").borders(Borders::TOP));

    f.render_widget(table, area);
}

fn format_price(price: u32) -> String {
    format!("{:.2}", price as f64 / 100.0)
}
