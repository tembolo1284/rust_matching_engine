// crates/engine-trading-client/src/components/help.rs

use ratatui::{
    backend::Backend,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

pub fn draw_help(f: &mut Frame, area: Rect) {
    // Clear the area first for the overlay
    f.render_widget(Clear, area);

    let help_items = vec![
        ListItem::new(Line::from(vec![
            Span::styled("B/b", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" - Place Buy Order"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("S/s", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" - Place Sell Order"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("M/m", Style::default().fg(Color::Yellow)),
            Span::raw(" - Toggle Market/Limit Order"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("C/c", Style::default().fg(Color::Cyan)),
            Span::raw(" - Cancel Selected Order"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("X/x", Style::default().fg(Color::Magenta)),
            Span::raw(" - Cancel All Orders"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Blue)),
            Span::raw(" - Next Panel"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("Shift+Tab", Style::default().fg(Color::Blue)),
            Span::raw(" - Previous Panel"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("↑/k", Style::default().fg(Color::White)),
            Span::raw(" - Move Up"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("↓/j", Style::default().fg(Color::White)),
            Span::raw(" - Move Down"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(" - Search Symbol"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("F1", Style::default().fg(Color::Gray)),
            Span::raw(" - Toggle Help"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("F2", Style::default().fg(Color::Gray)),
            Span::raw(" - Toggle Chart"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("F3", Style::default().fg(Color::Gray)),
            Span::raw(" - Toggle Market Depth"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("Q/q", Style::default().fg(Color::Red)),
            Span::raw(" - Quit"),
        ])),
    ];

    let help_list = List::new(help_items)
        .block(Block::default()
            .title(" Help - Keyboard Shortcuts ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)));

    f.render_widget(help_list, area);
    
    // Add footer with close instruction
    let footer = Paragraph::new("Press F1 or ESC to close help")
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Center);
    
    let footer_area = Rect {
        x: area.x,
        y: area.y + area.height - 1,
        width: area.width,
        height: 1,
    };
    
    f.render_widget(footer, footer_area);
}
