use crate::config::UiTheme;
use crate::services::container_logging::ContainerLoggingService;
use crate::services::filter::prompt_state::PromptMode;
use crate::services::filter::FilterService;
use crate::ui::state::AppUiState;
use crate::ui::views::status_bar::components::duration_gauge::DurationGauge;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub struct StatusBarView;

impl StatusBarView {
    pub fn render(frame: &mut Frame, area: Rect, ui: &AppUiState, logging: &ContainerLoggingService, filter: &FilterService, theme: &UiTheme) {
        let left_spans = Self::render_left_side(ui.line_fold, filter.prompt().mode, theme);
        let right_spans = Self::render_telemetry(ui, logging);

        let right_len = right_spans.iter().map(|s| s.content.chars().count()).sum::<usize>() as u16;
        let left_area = Rect { width: area.width.saturating_sub(right_len), ..area };
        let right_area = Rect {
            x: area.right().saturating_sub(right_len),
            width: right_len,
            ..area
        };

        frame.render_widget(Paragraph::new(Line::from(left_spans)), left_area);
        frame.render_widget(Paragraph::new(Line::from(right_spans)), right_area);
    }

    fn render_left_side(line_fold: bool, prompt_mode: PromptMode, theme: &UiTheme) -> Vec<Span<'static>> {
        let d = Style::default().fg(theme.dim.to_ratatui());
        if prompt_mode == PromptMode::Active {
            return vec![
                Span::styled("hints: ", d.add_modifier(Modifier::BOLD)),
                Span::styled("\"err\" or 'warn' | `ipv4` | context 3 | dedup | dedup all | since 5m", d),
            ];
        }

        let k = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
        let fold_str = if line_fold { " unfold  " } else { " fold    " };

        vec![
            Span::styled("C-s", k), Span::styled(" services  ", d),
            Span::styled("C-f", k), Span::styled(" filter  ", d),
            Span::styled("z", k), Span::styled(fold_str, d),
            Span::styled("?", k), Span::styled(" help", d),
        ]
    }

    fn render_telemetry(ui: &AppUiState, logging: &ContainerLoggingService) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let total = logging.buffer().visible_count();

        if let Some((anchor_r, _)) = ui.visual_anchor {
            let (r1, r2) = (anchor_r.min(ui.cursor_row), anchor_r.max(ui.cursor_row));
            if let (Some(l1), Some(l2)) = (logging.buffer().get_visible(r1), logging.buffer().get_visible(r2)) {
                if let (Some(t1), Some(t2)) = (&l1.timestamp_rfc3339, &l2.timestamp_rfc3339) {
                    if let Some(span) = DurationGauge::from_timestamps(t1, t2) {
                        spans.push(Span::styled("\u{f13ab} ", Style::default().fg(Color::LightCyan)));
                        spans.push(span);
                    }
                }
            }
        } else if let Some(log) = logging.buffer().get_visible(ui.cursor_row) {
            if let Some(secs) = log.timestamp_secs {
                let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                let diff = wall.saturating_sub(secs);
                spans.push(Span::styled(format!("\u{f144e} {}  ", crate::utils::time::TimeFormatter::format_duration(diff)), Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)));
            }
        }

        let max_scroll = total.saturating_sub(ui.viewport_height);
        let at_bottom = ui.scroll_offset >= max_scroll;
        let badge_style = if !at_bottom && total > 0 {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let indicator = format!("  {}:{} / {} ", ui.cursor_row + 1, ui.cursor_col + 1, total);
        spans.push(Span::styled(indicator, badge_style));
        spans
    }
}
