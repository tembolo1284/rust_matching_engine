// crates/engine-trading-client/src/components/status_bar.rs

use ratatui::{
    backend::Backend,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::{App, InputMode};

pub fn draw_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let (msg, style) = match app.input_mode {
        InputMode::Normal => {
            let shortcuts = vec![
                Span::styled("[B]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("uy "),
                Span::styled("[S]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("ell "),
                Span::styled("[M]", Style::default().fg(Color::Yellow)),
                Span::raw("arket "),
                Span::styled("[C]", Style::default().fg(Color::Cyan)),
                Span::raw("ancel "),
                Span::styled("[X]", Style::default().fg(Color::Magenta)),
                Span::raw("Cancel All "),
                Span::styled("[Q]", Style::default().fg(Color::Gray)),
                Span::raw("uit"),
            ];
            (Line::from(shortcuts), Style::default())
        }
        InputMode::Editing => {
            let input = vec![
                Span::raw("Input: "),
                Span::styled(&app.input_buffer, Style::default().fg(Color::Yellow)),
                Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
                Span::raw(" [Enter] Submit [Esc] Cancel"),
            ];
            (Line::from(input), Style::default().fg(Color::Yellow))
        }
    };

    let status_block = Block::default()
        .borders(Borders::ALL)
        .border_style(style);

    let paragraph = Paragraph::new(msg)
        .block(status_block)
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}
