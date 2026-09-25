use crate::config::UiTheme;
use crate::libs::ockql::QueryEvaluationContext;
use crate::services::container_logging::ContainerLoggingService;
use crate::services::filter::FilterService;
use crate::terminal_colors::TerminalColorMode;
use crate::ui::state::selection::{SelectionCalculator, VisualMode};
use crate::ui::state::AppUiState;
use crate::ui::theme::selection_style::SelectionTheme;
use crate::ui::views::log_viewport::components::cursor::CursorPositioner;
use crate::ui::views::log_viewport::components::log_row::{LogRowRenderer, RowRenderParams};
use crate::ui::views::log_viewport::components::unfolded_row::{UnfoldedChunkParams, UnfoldedRowRenderer};
use crate::ui::views::log_viewport::line_composer::LineComposer;
use crate::utils::text::TextUtils;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub struct ViewportRenderParams<'a> {
    pub ui: &'a mut AppUiState,
    pub logging: &'a ContainerLoggingService,
    pub filter: &'a FilterService,
    pub theme: &'a UiTheme,
    pub color_mode: &'a TerminalColorMode,
}

pub struct LogViewportView;

impl LogViewportView {
    pub fn render(frame: &mut Frame, area: Rect, p: ViewportRenderParams<'_>) {
        p.ui.viewport_height = area.height as usize;
        p.ui.viewport_width = area.width as usize;

        let active_bg = SelectionTheme::active_line(p.color_mode);
        let sel_style = SelectionTheme::selection(p.color_mode);
        let ref_now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();

        if p.ui.line_fold {
            Self::render_folded_mode(frame, area, &p, active_bg, sel_style, ref_now);
        } else {
            Self::render_unfolded_mode(frame, area, &p, active_bg, sel_style, ref_now);
        }
    }

    fn render_folded_mode(
        frame: &mut Frame,
        area: Rect,
        p: &ViewportRenderParams<'_>,
        active_bg: ratatui::style::Style,
        sel_style: ratatui::style::Style,
        ref_now: u64,
    ) {
        let mut lines = Vec::new();
        let total = p.logging.buffer().visible_count();
        let end = (p.ui.scroll_offset + p.ui.viewport_height).min(total);

        for row_idx in p.ui.scroll_offset..end {
            if let Some(record) = p.logging.buffer().get_visible(row_idx) {
                let is_current = p.ui.visual_mode == VisualMode::None && p.ui.mouse_mode == VisualMode::None && row_idx == p.ui.cursor_row;
                let is_toggled = p.ui.toggled_lines.contains(&row_idx);
                let is_dimmed = p.filter.prompt().preview_ast.as_ref().is_some_and(|q| {
                    let ctx = QueryEvaluationContext { content: &record.content, timestamp_secs: record.timestamp_secs, reference_now_secs: ref_now };
                    !q.matches(&ctx)
                });

                let is_line_selected = match (p.ui.visual_mode, p.ui.visual_anchor) {
                    (VisualMode::Line, Some((ar, _))) => row_idx >= ar.min(p.ui.cursor_row) && row_idx <= ar.max(p.ui.cursor_row),
                    _ => match (p.ui.mouse_mode, p.ui.mouse_anchor, p.ui.mouse_head) {
                        (VisualMode::Line, Some((ar, _)), Some((hr, _))) => row_idx >= ar.min(hr) && row_idx <= ar.max(hr),
                        _ => false,
                    },
                };

                let prefix_len = record.prefix_len();
                let avail_width = p.ui.viewport_width.saturating_sub(prefix_len);
                let sel_range = SelectionCalculator::get_effective_range(
                    p.ui.visual_mode,
                    p.ui.visual_anchor,
                    (p.ui.cursor_row, p.ui.cursor_col),
                    p.ui.mouse_mode,
                    p.ui.mouse_anchor,
                    p.ui.mouse_head,
                    row_idx,
                    record.content.chars().count(),
                );

                let params = RowRenderParams {
                    record,
                    is_current,
                    is_toggled,
                    is_dimmed,
                    is_line_selected,
                    h_scroll: p.ui.h_scroll,
                    avail_width,
                    viewport_width: p.ui.viewport_width,
                    sel_range,
                    active_bg,
                    sel_style,
                };
                lines.push(Line::from(LogRowRenderer::render_folded(params, p.theme)));
            }
        }

        frame.render_widget(Paragraph::new(lines), area);

        if !p.ui.picker.is_open && p.ui.cursor_row >= p.ui.scroll_offset && p.ui.cursor_row < end {
            if let Some(record) = p.logging.buffer().get_visible(p.ui.cursor_row) {
                let vy = p.ui.cursor_row - p.ui.scroll_offset;
                if record.is_system {
                    let slice: String = record.content.chars().take(p.ui.cursor_col).collect();
                    let vx = TextUtils::display_width_up_to(&slice, p.ui.cursor_col);
                    if vx < p.ui.viewport_width {
                        CursorPositioner::place(frame, area, vx, vy);
                    }
                } else {
                    let p_str = record.tag_text();
                    let p_width = TextUtils::display_width_up_to(&p_str, p_str.chars().count()) + if p.ui.toggled_lines.contains(&p.ui.cursor_row) { 2 } else { 0 };
                    let has_left = p.ui.h_scroll > 0 && !record.content.is_empty();
                    let left_pad = if has_left { 1 } else { 0 };

                    if p.ui.cursor_col >= p.ui.h_scroll {
                        let rel_chars = p.ui.cursor_col - p.ui.h_scroll;
                        let visible_content_slice: String = record.content.chars().skip(p.ui.h_scroll).take(rel_chars).collect();
                        let content_w = TextUtils::display_width_up_to(&visible_content_slice, rel_chars);
                        let vx = p_width + left_pad + content_w;
                        let max_allowed_x = p.ui.viewport_width.saturating_sub(2);
                        if vx <= max_allowed_x {
                            CursorPositioner::place(frame, area, vx, vy);
                        }
                    }
                }
            }
        }
    }

