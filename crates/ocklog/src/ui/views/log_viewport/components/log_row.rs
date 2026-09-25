use crate::config::UiTheme;
use crate::services::container_logging::transformer::ProcessedLogRecord;
use crate::ui::theme::ThemePalette;
use crate::ui::views::log_viewport::components::continuation::ContinuationIndicator;
use crate::ui::views::log_viewport::components::slice_styler::{SegmentSliceParams, SegmentSliceStyler};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

pub struct RowRenderParams<'a> {
    pub record: &'a ProcessedLogRecord,
    pub is_current: bool,
    pub is_toggled: bool,
    pub is_dimmed: bool,
    pub is_line_selected: bool,
    pub h_scroll: usize,
    pub avail_width: usize,
    pub viewport_width: usize,
    pub sel_range: Option<(usize, usize)>,
    pub active_bg: Style,
    pub sel_style: Style,
}

pub struct LogRowRenderer;

impl LogRowRenderer {
    pub fn render_folded(p: RowRenderParams<'_>, theme: &UiTheme) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let base_bg = if p.is_current { p.active_bg } else { Style::default() };
        let dim_fg = theme.dim.to_ratatui();

        if p.is_toggled {
            spans.push(Span::styled("✔ ", Style::default().fg(theme.status_active.to_ratatui()).patch(base_bg)));
        }

        if p.record.is_system {
            let style = Self::resolve_system_style(&p.record.content, p.is_dimmed, dim_fg, theme).patch(base_bg);
            let content_chars: Vec<char> = p.record.content.chars().collect();
            let total = content_chars.len();
            let take_len = total.min(p.viewport_width);

            Self::push_system_highlighted_slice(
                &mut spans,
                &content_chars[..take_len],
                p.sel_range,
                style,
                p.sel_style,
            );
        } else {
            let svc_color = if p.is_dimmed { dim_fg } else { ThemePalette::service_color(&p.record.service_name) };
            spans.push(Span::styled(p.record.tag_text(), Style::default().fg(svc_color).add_modifier(Modifier::BOLD).patch(base_bg)));

            let total = p.record.content.chars().count();
            let (start, end, has_left, has_right) = Self::h_bounds(p.h_scroll, p.avail_width, total);

            if has_left {
                spans.push(ContinuationIndicator::ellipsis(base_bg));
            }

            if !p.record.segments.is_empty() {
                let params = SegmentSliceParams {
                    segments: &p.record.segments,
                    sel_range: p.sel_range,
                    h_start: start,
                    h_end: end,
                    base_bg,
                    sel_style: p.sel_style,
                    is_dimmed: p.is_dimmed,
                    dim_fg,
                };
                spans.extend(SegmentSliceStyler::render_spans(params));
            } else {
                let content_chars: Vec<char> = p.record.content.chars().collect();
                Self::push_content_slices(&mut spans, &content_chars[start..end], start, p.sel_range, p.is_dimmed, base_bg, p.sel_style, theme);
            }

            if has_right {
                spans.push(ContinuationIndicator::ellipsis(base_bg));
            }
        }

        let cur_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        if cur_len < p.viewport_width {
            let padding = " ".repeat(p.viewport_width - cur_len);
            let pad_style = if p.is_line_selected { p.sel_style } else if p.is_current { p.active_bg } else { Style::default() };
            spans.push(Span::styled(padding, pad_style));
        }

        spans
    }

    fn resolve_system_style(content: &str, is_dimmed: bool, dim_fg: Color, theme: &UiTheme) -> Style {
        if is_dimmed {
            Style::default().fg(dim_fg).add_modifier(Modifier::DIM)
        } else if content.contains("DIE") || content.contains("KILL") || content.contains("UNHEALTHY") {
            Style::default().fg(theme.status_error.to_ratatui()).add_modifier(Modifier::BOLD)
        } else if content.contains("RESTART") || content.contains("REMOVED") {
            Style::default().fg(theme.status_warn.to_ratatui()).add_modifier(Modifier::BOLD)
        } else if content.contains("START") || content.contains("HEALTHY") {
            Style::default().fg(theme.status_active.to_ratatui()).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.status_warn.to_ratatui())
        }
    }

    fn push_system_highlighted_slice(
        spans: &mut Vec<Span<'static>>,
        chars: &[char],
        sel: Option<(usize, usize)>,
        system_style: Style,
        sel_style: Style,
    ) {
        match sel {
            None => spans.push(Span::styled(chars.iter().collect::<String>(), system_style)),
            Some((s, e)) => {
                for (col, &c) in chars.iter().enumerate() {
                    let style = if col >= s && col <= e { sel_style } else { system_style };
                    spans.push(Span::styled(c.to_string(), style));
                }
            }
        }
    }

    fn push_content_slices(
        spans: &mut Vec<Span<'static>>,
        chars: &[char],
        start_idx: usize,
        sel: Option<(usize, usize)>,
        is_dimmed: bool,
        base_bg: Style,
        sel_style: Style,
        theme: &UiTheme,
    ) {
        let text: String = chars.iter().collect();
        if start_idx == 0 && text.starts_with("(x") {
            if let Some(close) = text.find(") ") {
                let count_str = &text[..close + 1];
                let badge_style = Style::default()
                    .bg(Color::Magenta)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD);
                spans.push(Span::styled(count_str.to_string(), badge_style));
                spans.push(Span::styled(" ", base_bg));

                let rem: Vec<char> = chars.iter().skip(close + 2).copied().collect();
                Self::push_highlighted_slice(spans, &rem, start_idx + close + 2, sel, is_dimmed, base_bg, sel_style, theme);
                return;
            }
        }
        Self::push_highlighted_slice(spans, chars, start_idx, sel, is_dimmed, base_bg, sel_style, theme);
    }

    fn push_highlighted_slice(
        spans: &mut Vec<Span<'static>>,
        chars: &[char],
        content_start_idx: usize,
        sel: Option<(usize, usize)>,
        is_dimmed: bool,
        base_bg: Style,
        sel_style: Style,
        theme: &UiTheme,
    ) {
        let normal_style = if is_dimmed {
            Style::default().fg(theme.dim.to_ratatui()).patch(base_bg)
        } else {
            Style::default().fg(theme.fg.to_ratatui()).patch(base_bg)
        };

        match sel {
            None => spans.push(Span::styled(chars.iter().collect::<String>(), normal_style)),
            Some((s, e)) => {
                for (i, &c) in chars.iter().enumerate() {
                    let col = content_start_idx + i;
                    let style = if col >= s && col <= e { sel_style } else { normal_style };
                    spans.push(Span::styled(c.to_string(), style));
                }
            }
        }
    }

    fn h_bounds(h: usize, w: usize, total: usize) -> (usize, usize, bool, bool) {
        if w <= 2 { return (0, total, false, false); }
        let has_left = h > 0 && total > 0;
        let mut capacity = w.saturating_sub(if has_left { 1 } else { 0 });
        let has_right = (h + capacity) < total;
        if has_right {
            capacity = capacity.saturating_sub(1);
        }
        (h.min(total), (h + capacity).min(total), has_left, has_right)
    }
}
