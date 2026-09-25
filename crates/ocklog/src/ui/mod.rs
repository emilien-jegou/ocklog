use crate::config::UiTheme;
use crate::query::{parse_duration, parse_timestamp_secs, Query};
use crate::state::{
    char_at_display_width, get_log_visual_chunks, get_unfolded_content_width, str_display_width_up_to,
    truncate_service, visual_lines_for_log, AppState, LogItem, PromptMode, VisualMode,
};
use crate::terminal_colors::TerminalColorMode;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect},
    style::{Color as RatColor, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use std::time::Duration;

pub fn render(
    frame: &mut Frame,
    state: &mut AppState,
    theme: &UiTheme,
    color_mode: &TerminalColorMode,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_logs(frame, chunks[0], state, theme, color_mode);
    render_status_bar(frame, chunks[1], state, theme);
    render_command_line(frame, chunks[2], state, theme);

    if state.show_service_menu {
        render_service_menu(frame, chunks[0], state, theme);
    }

    if let Some((msg, instant)) = &state.clipboard_notice {
        if instant.elapsed() < Duration::from_millis(2000) {
            render_clipboard_popup(frame, chunks[0], msg);
        }
    }
}

fn render_clipboard_popup(frame: &mut Frame, parent: Rect, msg: &str) {
    let width = (msg.chars().count() as u16 + 4).min(parent.width.saturating_sub(2));
    let height = 3;

    let area = Rect {
        x: parent.right().saturating_sub(width + 1),
        y: parent.y + 1,
        width,
        height,
    };

    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(RatColor::Gray));

    let content = Paragraph::new(Line::from(vec![
        Span::styled(
            msg,
            Style::default().fg(RatColor::White).add_modifier(Modifier::BOLD),
        ),
    ]))
    .block(block);

    frame.render_widget(content, area);
}

pub fn get_service_color(service: &str) -> RatColor {
    const PALETTE: &[RatColor] = &[
        RatColor::Cyan,
        RatColor::LightCyan,
        RatColor::LightGreen,
        RatColor::Yellow,
        RatColor::LightYellow,
        RatColor::Blue,
        RatColor::LightBlue,
        RatColor::Magenta,
        RatColor::LightMagenta,
        RatColor::Green,
    ];

    let mut hash: usize = 5381;
    for b in service.bytes() {
        hash = ((hash << 5).wrapping_add(hash)).wrapping_add(b as usize);
    }
    PALETTE[hash % PALETTE.len()]
}

fn compute_neutral_selection_style(color_mode: &TerminalColorMode) -> Style {
    if let TerminalColorMode::TrueColor(palette) = color_mode {
        if let Some((r, g, b)) = palette.bg {
            let luminance = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
            let is_dark = luminance < 128.0;

            let (bg_r, bg_g, bg_b) = if is_dark {
                (
                    r.saturating_add(35).min(255),
                    g.saturating_add(35).min(255),
                    b.saturating_add(35).min(255),
                )
            } else {
                (
                    r.saturating_sub(35),
                    g.saturating_sub(35),
                    b.saturating_sub(35),
                )
            };
            let (fg_r, fg_g, fg_b) = if is_dark { (255, 255, 255) } else { (0, 0, 0) };

            return Style::default()
                .bg(RatColor::Rgb(bg_r, bg_g, bg_b))
                .fg(RatColor::Rgb(fg_r, fg_g, fg_b))
                .add_modifier(Modifier::BOLD);
        }
    }

    Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
}

fn compute_neutral_active_line_style(color_mode: &TerminalColorMode) -> Style {
    if let TerminalColorMode::TrueColor(palette) = color_mode {
        if let Some((r, g, b)) = palette.bg {
            let luminance = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
            let is_dark = luminance < 128.0;

            let (bg_r, bg_g, bg_b) = if is_dark {
                (
                    r.saturating_add(20).min(255),
                    g.saturating_add(20).min(255),
                    b.saturating_add(20).min(255),
                )
            } else {
                (
                    r.saturating_sub(20),
                    r.saturating_sub(20),
                    r.saturating_sub(20),
                )
            };

            return Style::default().bg(RatColor::Rgb(bg_r, bg_g, bg_b));
        }
    }

    Style::default().bg(RatColor::Indexed(236))
}

fn push_styled_slice_with_sel(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    global_start_col: usize,
    sel_range: Option<(usize, usize)>,
    default_style: Style,
    sel_style: Style,
) {
    if text.is_empty() {
        return;
    }

    match sel_range {
        None => {
            spans.push(Span::styled(text.to_string(), default_style));
        }
        Some((sel_start, sel_end)) => {
            let chars: Vec<char> = text.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let actual_col = global_start_col + i;
                let is_sel = actual_col >= sel_start && actual_col <= sel_end;
                let start_idx = i;
                while i < chars.len() {
                    let c_col = global_start_col + i;
                    let c_sel = c_col >= sel_start && c_col <= sel_end;
                    if c_sel != is_sel {
                        break;
                    }
                    i += 1;
                }
                let slice: String = chars[start_idx..i].iter().collect();
                let style = if is_sel { sel_style } else { default_style };
                spans.push(Span::styled(slice, style));
            }
        }
    }
}

