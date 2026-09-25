use crate::services::filter::prompt_state::PromptMode;
use crate::services::filter::FilterService;
use crate::ui::commons::text_input::TextInput;
use crate::ui::views::command_line::components::syntax_highlighter::SyntaxHighlighter;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::Frame;

pub struct CommandLineView;

impl CommandLineView {
    pub fn render(frame: &mut Frame, area: Rect, filter_service: &FilterService) {
        let prompt = filter_service.prompt();
        if prompt.mode == PromptMode::Active {
            let prefix = Span::styled("filter: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
            let spans = SyntaxHighlighter::highlight(&prompt.input_buffer);
            TextInput::render_line(frame, area, prefix, spans);

            let cx = area.x + 8 + prompt.cursor_col as u16;
            frame.set_cursor_position(Position::new(cx.min(area.right().saturating_sub(1)), area.y));
        } else if let Some(active) = filter_service.active_filter() {
            let prefix = Span::styled("active: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            let spans = SyntaxHighlighter::highlight(active.raw());
            TextInput::render_line(frame, area, prefix, spans);
        }
    }
}
