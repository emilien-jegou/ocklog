use ratatui::style::{Color, Style};
use ratatui::text::Span;

pub struct ContinuationIndicator;

impl ContinuationIndicator {
    pub fn ellipsis(base_bg: Style) -> Span<'static> {
        Span::styled("…", Style::default().fg(Color::DarkGray).patch(base_bg))
    }

    pub fn wrap_symbol(base_bg: Style) -> Span<'static> {
        Span::styled(" ↵", Style::default().fg(Color::DarkGray).patch(base_bg))
    }
}