fn build_styled_log_spans(
    log: &LogItem,
    row: usize,
    state: &AppState,
    theme: &UiTheme,
    sel_style: Style,
    is_current: bool,
    is_toggled: bool,
    is_dimmed: bool,
    active_line_style: Style,
) -> Vec<Span<'static>> {
    let full_text = log.full_line_text();
    let total_chars = full_text.chars().count();
    let sel_range = get_line_selection_col_range(row, state, total_chars);

    let base_bg = if is_current {
        active_line_style
    } else if is_toggled {
        Style::default().bg(theme.selection_bg.to_ratatui())
    } else {
        Style::default()
    };

    let dim_fg = theme.dim.to_ratatui();

    if log.is_system {
        let style = if is_dimmed {
            Style::default().fg(dim_fg).add_modifier(Modifier::DIM)
        } else if log.content.contains("DIE") || log.content.contains("KILL") {
            Style::default().fg(theme.status_error.to_ratatui()).add_modifier(Modifier::BOLD)
        } else if log.content.contains("RESTART") {
            Style::default().fg(theme.status_warn.to_ratatui()).add_modifier(Modifier::BOLD)
        } else if log.content.contains("START") {
            Style::default().fg(theme.status_active.to_ratatui()).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.status_warn.to_ratatui())
        }
        .patch(base_bg);

        if let Some((start_col, end_col)) = sel_range {
            let chars: Vec<char> = full_text.chars().collect();
            let mut spans = Vec::new();

            let before: String = chars.iter().take(start_col).collect();
            if !before.is_empty() {
                spans.push(Span::styled(before, style));
            }
            let selected: String = chars
                .iter()
                .skip(start_col)
                .take(end_col.saturating_sub(start_col) + 1)
                .collect();
            if !selected.is_empty() {
                spans.push(Span::styled(selected, sel_style));
            }
            let after: String = chars.iter().skip(end_col + 1).collect();
            if !after.is_empty() {
                spans.push(Span::styled(after, style));
            }
            return spans;
        }

        return vec![Span::styled(log.content.clone(), style)];
    }

    let svc_color = if is_dimmed {
        dim_fg
    } else {
        get_service_color(&log.service)
    };

    let mut spans = Vec::new();

    if is_toggled {
        spans.push(Span::styled("✔ ", Style::default().fg(theme.status_active.to_ratatui()).patch(base_bg)));
    }

    let tag_style = if is_dimmed {
        Style::default().fg(dim_fg).patch(base_bg)
    } else {
        Style::default().fg(svc_color).add_modifier(Modifier::BOLD).patch(base_bg)
    };
    spans.push(Span::styled(log.tag_text(), tag_style));

    let prefix_chars = log.prefix_len();
    let avail_width = state.viewport_width.saturating_sub(prefix_chars);
    let content_chars: Vec<char> = log.content.chars().collect();
    let content_len = content_chars.len();

    let effective_cursor_in_content = state.cursor_col.saturating_sub(prefix_chars).min(content_len);
    let cursor_out_of_view_left = is_current && state.h_scroll > 0 && effective_cursor_in_content < state.h_scroll;

    let (h_start, h_end, has_left, has_right) = if avail_width <= 2 {
        (0, content_len, false, false)
    } else {
        let has_left = state.h_scroll > 0 && content_len > 0;
        let mut capacity = avail_width - (if has_left { 1 } else { 0 });
        let has_right = (state.h_scroll + capacity) < content_len;
        if has_right {
            capacity = capacity.saturating_sub(1);
        }

        let start = state.h_scroll.min(content_len);
        let end = (state.h_scroll + capacity).min(content_len);
        (start, end, has_left, has_right)
    };

    if has_left {
        if cursor_out_of_view_left {
            spans.push(Span::styled(
                "◀",
                Style::default().fg(RatColor::White).add_modifier(Modifier::BOLD).patch(base_bg),
            ));
        } else {
            spans.push(Span::styled("…", Style::default().fg(RatColor::DarkGray).patch(base_bg)));
        }
    }

    let mut current_char_offset = 0;
    for seg in &log.segments {
        let seg_len = seg.text.chars().count();
        let seg_start = current_char_offset;
        let seg_end = current_char_offset + seg_len;
        current_char_offset += seg_len;

        if seg_end <= h_start || seg_start >= h_end {
            continue;
        }

        let take_start = h_start.saturating_sub(seg_start);
        let take_len = (h_end.min(seg_end) - seg_start).saturating_sub(take_start);

        let slice: String = seg.text.chars().skip(take_start).take(take_len).collect();
        let slice_global_start = seg_start + take_start;

        let seg_style = if is_dimmed {
            Style::default().fg(dim_fg).patch(base_bg)
        } else if seg.style.bg.is_some() {
            seg.style
        } else {
            seg.style.patch(base_bg)
        };

        match sel_range {
            None => {
                spans.push(Span::styled(slice, seg_style));
            }
            Some((sel_start, sel_end)) => {
                for (char_i, c) in slice.chars().enumerate() {
                    let actual_idx = prefix_chars + slice_global_start + char_i;
                    if actual_idx >= sel_start && actual_idx <= sel_end {
                        spans.push(Span::styled(c.to_string(), sel_style));
                    } else {
                        spans.push(Span::styled(c.to_string(), seg_style));
                    }
                }
            }
        }
    }

    if has_right {
        spans.push(Span::styled("…", Style::default().fg(RatColor::DarkGray).patch(base_bg)));
    }

    spans
}

