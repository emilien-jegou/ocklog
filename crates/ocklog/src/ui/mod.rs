pub mod commons;
pub mod state;
pub mod theme;
pub mod views;

use crate::config::UiTheme;
use crate::services::ServiceRegistry;
use crate::terminal_colors::TerminalColorMode;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use state::AppUiState;
use std::time::Duration;
use views::command_line::CommandLineView;
use views::log_viewport::{LogViewportView, ViewportRenderParams};
use views::service_picker::ServicePickerView;
use views::status_bar::StatusBarView;

pub struct MasterUiRenderer;

impl MasterUiRenderer {
    pub fn render(
        frame: &mut Frame,
        ui: &mut AppUiState,
        services: &ServiceRegistry,
        theme: &UiTheme,
        color_mode: &TerminalColorMode,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)])
            .split(frame.area());

        let params = ViewportRenderParams { ui, logging: &services.logging, filter: &services.filter, theme, color_mode };
        LogViewportView::render(frame, chunks[0], params);
        StatusBarView::render(frame, chunks[1], ui, &services.logging, &services.filter, theme);
        CommandLineView::render(frame, chunks[2], &services.filter);
        ServicePickerView::render(frame, chunks[0], &mut ui.picker, theme);

        Self::render_clipboard_popup(frame, chunks[0], ui);
    }

    fn render_clipboard_popup(frame: &mut Frame, parent: Rect, ui: &AppUiState) {
        if let Some((msg, instant)) = &ui.clipboard_notice {
            if instant.elapsed() < Duration::from_millis(2000) {
                let width = (msg.chars().count() as u16 + 4).min(parent.width.saturating_sub(2));
                let area = Rect {
                    x: parent.right().saturating_sub(width + 1),
                    y: parent.y + 1,
                    width,
                    height: 3,
                };
                frame.render_widget(Clear, area);
                let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Gray));
                let p = Paragraph::new(Line::from(vec![
                    Span::styled(msg.as_str(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ])).block(block);
                frame.render_widget(p, area);
            }
        }
    }
}
