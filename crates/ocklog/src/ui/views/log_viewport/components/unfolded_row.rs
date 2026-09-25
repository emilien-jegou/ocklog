use crate::config::UiTheme;
use crate::services::container_logging::transformer::ProcessedLogRecord;
use crate::ui::theme::ThemePalette;
use crate::ui::views::log_viewport::components::continuation::ContinuationIndicator;
use crate::ui::views::log_viewport::components::slice_styler::{SegmentSliceParams, SegmentSliceStyler};
use crate::ui::views::log_viewport::line_composer::LineVisualChunk;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub struct UnfoldedChunkParams<'a> {
    pub record: &'a ProcessedLogRecord,
    pub chunk: &'a LineVisualChunk,
    pub chunk_index: usize,
    pub is_toggled: bool,
    pub is_dimmed: bool,
    pub sel_range: Option<(usize, usize)>,
    pub base_bg: Style,
    pub sel_style: Style,
}

pub struct UnfoldedRowRenderer;

impl UnfoldedRowRenderer {
    pub fn render_chunk(p: UnfoldedChunkParams<'_>, theme: &UiTheme) -> Line<'static> {
        let mut spans = Vec::new();
        let prefix_len = p.record.prefix_len();
        let dim_fg = theme.dim.to_ratatui();

        if p.chunk_index == 0 {
            if p.is_toggled {
                spans.push(Span::styled("✔ ", Style::default().fg(theme.status_active.to_ratatui()).patch(p.base_bg)));
            }
            if !p.record.is_system {
                let svc_color = if p.is_dimmed { dim_fg } else { ThemePalette::service_color(&p.record.service_name) };
                spans.push(Span::styled(p.record.tag_text(), Style::default().fg(svc_color).add_modifier(Modifier::BOLD).patch(p.base_bg)));
            }
        } else if !p.record.is_system {
            let indent = prefix_len + if p.is_toggled { 2 } else { 0 };
            spans.push(Span::styled(" ".repeat(indent), p.base_bg));
        }

        let chunk_start = p.chunk.content_char_start;
        let chunk_len = p.chunk.text.chars().count();
        let chunk_end = chunk_start + chunk_len;

        if !p.record.segments.is_empty() {
            let params = SegmentSliceParams {
                segments: &p.record.segments,
                sel_range: p.sel_range,
                h_start: chunk_start,
                h_end: chunk_end,
                base_bg: p.base_bg,
                sel_style: p.sel_style,
                is_dimmed: p.is_dimmed,
                dim_fg,
            };
            spans.extend(SegmentSliceStyler::render_spans(params));
        } else {
            let normal_style = if p.is_dimmed { Style::default().fg(dim_fg).patch(p.base_bg) } else { Style::default().fg(theme.fg.to_ratatui()).patch(p.base_bg) };
            Self::push_content_spans(&mut spans, &p.chunk.text, chunk_start, p.sel_range, normal_style, p.sel_style);
        }

        if p.chunk.has_continuation {
            spans.push(ContinuationIndicator::wrap_symbol(p.base_bg));
        }

        Line::from(spans)
    }

    fn push_content_spans(
        spans: &mut Vec<Span<'static>>,
        text: &str,
        start_col: usize,
        sel: Option<(usize, usize)>,
        normal: Style,
        selected: Style,
    ) {
        match sel {
            None => spans.push(Span::styled(text.to_string(), normal)),
            Some((s, e)) => {
                for (i, c) in text.chars().enumerate() {
                    let col = start_col + i;
                    let style = if col >= s && col <= e { selected } else { normal };
                    spans.push(Span::styled(c.to_string(), style));
                }
            }
        }
    }
}