fn render_logs(
    frame: &mut Frame,
    area: Rect,
    state: &mut AppState,
    theme: &UiTheme,
    color_mode: &TerminalColorMode,
) {
    state.viewport_height = area.height as usize;
    state.viewport_width = area.width as usize;

    let sel_style = compute_neutral_selection_style(color_mode);
    let active_line_style = compute_neutral_active_line_style(color_mode);
    let ref_now_secs = state.reference_timestamp_secs();

    let mut rendered_lines = Vec::new();
    let count = state.visible_count();
    let mut cursor_screen_pos: Option<(u16, u16)> = None;

    if state.line_fold {
        // Folded Mode: exactly 1 physical line per log item with horizontal scroll
        let visible_range = state.scroll_offset..count.min(state.scroll_offset + state.viewport_height);

        for (screen_line_idx, row_idx) in visible_range.enumerate() {
            let is_current = state.visual_mode == VisualMode::None && row_idx == state.cursor_row;
            let is_toggled = state
                .visible_indices
                .get(row_idx)
                .map(|idx| state.toggled_lines.contains(idx))
                .unwrap_or(false);

            if let Some(log) = state.get_visible_log(row_idx) {
                let is_dimmed = if let Some(ref live_q) = state.live_filter_ast {
                    !live_q.matches(log, ref_now_secs)
                } else {
                    false
                };

                let mut spans = build_styled_log_spans(
                    log,
                    row_idx,
                    state,
                    theme,
                    sel_style,
                    is_current,
                    is_toggled,
                    is_dimmed,
                    active_line_style,
                );

                if is_current {
                    let current_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
                    if current_width < state.viewport_width {
                        let padding = " ".repeat(state.viewport_width - current_width);
                        spans.push(Span::styled(padding, active_line_style));
                    }

                    // Cursor calculation for single line
                    let p_len = log.prefix_len();
                    let p_str = log.tag_text();
                    let p_display_width = str_display_width_up_to(&p_str, p_len);

                    let content_chars_count = log.content.chars().count();
                    let effective_cursor = state.cursor_col.saturating_sub(p_len).min(content_chars_count);

                    let cx = if state.h_scroll > 0 && effective_cursor < state.h_scroll {
                        area.x + p_display_width as u16
                    } else if state.cursor_col < p_len {
                        area.x + str_display_width_up_to(&p_str, state.cursor_col) as u16
                    } else {
                        let rel_idx = effective_cursor.saturating_sub(state.h_scroll);
                        let c_slice: String = log.content.chars().skip(state.h_scroll).take(rel_idx).collect();
                        let left_pad = if state.h_scroll > 0 && content_chars_count > 0 { 1 } else { 0 };
                        let total = area.x + (p_display_width + left_pad + str_display_width_up_to(&c_slice, rel_idx)) as u16;
                        total.min(area.right().saturating_sub(1))
                    };
                    cursor_screen_pos = Some((cx, area.y + screen_line_idx as u16));
                }

                rendered_lines.push(Line::from(spans));
            }
        }
    } else {
        // Unfolded Mode: wrapped lines with stationary tags and normalized multiline chunks
        let mut row_idx = state.scroll_offset;

        while row_idx < count && rendered_lines.len() < state.viewport_height {
            let is_current = state.visual_mode == VisualMode::None && row_idx == state.cursor_row;
            let is_toggled = state
                .visible_indices
                .get(row_idx)
                .map(|idx| state.toggled_lines.contains(idx))
                .unwrap_or(false);

            if let Some(log) = state.get_visible_log(row_idx) {
                let is_dimmed = if let Some(ref live_q) = state.live_filter_ast {
                    !live_q.matches(log, ref_now_secs)
                } else {
                    false
                };

                let full_text = log.full_line_text();
                let total_chars = full_text.chars().count();
                let sel_range = get_line_selection_col_range(row_idx, state, total_chars);

                let is_line_in_line_sel = if state.visual_mode == VisualMode::Line {
                    if let Some((anchor_r, _)) = state.visual_anchor {
                        let min_r = anchor_r.min(state.cursor_row);
                        let max_r = anchor_r.max(state.cursor_row);
                        row_idx >= min_r && row_idx <= max_r
                    } else {
                        false
                    }
                } else {
                    false
                };

                let prefix = log.tag_text();
                let prefix_len = log.prefix_len();
                let content_width = get_unfolded_content_width(state.viewport_width, prefix_len);
                let chunks = get_log_visual_chunks(&log.content, content_width);

                let dim_fg = theme.dim.to_ratatui();
                let svc_color = if is_dimmed { dim_fg } else { get_service_color(&log.service) };
                let base_bg = if is_current {
                    active_line_style
                } else if is_toggled {
                    Style::default().bg(theme.selection_bg.to_ratatui())
                } else {
                    Style::default()
                };

                let cursor_in_content = state.cursor_col.saturating_sub(prefix_len);

                for (chunk_i, chunk) in chunks.iter().enumerate() {
                    if rendered_lines.len() >= state.viewport_height {
                        break;
                    }

                    let current_visual_y = rendered_lines.len() as u16;
                    let mut spans = Vec::new();

                    if chunk_i == 0 {
                        if is_toggled {
                            spans.push(Span::styled("✔ ", Style::default().fg(theme.status_active.to_ratatui()).patch(base_bg)));
                        }

                        // Tag is never highlighted by visual selection
                        let tag_style = if is_dimmed {
                            Style::default().fg(dim_fg).patch(base_bg)
                        } else {
                            Style::default().fg(svc_color).add_modifier(Modifier::BOLD).patch(base_bg)
                        };
                        spans.push(Span::styled(prefix.clone(), tag_style));
                    } else {
                        let indent_len = prefix_len + if is_toggled { 2 } else { 0 };
                        spans.push(Span::styled(" ".repeat(indent_len), base_bg));
                    }

                    let default_content_style = if is_dimmed {
                        Style::default().fg(dim_fg).patch(base_bg)
                    } else {
                        Style::default().fg(theme.fg.to_ratatui()).patch(base_bg)
                    };

                    let chunk_global_start = prefix_len + chunk.content_char_start;

                    // Highlight content characters matching the selection range
                    push_styled_slice_with_sel(
                        &mut spans,
                        &chunk.text,
                        chunk_global_start,
                        sel_range,
                        default_content_style,
                        sel_style,
                    );

                    // Show gray '↵' at the end of wrapping chunks (outside selection range)
                    if chunk.has_continuation {
                        spans.push(Span::styled(
                            " ↵",
                            Style::default().fg(RatColor::DarkGray).patch(base_bg),
                        ));
                    }

                    // Cursor calculation for the current chunk
                    let chunk_char_count = chunk.text.chars().count();
                    let is_cursor_in_this_chunk = is_current
                        && (cursor_in_content >= chunk.content_char_start
                            && (cursor_in_content < chunk.content_char_start + chunk_char_count
                                || chunk_i + 1 == chunks.len()));

                    if is_cursor_in_this_chunk {
                        let cx = if chunk_i == 0 && state.cursor_col < prefix_len {
                            (area.x + state.cursor_col as u16).min(area.right().saturating_sub(1))
                        } else {
                            let indent = prefix_len + if is_toggled { 2 } else { 0 };
                            let rel_col = cursor_in_content.saturating_sub(chunk.content_char_start);
                            let col_offset = str_display_width_up_to(&chunk.text, rel_col);
                            (area.x + (indent + col_offset) as u16).min(area.right().saturating_sub(1))
                        };
                        cursor_screen_pos = Some((cx, area.y + current_visual_y));
                    }

                    // Padding for visual line mode across remaining row width
                    let current_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
                    if current_width < state.viewport_width {
                        let padding = " ".repeat(state.viewport_width - current_width);
                        let pad_style = if is_line_in_line_sel {
                            sel_style
                        } else if is_current {
                            active_line_style
                        } else {
                            Style::default()
                        };
                        spans.push(Span::styled(padding, pad_style));
                    }

                    rendered_lines.push(Line::from(spans));
                }
            }
            row_idx += 1;
        }
    }

    let paragraph = Paragraph::new(rendered_lines);
    frame.render_widget(paragraph, area);

    // Strictly clamp and position the cursor inside the log viewport area
    if state.prompt_mode == PromptMode::None && !state.show_service_menu {
        if let Some((cx, cy)) = cursor_screen_pos {
            if cy >= area.y && cy < area.bottom() && cx >= area.x && cx < area.right() {
                frame.set_cursor_position(Position::new(cx, cy));
            }
        }
    }
}

