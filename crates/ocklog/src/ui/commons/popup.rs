use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear};
use ratatui::Frame;

pub struct Popup;

impl Popup {
    pub fn render<F: FnOnce(Rect, &mut Frame)>(frame: &mut Frame, area: Rect, title: &str, render_inner: F) {
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", title))
            .border_style(Style::default().fg(Color::Gray));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        render_inner(inner, frame);
    }
}
