use crate::config::UiTheme;
use crate::ui::commons::popup::Popup;
use crate::ui::state::service_picker::ServicePickerState;
use crate::ui::views::service_picker::components::service_item::ServiceItemRenderer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub struct ServicePickerView;

impl ServicePickerView {
    pub fn render(
        frame: &mut Frame,
        parent: Rect,
        picker: &mut ServicePickerState,
        theme: &UiTheme,
    ) {
        if !picker.is_open {
            return;
        }

        let width = 56u16.min(parent.width);
        let height = 16u16.min(parent.height);
        let area = Rect {
            x: parent.x,
            y: parent.bottom().saturating_sub(height),
            width,
            height,
        };

        Popup::render(
            frame,
            area,
            "Services",
            |inner_area: Rect, f: &mut Frame| {
                let constraints = if picker.is_searching {
                    vec![
                        Constraint::Min(1),
                        Constraint::Length(2),
                        Constraint::Length(2),
                    ]
                } else {
                    vec![Constraint::Min(1), Constraint::Length(2)]
                };

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(constraints)
                    .split(inner_area);
                Self::render_list(f, chunks[0], picker, theme);
                Self::render_footer(f, chunks[1], theme);

                if picker.is_searching {
                    Self::render_search(f, chunks[2], picker, theme);
                }
            },
        );
    }

    fn render_list(
        frame: &mut Frame,
        area: Rect,
        picker: &mut ServicePickerState,
        theme: &UiTheme,
    ) {
        let filtered = picker.filtered_indices();
        if filtered.is_empty() {
            let empty_lines = area.height.saturating_sub(1) / 2;
            let mut lines = vec![Line::raw(""); empty_lines as usize];
            lines.push(
                Line::from(vec![Span::styled(
                    "No results",
                    Style::default().fg(theme.dim.to_ratatui()),
                )])
                .alignment(Alignment::Center),
            );
            frame.render_widget(Paragraph::new(lines), area);
            return;
        }

        let list_height = area.height as usize;
        let margin = 3.min(list_height.saturating_sub(1) / 2);

        if picker.selected_idx < picker.scroll_offset + margin {
            picker.scroll_offset = picker.selected_idx.saturating_sub(margin);
        }
        if picker.selected_idx + margin >= picker.scroll_offset + list_height {
            picker.scroll_offset = (picker.selected_idx + margin + 1).saturating_sub(list_height);
        }
        let max_scroll = filtered.len().saturating_sub(list_height);
        if picker.scroll_offset > max_scroll {
            picker.scroll_offset = max_scroll;
        }

        let start = picker.scroll_offset;
        let end = (start + list_height).min(filtered.len());

        let items: Vec<_> = (start..end)
            .map(|idx| {
                let actual_idx = filtered[idx];
                let opt = &picker.options[actual_idx];
                ServiceItemRenderer::render(opt, idx == picker.selected_idx, theme)
            })
            .collect();

        frame.render_widget(Paragraph::new(items), area);
    }

    fn render_footer(frame: &mut Frame, area: Rect, theme: &UiTheme) {
        let k = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let d = Style::default().fg(theme.dim.to_ratatui());

        let lines = vec![
            Line::from(vec![
                Span::styled("r", k),
                Span::styled(" Restart  ", d),
                Span::styled("s", k),
                Span::styled(" Stop  ", d),
                Span::styled("x", k),
                Span::styled(" Kill  ", d),
                Span::styled("C-o", k),
                Span::styled(" Only  ", d),
                Span::styled("D", k),
                Span::styled(" Purge", d),
            ]),
            Line::from(vec![
                Span::styled("C-a", k),
                Span::styled(" All  ", d),
                Span::styled("C-x", k),
                Span::styled(" None  ", d),
                Span::styled("C-f", k),
                Span::styled(" Search  ", d),
                Span::styled("q", k),
                Span::styled(" Close", d),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_search(frame: &mut Frame, area: Rect, picker: &ServicePickerState, theme: &UiTheme) {
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.dim.to_ratatui()));
        let search_bar = Paragraph::new(Line::from(vec![
            Span::styled("Search: ", Style::default().fg(theme.dim.to_ratatui())),
            Span::styled(
                &picker.search_query,
                Style::default().fg(theme.fg.to_ratatui()),
            ),
        ]))
        .block(block);

        frame.render_widget(search_bar, area);
        let cx = area.x + 8 + picker.search_query.chars().count() as u16;
        frame.set_cursor_position(Position::new(
            cx.min(area.right().saturating_sub(1)),
            area.y + 1,
        ));
    }
}