fn get_line_selection_col_range(
    row: usize,
    state: &AppState,
    total_cols: usize,
) -> Option<(usize, usize)> {
    let (anchor_row, anchor_col) = state.visual_anchor?;
    if state.visual_mode == VisualMode::None || total_cols == 0 {
        return None;
    }

    match state.visual_mode {
        VisualMode::Line => {
            let min_r = anchor_row.min(state.cursor_row);
            let max_r = anchor_row.max(state.cursor_row);
            if row >= min_r && row <= max_r {
                let p_len = state.get_visible_log(row).map(|l| l.prefix_len()).unwrap_or(0);
                Some((p_len, total_cols.saturating_sub(1)))
            } else {
                None
            }
        }
        VisualMode::Block => {
            let min_r = anchor_row.min(state.cursor_row);
            let max_r = anchor_row.max(state.cursor_row);
            if row >= min_r && row <= max_r {
                let p_len = state.get_visible_log(row).map(|l| l.prefix_len()).unwrap_or(0);
                let min_c = anchor_col.min(state.cursor_col).max(p_len);
                let max_c = anchor_col.max(state.cursor_col).max(p_len);
                Some((min_c.min(total_cols.saturating_sub(1)), max_c.min(total_cols.saturating_sub(1))))
            } else {
                None
            }
        }
        VisualMode::Character => {
            let (start, end) = if (anchor_row, anchor_col) <= (state.cursor_row, state.cursor_col) {
                ((anchor_row, anchor_col), (state.cursor_row, state.cursor_col))
            } else {
                ((state.cursor_row, state.cursor_col), (anchor_row, anchor_col))
            };

            let p_len = state.get_visible_log(row).map(|l| l.prefix_len()).unwrap_or(0);

            if row < start.0 || row > end.0 {
                None
            } else if start.0 == end.0 {
                let s_col = start.1.max(p_len);
                let e_col = end.1.max(p_len);
                Some((s_col.min(total_cols.saturating_sub(1)), e_col.min(total_cols.saturating_sub(1))))
            } else if row == start.0 {
                let s_col = start.1.max(p_len);
                Some((s_col.min(total_cols.saturating_sub(1)), total_cols.saturating_sub(1)))
            } else if row == end.0 {
                Some((p_len, end.1.max(p_len).min(total_cols.saturating_sub(1))))
            } else {
                Some((p_len, total_cols.saturating_sub(1)))
            }
        }
        VisualMode::None => None,
    }
}

