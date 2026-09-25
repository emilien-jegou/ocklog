use crate::ansi::AnsiSegment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

pub struct SegmentSliceParams<'a> {
    pub segments: &'a [AnsiSegment],
    pub sel_range: Option<(usize, usize)>,
    pub h_start: usize,
    pub h_end: usize,
    pub base_bg: Style,
    pub sel_style: Style,
    pub is_dimmed: bool,
    pub dim_fg: ratatui::style::Color,
}

pub struct SegmentSliceStyler;

impl SegmentSliceStyler {
    pub fn render_spans(p: SegmentSliceParams<'_>) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let mut current_offset = 0;

        for seg in p.segments {
            let seg_len = seg.text.chars().count();
            let seg_start = current_offset;
            let seg_end = current_offset + seg_len;
            current_offset += seg_len;

            if seg_end <= p.h_start || seg_start >= p.h_end {
                continue;
            }

            let take_start = p.h_start.saturating_sub(seg_start);
            let take_len = (p.h_end.min(seg_end) - seg_start).saturating_sub(take_start);
            let slice: String = seg.text.chars().skip(take_start).take(take_len).collect();

            if seg.text.starts_with("(x") && seg.text.ends_with(") ") {
                if let Some(close) = slice.find(") ") {
                    let badge = &slice[..close + 1];
                    let badge_style = Style::default()
                        .bg(Color::Magenta)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD);
                    spans.push(Span::styled(badge.to_string(), badge_style));
                    spans.push(Span::styled(" ", p.base_bg));
                    continue;
                }
            }

            let seg_style = if p.is_dimmed {
                Style::default().fg(p.dim_fg).patch(p.base_bg)
            } else if seg.style.bg.is_some() {
                seg.style
            } else {
                seg.style.patch(p.base_bg)
            };

            let slice_global_start = seg_start + take_start;

            match p.sel_range {
                None => {
                    spans.push(Span::styled(slice, seg_style));
                }
                Some((s, e)) => {
                    for (i, c) in slice.chars().enumerate() {
                        let col = slice_global_start + i;
                        let style = if col >= s && col <= e { p.sel_style } else { seg_style };
                        spans.push(Span::styled(c.to_string(), style));
                    }
                }
            }
        }
        spans
    }
}
