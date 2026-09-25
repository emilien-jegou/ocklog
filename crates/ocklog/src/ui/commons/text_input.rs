use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub struct TextInput;

impl TextInput {
    pub fn render_line(frame: &mut Frame, area: Rect, prefix: Span<'static>, content: Vec<Span<'static>>) {
        let mut spans = vec![prefix];
        spans.extend(content);
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}