    fn render_unfolded_mode(
        frame: &mut Frame,
        area: Rect,
        p: &ViewportRenderParams<'_>,
        active_bg: ratatui::style::Style,
        sel_style: ratatui::style::Style,
        _ref_now: u64,
    ) {
        let mut lines = Vec::new();
        let mut row_idx = p.ui.scroll_offset;
        let total = p.logging.buffer().visible_count();
        let mut cursor_screen_pos = None;

        while row_idx < total && lines.len() < p.ui.viewport_height {
            if let Some(record) = p.logging.buffer().get_visible(row_idx) {
                let is_current = p.ui.visual_mode == VisualMode::None && p.ui.mouse_mode == VisualMode::None && row_idx == p.ui.cursor_row;
                let is_toggled = p.ui.toggled_lines.contains(&row_idx);
                let is_dimmed = p.filter.prompt().preview_ast.as_ref().is_some_and(|q| {
                    let ctx = QueryEvaluationContext { content: &record.content, timestamp_secs: record.timestamp_secs, reference_now_secs: _ref_now };
                    !q.matches(&ctx)
                });

                let prefix_len = record.prefix_len();
                let content_w = p.ui.viewport_width.saturating_sub(prefix_len + 2).max(1);
                let chunks = LineComposer::get_unfolded_chunks(&record.content, content_w);
                let sel_range = SelectionCalculator::get_effective_range(
                    p.ui.visual_mode,
                    p.ui.visual_anchor,
                    (p.ui.cursor_row, p.ui.cursor_col),
                    p.ui.mouse_mode,
                    p.ui.mouse_anchor,
                    p.ui.mouse_head,
                    row_idx,
                    record.content.chars().count(),
                );

                for (chunk_index, chunk) in chunks.iter().enumerate() {
                    if lines.len() >= p.ui.viewport_height { break; }
                    let vy = lines.len();

                    let params = UnfoldedChunkParams {
                        record,
                        chunk,
                        chunk_index,
                        is_toggled,
                        is_dimmed,
                        sel_range,
                        base_bg: if is_current { active_bg } else { ratatui::style::Style::default() },
                        sel_style,
                    };

                    lines.push(UnfoldedRowRenderer::render_chunk(params, p.theme));

                    if is_current && cursor_screen_pos.is_none() {
                        let chunk_chars = chunk.text.chars().count();
                        let in_chunk = p.ui.cursor_col >= chunk.content_char_start
                            && (p.ui.cursor_col < chunk.content_char_start + chunk_chars || chunk_index + 1 == chunks.len());

                        if in_chunk {
                            let p_str = record.tag_text();
                            let p_width = TextUtils::display_width_up_to(&p_str, p_str.chars().count());
                            let indent = if chunk_index == 0 { p_width + if is_toggled { 2 } else { 0 } } else { prefix_len + if is_toggled { 2 } else { 0 } };
                            let rel_chars = p.ui.cursor_col.saturating_sub(chunk.content_char_start);
                            let slice: String = chunk.text.chars().take(rel_chars).collect();
                            let col_w = TextUtils::display_width_up_to(&slice, rel_chars);
                            cursor_screen_pos = Some((indent + col_w, vy));
                        }
                    }
                }
            }
            row_idx += 1;
        }

        frame.render_widget(Paragraph::new(lines), area);
        if !p.ui.picker.is_open {
            if let Some((vx, vy)) = cursor_screen_pos {
                CursorPositioner::place(frame, area, vx, vy);
            }
        }
    }
}