fn format_time_hh_mm(ts: &str) -> Option<String> {
    let clean = ts.trim_end_matches('Z');
    let (_, time_part) = clean.split_once('T').or_else(|| clean.split_once(' '))?;
    let mut parts = time_part.split(':');
    let h = parts.next()?;
    let m = parts.next()?;
    Some(format!("{}:{}", h, m))
}

fn format_time_hh_mm_ss(ts: &str) -> Option<String> {
    let clean = ts.trim_end_matches('Z');
    let (_, time_part) = clean.split_once('T').or_else(|| clean.split_once(' '))?;
    let sec_clean = time_part.split('.').next()?;
    Some(sec_clean.to_string())
}

/// Status / Hint bar: keybindings on left, time/timer indicator and line count on right
fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState, theme: &UiTheme) {
    let key_style = Style::default().fg(RatColor::White).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(RatColor::DarkGray);

    let left_spans = if state.prompt_mode == PromptMode::Filter {
        vec![
            Span::styled("hints: ", Style::default().fg(theme.dim.to_ratatui()).add_modifier(Modifier::BOLD)),
            Span::styled("\"err\" or 'warn' | `ipv4` | context 3 | dedup | dedup all | since 5m", Style::default().fg(theme.dim.to_ratatui())),
        ]
    } else {
        vec![
            Span::styled("C-s", key_style),
            Span::styled(" services  ", desc_style),
            Span::styled("C-f", key_style),
            Span::styled(" filter  ", desc_style),
            Span::styled("z", key_style),
            Span::styled(if state.line_fold { " unfold  " } else { " fold    " }, desc_style),
            Span::styled("?", key_style),
            Span::styled(" help   ", desc_style),
        ]
    };

    let time_info_str = if state.visual_mode != VisualMode::None {
        if let Some((anchor_r, _)) = state.visual_anchor {
            let r1 = anchor_r.min(state.cursor_row);
            let r2 = anchor_r.max(state.cursor_row);
            let log1 = state.get_visible_log(r1);
            let log2 = state.get_visible_log(r2);

            match (log1.and_then(|l| l.timestamp.as_deref()), log2.and_then(|l| l.timestamp.as_deref())) {
                (Some(ts1), Some(ts2)) => {
                    let s1 = parse_timestamp_secs(ts1).unwrap_or(0);
                    let s2 = parse_timestamp_secs(ts2).unwrap_or(0);
                    let diff = s2.abs_diff(s1);

                    let formatted_diff = if diff < 60 {
                        format!("{}s", diff)
                    } else if diff < 3600 {
                        format!("{}m", diff / 60)
                    } else if diff < 86400 {
                        let h = diff / 3600;
                        let m = (diff % 3600) / 60;
                        if m == 0 { format!("{}h", h) } else { format!("{}h {}m", h, m) }
                    } else {
                        let d = diff / 86400;
                        let h = (diff % 86400) / 3600;
                        format!("{}d {}h", d, h)
                    };

                    let (t_start, t_end) = if diff < 60 {
                        (
                            format_time_hh_mm_ss(ts1).unwrap_or_else(|| "00:00:00".into()),
                            format_time_hh_mm_ss(ts2).unwrap_or_else(|| "00:00:00".into()),
                        )
                    } else {
                        (
                            format_time_hh_mm(ts1).unwrap_or_else(|| "00:00".into()),
                            format_time_hh_mm(ts2).unwrap_or_else(|| "00:00".into()),
                        )
                    };

                    Some(format!("\u{f13ab} {} → {} ({})  ", t_start, t_end, formatted_diff))
                }
                _ => None,
            }
        } else {
            None
        }
    } else if let Some(log) = state.get_visible_log(state.cursor_row) {
        if let Some(ref ts) = log.timestamp {
            if let Some(log_secs) = parse_timestamp_secs(ts) {
                let wall_now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let ref_now = state.reference_timestamp_secs().max(wall_now);
                let diff = ref_now.saturating_sub(log_secs);
                let formatted = if diff < 60 {
                    format!("{}s", diff)
                } else if diff < 3600 {
                    format!("{}m", diff / 60)
                } else if diff < 86400 {
                    format!("{}h", diff / 3600)
                } else {
                    format!("{}d", diff / 86400)
                };
                Some(format!("\u{f144e} {}  ", formatted))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let line_indicator = format!(
        "  {}:{} / {} ",
        state.cursor_row + 1,
        state.cursor_col + 1,
        state.visible_count()
    );

    let max_scroll = state.visible_count().saturating_sub(state.viewport_height);
    let is_at_bottom = state.scroll_offset >= max_scroll;

    let indicator_style = if !is_at_bottom && state.visible_count() > 0 {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default().fg(RatColor::DarkGray)
    };

    let mut right_spans = Vec::new();
    if let Some(ref time_str) = time_info_str {
        right_spans.push(Span::styled(
            time_str.clone(),
            Style::default().fg(RatColor::LightCyan).add_modifier(Modifier::BOLD),
        ));
    }
    right_spans.push(Span::styled(line_indicator.clone(), indicator_style));

    let right_width = (time_info_str.as_ref().map_or(0, |s| s.chars().count()) + line_indicator.chars().count()) as u16;
    let left_width = area.width.saturating_sub(right_width);

    let left_area = Rect {
        x: area.x,
        y: area.y,
        width: left_width,
        height: 1,
    };
    frame.render_widget(Paragraph::new(Line::from(left_spans)), left_area);

    let right_area = Rect {
        x: area.right().saturating_sub(right_width),
        y: area.y,
        width: right_width,
        height: 1,
    };
    frame.render_widget(Paragraph::new(Line::from(right_spans)), right_area);
}

/// Command line: displays the active filter or the interactive filter prompt with "filter: "
fn render_command_line(frame: &mut Frame, area: Rect, state: &AppState, _theme: &UiTheme) {
    if state.prompt_mode == PromptMode::Filter {
        let mut spans = vec![
            Span::styled(
                "filter: ",
                Style::default().fg(RatColor::White).add_modifier(Modifier::BOLD),
            ),
        ];

        let colored_tokens = colorize_filter_query(&state.input_buffer);
        spans.extend(colored_tokens);

        frame.render_widget(Paragraph::new(Line::from(spans)), area);

        let cursor_offset = str_display_width_up_to(&state.input_buffer, state.filter_cursor_col);
        let cursor_x = area.x + 8 + cursor_offset as u16;
        frame.set_cursor_position(Position::new(
            cursor_x.min(area.right().saturating_sub(1)),
            area.y,
        ));
    } else if let Some(ref active) = state.active_filter_str {
        let is_valid = Query::parse(active).is_ok();
        let mut spans = vec![
            Span::styled(
                "filter: ",
                Style::default().fg(if is_valid { RatColor::Yellow } else { RatColor::Red }).add_modifier(Modifier::BOLD),
            ),
        ];

        if is_valid {
            spans.extend(colorize_filter_query(active));
        } else {
            spans.push(Span::styled(
                format!("[invalid: {}]", active),
                Style::default().fg(RatColor::Red).add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            ));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

fn is_quote_char(c: char) -> bool {
    c == '"' || c == '\'' || c == '`'
}

fn consume_quoted_span(chars: &[char], i: &mut usize, delimiter: char) -> String {
    let mut s = String::new();
    s.push(delimiter);
    while *i < chars.len() {
        let c = chars[*i];
        if c == '\\' && *i + 1 < chars.len() {
            s.push(c);
            s.push(chars[*i + 1]);
            *i += 2;
            continue;
        }
        s.push(c);
        *i += 1;
        if c == delimiter {
            break;
        }
    }
    s
}

fn colorize_filter_query(query: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let chars: Vec<char> = query.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i].is_whitespace() {
            spans.push(Span::raw(" "));
            i += 1;
            continue;
        }

        if chars[i] == '|' {
            spans.push(Span::styled("|", Style::default().fg(RatColor::White).add_modifier(Modifier::BOLD)));
            i += 1;
            continue;
        }

        if chars[i] == '(' || chars[i] == ')' {
            spans.push(Span::styled(chars[i].to_string(), Style::default().fg(RatColor::Yellow)));
            i += 1;
            continue;
        }

        let rem = &chars[i..];

        // Case-sensitive regex: sr"...", sr'...', sr`...`
        if rem.len() >= 3 && rem[0] == 's' && rem[1] == 'r' && is_quote_char(rem[2]) {
            let delimiter = rem[2];
            i += 3;
            let inner = consume_quoted_span(&chars, &mut i, delimiter);
            spans.push(Span::styled(format!("sr{}", inner), Style::default().fg(RatColor::Cyan).add_modifier(Modifier::BOLD)));
            continue;
        }

        // Case-insensitive regex: r"...", r'...', r`...`
        if rem.len() >= 2 && rem[0] == 'r' && is_quote_char(rem[1]) {
            let delimiter = rem[1];
            i += 2;
            let inner = consume_quoted_span(&chars, &mut i, delimiter);
            spans.push(Span::styled(format!("r{}", inner), Style::default().fg(RatColor::LightCyan)));
            continue;
        }

        // Case-sensitive glob: s~"...", s~'...', s~`...`
        if rem.len() >= 3 && rem[0] == 's' && rem[1] == '~' && is_quote_char(rem[2]) {
            let delimiter = rem[2];
            i += 3;
            let inner = consume_quoted_span(&chars, &mut i, delimiter);
            spans.push(Span::styled(format!("s~{}", inner), Style::default().fg(RatColor::Magenta).add_modifier(Modifier::BOLD)));
            continue;
        }

        // Case-insensitive glob: ~"...", ~'...', ~`...`
        if rem.len() >= 2 && rem[0] == '~' && is_quote_char(rem[1]) {
            let delimiter = rem[1];
            i += 2;
            let inner = consume_quoted_span(&chars, &mut i, delimiter);
            spans.push(Span::styled(format!("~{}", inner), Style::default().fg(RatColor::LightMagenta)));
            continue;
        }

        // Case-sensitive literal: s"...", s'...', s`...`
        if rem.len() >= 2 && rem[0] == 's' && is_quote_char(rem[1]) {
            let delimiter = rem[1];
            i += 2;
            let inner = consume_quoted_span(&chars, &mut i, delimiter);
            spans.push(Span::styled(format!("s{}", inner), Style::default().fg(RatColor::Green).add_modifier(Modifier::BOLD)));
            continue;
        }

        // Case-insensitive literal: "...", '...', `...`
        if is_quote_char(chars[i]) {
            let delimiter = chars[i];
            i += 1;
            let inner = consume_quoted_span(&chars, &mut i, delimiter);
            spans.push(Span::styled(inner, Style::default().fg(RatColor::LightGreen)));
            continue;
        }

        let mut term = String::new();
        while i < len && !chars[i].is_whitespace() && chars[i] != '(' && chars[i] != ')' && chars[i] != '|' {
            term.push(chars[i]);
            i += 1;
        }

        let lower = term.to_lowercase();
        if lower == "and" || lower == "or" {
            spans.push(Span::styled(term, Style::default().fg(RatColor::LightRed).add_modifier(Modifier::BOLD)));
        } else if lower == "not" {
            spans.push(Span::styled(term, Style::default().fg(RatColor::Red).add_modifier(Modifier::BOLD)));
        } else if lower == "since" || lower == "before" {
            spans.push(Span::styled(term, Style::default().fg(RatColor::LightCyan).add_modifier(Modifier::BOLD)));
        } else if lower == "context" || lower == "after" {
            spans.push(Span::styled(term, Style::default().fg(RatColor::LightYellow).add_modifier(Modifier::BOLD)));
        } else if lower == "first" || lower == "last" {
            spans.push(Span::styled(term, Style::default().fg(RatColor::LightBlue).add_modifier(Modifier::BOLD)));
        } else if lower == "dedup" || lower == "deduplicate" {
            spans.push(Span::styled(term, Style::default().fg(RatColor::Magenta).add_modifier(Modifier::BOLD)));
        } else if lower == "all" || term.chars().all(|c| c.is_ascii_digit()) || parse_duration(&term).is_some() {
            spans.push(Span::styled(term, Style::default().fg(RatColor::White)));
        } else {
            // Unquoted invalid token: underline in bold red
            spans.push(Span::styled(
                term,
                Style::default().fg(RatColor::Red).add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            ));
        }
    }

    spans
}

fn render_service_menu(frame: &mut Frame, parent_area: Rect, state: &mut AppState, theme: &UiTheme) {
    let menu_width = 46u16.min(parent_area.width);
    let menu_height = 14u16.min(parent_area.height);

    let menu_area = Rect {
        x: parent_area.x,
        y: parent_area.bottom().saturating_sub(menu_height),
        width: menu_width,
        height: menu_height,
    };

    frame.render_widget(Clear, menu_area);

    let main_block = Block::default()
        .borders(Borders::ALL)
        .title(" Services ")
        .border_style(Style::default().fg(RatColor::Gray));

    let inner = main_block.inner(menu_area);
    frame.render_widget(main_block, menu_area);

    let constraints = if state.show_service_search {
        vec![
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(2),
        ]
    } else {
        vec![
            Constraint::Min(1),
            Constraint::Length(1),
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let filtered_indices = state.filtered_service_indices();
    let list_height = chunks[0].height as usize;
    let list_width = chunks[0].width as usize;

    if filtered_indices.is_empty() {
        let empty_lines = chunks[0].height.saturating_sub(1) / 2;
        let mut lines = Vec::new();
        for _ in 0..empty_lines {
            lines.push(Line::raw(""));
        }
        lines.push(
            Line::from(vec![Span::styled(
                "No results",
                Style::default().fg(theme.dim.to_ratatui()),
            )])
            .alignment(Alignment::Center),
        );
        frame.render_widget(Paragraph::new(lines), chunks[0]);
    } else {
        let margin = 2.min(list_height.saturating_sub(1) / 2);

        if state.service_menu_selected_idx < state.service_menu_scroll + margin {
            state.service_menu_scroll = state.service_menu_selected_idx.saturating_sub(margin);
        }
        if state.service_menu_selected_idx + margin >= state.service_menu_scroll + list_height {
            state.service_menu_scroll = (state.service_menu_selected_idx + margin + 1).saturating_sub(list_height);
        }

        let max_scroll = filtered_indices.len().saturating_sub(list_height);
        if state.service_menu_scroll > max_scroll {
            state.service_menu_scroll = max_scroll;
        }

        let start = state.service_menu_scroll;
        let end = (start + list_height).min(filtered_indices.len());

        let mut list_lines = Vec::new();
        for item_idx in start..end {
            let actual_service_idx = filtered_indices[item_idx];
            let opt = &state.service_options[actual_service_idx];
            let is_selected = item_idx == state.service_menu_selected_idx;

            let row_bg = if is_selected {
                Style::default().bg(RatColor::DarkGray)
            } else {
                Style::default()
            };

            let name_fg = if is_selected {
                RatColor::White
            } else {
                theme.fg.to_ratatui()
            };

            let dot_color = if opt.has_error {
                theme.status_error.to_ratatui()
            } else if opt.is_running {
                theme.status_active.to_ratatui()
            } else {
                theme.dim.to_ratatui()
            };

            let check_color = theme.status_active.to_ratatui();

            let check_span = if opt.enabled {
                Span::styled(
                    "✔ ",
                    Style::default().fg(check_color).add_modifier(Modifier::BOLD).patch(row_bg),
                )
            } else {
                Span::styled("  ", row_bg)
            };

            let check_len = 2;
            let dot_len = 1;
            let space_for_name = list_width.saturating_sub(check_len + dot_len + 1);
            let disp_name = truncate_service(&opt.name, space_for_name);
            let padding_len = space_for_name.saturating_sub(disp_name.chars().count()) + 1;
            let padding = " ".repeat(padding_len);

            let name_span = Span::styled(
                disp_name,
                Style::default().fg(name_fg).add_modifier(if is_selected { Modifier::BOLD } else { Modifier::empty() }).patch(row_bg),
            );
            let pad_span = Span::styled(padding, row_bg);

            let dot_span = Span::styled(
                "●",
                Style::default().fg(dot_color).add_modifier(Modifier::BOLD).patch(row_bg),
            );

            list_lines.push(Line::from(vec![check_span, name_span, pad_span, dot_span]));
        }

        frame.render_widget(Paragraph::new(list_lines), chunks[0]);
    }

    let footer = Line::from(vec![
        Span::styled("C-x", Style::default().fg(RatColor::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Unselect all  ", Style::default().fg(theme.dim.to_ratatui())),
        Span::styled("C-a", Style::default().fg(RatColor::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Select all", Style::default().fg(theme.dim.to_ratatui())),
    ]);
    frame.render_widget(Paragraph::new(footer), chunks[1]);

    if state.show_service_search {
        let search_block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.dim.to_ratatui()));

        let search_bar = Paragraph::new(Line::from(vec![
            Span::styled("Search: ", Style::default().fg(theme.dim.to_ratatui())),
            Span::styled(&state.service_search_query, Style::default().fg(theme.fg.to_ratatui())),
        ]))
        .block(search_block);

        frame.render_widget(search_bar, chunks[2]);

        let search_cursor_x = chunks[2].x + 8 + state.service_search_query.chars().count() as u16;
        let search_cursor_y = chunks[2].y + 1;
        frame.set_cursor_position(Position::new(
            search_cursor_x.min(chunks[2].right().saturating_sub(1)),
            search_cursor_y,
        ));
    }
}
