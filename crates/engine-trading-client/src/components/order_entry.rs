use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{App, InputMode};
use engine_core::Side;

pub fn draw_order_entry(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title(" Order Entry ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(
            if matches!(app.current_panel, crate::app::Panel::OrderEntry) {
                Color::Yellow
            } else {
                Color::White
            }
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Side selection
            Constraint::Length(3),  // Price input
            Constraint::Length(3),  // Quantity input
            Constraint::Length(3),  // Order type
            Constraint::Min(5),     // Summary
            Constraint::Length(3),  // Actions
        ])
        .split(inner);

    // Side selection
    let side_text = if let Some(side) = &app.order_side {
        match side {
            Side::Buy => Line::from(vec![
                Span::raw("Side: "),
                Span::styled("BUY", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ]),
            Side::Sell => Line::from(vec![
                Span::raw("Side: "),
                Span::styled("SELL", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            ]),
        }
    } else {
        Line::from(vec![
            Span::raw("Side: "),
            Span::styled("[B]uy / [S]ell", Style::default().fg(Color::Gray)),
        ])
    };
    
    let side_widget = Paragraph::new(side_text)
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(side_widget, chunks[0]);

    // Price input
    let price_text = if app.is_market_order {
        Line::from(vec![
            Span::raw("Price: "),
            Span::styled("MARKET", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ])
    } else {
        Line::from(vec![
            Span::raw("Price: "),
            Span::styled(&app.order_price_input, Style::default().fg(Color::Cyan)),
            if matches!(app.input_mode, InputMode::Editing) && app.current_panel == crate::app::Panel::OrderEntry {
                Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK))
            } else {
                Span::raw("")
            },
        ])
    };
    
    let price_widget = Paragraph::new(price_text)
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(price_widget, chunks[1]);

    // Quantity input
    let qty_text = Line::from(vec![
        Span::raw("Quantity: "),
        Span::styled(&app.order_qty_input, Style::default().fg(Color::Cyan)),
    ]);
    
    let qty_widget = Paragraph::new(qty_text)
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(qty_widget, chunks[2]);

    // Order type
    let type_text = Line::from(vec![
        Span::raw("Type: "),
        if app.is_market_order {
            Span::styled("Market", Style::default().fg(Color::Yellow))
        } else {
            Span::styled("Limit", Style::default().fg(Color::Blue))
        },
        Span::raw(" "),
        Span::styled("[M] Toggle", Style::default().fg(Color::Gray)),
    ]);
    
    let type_widget = Paragraph::new(type_text)
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(type_widget, chunks[3]);

    // Summary
    let summary_items = vec![
        ListItem::new(format!("Symbol: {}", app.current_symbol)),
        ListItem::new(format!("User ID: {}", app.user_id)),
    ];
    
    let summary_list = List::new(summary_items)
        .block(Block::default().title("Summary").borders(Borders::TOP | Borders::BOTTOM));
    f.render_widget(summary_list, chunks[4]);

    // Actions
    let actions_text = if matches!(app.input_mode, InputMode::Editing) {
        "[Enter] Submit | [Esc] Cancel"
    } else {
        "[Enter] Place Order | [Esc] Clear"
    };
    
    let actions_widget = Paragraph::new(actions_text)
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Center);
    f.render_widget(actions_widget, chunks[5]);
}
