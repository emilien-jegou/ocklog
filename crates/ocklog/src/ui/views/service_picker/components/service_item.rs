use crate::config::UiTheme;
use crate::ui::state::service_picker::ServicePickerOption;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct ServiceItemRenderer;

impl ServiceItemRenderer {
    pub fn render(opt: &ServicePickerOption, is_selected: bool, theme: &UiTheme) -> Line<'static> {
        let bg = if is_selected { Style::default().bg(Color::DarkGray) } else { Style::default() };
        let check_color = theme.status_active.to_ratatui();

        let check_span = if opt.enabled {
            Span::styled("✔ ", Style::default().fg(check_color).add_modifier(Modifier::BOLD).patch(bg))
        } else {
            Span::styled("  ", bg)
        };

        let dot_color = if opt.has_error {
            theme.status_error.to_ratatui()
        } else if opt.is_running {
            theme.status_active.to_ratatui()
        } else {
            theme.dim.to_ratatui()
        };

        let name_span = Span::styled(format!(" {} ", opt.name), Style::default().fg(Color::White).patch(bg));
        let dot_span = Span::styled("●", Style::default().fg(dot_color).patch(bg));

        Line::from(vec![check_span, name_span, dot_span])
    }
}
