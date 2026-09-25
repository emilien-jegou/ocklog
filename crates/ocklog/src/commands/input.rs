use crate::libs::docker::DockerEngine;
use crate::services::filter::prompt_state::PromptMode;
use crate::services::ServiceRegistry;
use crate::ui::state::selection::VisualMode;
use crate::ui::state::AppUiState;
use crate::ui::views::log_viewport::line_composer::LineComposer;
use crate::utils::clipboard::Clipboard;
use crate::utils::text::TextUtils;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::collections::HashMap;
use std::sync::Arc;

pub struct KeyDispatchContext<'a> {
    pub ui: &'a mut AppUiState,
    pub services: &'a mut ServiceRegistry,
    pub docker: &'a Arc<DockerEngine>,
    pub pending_g: &'a mut bool,
}

pub struct InputDispatcher;

impl InputDispatcher {
    pub fn dispatch_key(event: KeyEvent, mut ctx: KeyDispatchContext<'_>) -> bool {
        if Self::handle_escape(&event, &mut ctx) {
            return false;
        }
        if ctx.services.filter.prompt().mode == PromptMode::Active {
            Self::handle_filter_key(event, ctx);
            return false;
        }
        if ctx.ui.picker.is_open {
            Self::handle_picker_key(event, ctx);
            return false;
        }
        Self::handle_vim_key(event, ctx)
    }

    pub fn dispatch_mouse(mouse: MouseEvent, ui: &mut AppUiState, services: &mut ServiceRegistry) {
        let total = services.logging.buffer().visible_count();
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let (r, c, on_tag) = Self::screen_to_buffer_pos(mouse.column as usize, mouse.row as usize, ui, services);
                if on_tag {
                    ui.mouse_mode = VisualMode::Line;
                    ui.mouse_anchor = Some((r, 0));
                    ui.mouse_head = Some((r, 0));
                } else {
                    ui.mouse_mode = VisualMode::Character;
                    ui.mouse_anchor = Some((r, c));
                    ui.mouse_head = Some((r, c));
                }
                ui.is_mouse_dragging = false;
                ui.last_mouse_pos = Some((mouse.column, mouse.row));
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let (r, c, _) = Self::screen_to_buffer_pos(mouse.column as usize, mouse.row as usize, ui, services);
                ui.mouse_head = Some((r, c));
                ui.is_mouse_dragging = true;
                ui.last_mouse_pos = Some((mouse.column, mouse.row));
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if ui.is_mouse_dragging && ui.mouse_mode != VisualMode::None {
                    Self::yank_mouse_selection(ui, services);
                }
                ui.clear_mouse_selection();

                let is_cursor_visible = ui.cursor_row >= ui.scroll_offset && ui.cursor_row < ui.scroll_offset + ui.viewport_height;
                if !is_cursor_visible {
                    ui.adjust_scroll(total);
                }
            }
            MouseEventKind::ScrollDown => {
                ui.scroll_viewport_down(3, total);
            }
            MouseEventKind::ScrollUp => {
                ui.scroll_viewport_up(3, total);
            }
            _ => {}
        }
    }

    fn screen_to_buffer_pos(
        screen_x: usize,
        screen_y: usize,
        ui: &AppUiState,
        services: &ServiceRegistry,
    ) -> (usize, usize, bool) {
        let total = services.logging.buffer().visible_count();
        if total == 0 {
            return (0, 0, false);
        }

        if ui.line_fold {
            let r = (ui.scroll_offset + screen_y).min(total.saturating_sub(1));
            let log = match services.logging.buffer().get_visible(r) {
                Some(l) => l,
                None => return (r, 0, false),
            };

            if log.is_system {
                let char_offset = TextUtils::char_index_at_width(&log.content, screen_x);
                return (r, char_offset.min(log.content.chars().count()), false);
            }

            let p_str = log.tag_text();
            let p_width = TextUtils::display_width_up_to(&p_str, p_str.chars().count()) + if ui.toggled_lines.contains(&r) { 2 } else { 0 };

            if screen_x <= p_width {
                return (r, 0, true);
            }

            let left_pad = if ui.h_scroll > 0 && !log.content.is_empty() { 1 } else { 0 };
            let content_screen_x = screen_x.saturating_sub(p_width + left_pad);
            let content_slice: String = log.content.chars().skip(ui.h_scroll).collect();
            let char_offset = TextUtils::char_index_at_width(&content_slice, content_screen_x);
            let content_col = (ui.h_scroll + char_offset).min(log.content.chars().count());

            (r, content_col, false)
        } else {
            let mut v_line = 0;
            for r in ui.scroll_offset..total {
                if let Some(log) = services.logging.buffer().get_visible(r) {
                    let p_len = log.prefix_len();
                    let content_w = ui.viewport_width.saturating_sub(p_len + 2).max(1);
                    let chunks = LineComposer::get_unfolded_chunks(&log.content, content_w);
                    let is_toggled = ui.toggled_lines.contains(&r);
                    let p_str = log.tag_text();
                    let tag_width = TextUtils::display_width_up_to(&p_str, p_str.chars().count()) + if is_toggled { 2 } else { 0 };

                    for (chunk_i, chunk) in chunks.iter().enumerate() {
                        if v_line == screen_y {
                            if chunk_i == 0 && screen_x <= tag_width {
                                return (r, 0, true);
                            }
                            let indent = if chunk_i == 0 { tag_width } else { p_len + if is_toggled { 2 } else { 0 } };
                            let chunk_rel_x = screen_x.saturating_sub(indent);
                            let char_offset = TextUtils::char_index_at_width(&chunk.text, chunk_rel_x);
                            let col = (chunk.content_char_start + char_offset).min(log.content.chars().count());
                            return (r, col, false);
                        }
                        v_line += 1;
                    }
                }
            }
            (ui.scroll_offset, 0, false)
        }
    }

    fn handle_escape(ev: &KeyEvent, ctx: &mut KeyDispatchContext<'_>) -> bool {
        if matches!((ev.code, ev.modifiers), (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL)) {
            *ctx.pending_g = false;
            if ctx.services.filter.prompt().mode == PromptMode::Active {
                ctx.services.filter.prompt_mut().deactivate();
            } else if ctx.ui.picker.is_open {
                ctx.ui.picker.is_open = false;
            } else {
                ctx.ui.exit_visual();
                ctx.ui.clear_mouse_selection();
            }
            return true;
        }
        false
    }

    fn handle_filter_key(ev: KeyEvent, ctx: KeyDispatchContext<'_>) {
        let prompt = ctx.services.filter.prompt_mut();
        match (ev.code, ev.modifiers) {
            (KeyCode::Enter, _) => {
                let _ = ctx.services.filter.submit_prompt();
                Self::reapply_active_filter(ctx);
            }
            (KeyCode::Left, _) => prompt.move_cursor_left(),
            (KeyCode::Right, _) => prompt.move_cursor_right(),
            (KeyCode::Home, _) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => prompt.cursor_col = 0,
            (KeyCode::End, _) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => prompt.cursor_col = prompt.input_buffer.chars().count(),
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                prompt.input_buffer.clear();
                prompt.cursor_col = 0;
            }
            (KeyCode::Backspace, _) => prompt.backspace(),
            (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => prompt.insert_char(c),
            _ => {}
        }
    }

    fn handle_picker_key(ev: KeyEvent, ctx: KeyDispatchContext<'_>) {
        if matches!((ev.code, ev.modifiers), (KeyCode::Char('s'), KeyModifiers::CONTROL) | (KeyCode::Char('q'), KeyModifiers::NONE)) {
            ctx.ui.picker.is_open = false;
            return;
        }

        if ctx.ui.picker.is_searching {
            match (ev.code, ev.modifiers) {
                (KeyCode::Char('f'), KeyModifiers::CONTROL) => ctx.ui.picker.is_searching = false,
                (KeyCode::Backspace, _) => { ctx.ui.picker.search_query.pop(); }
                (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => { ctx.ui.picker.search_query.push(c); }
                _ => {}
            }
            return;
        }

        let filtered_len = ctx.ui.picker.filtered_indices().len();

        match (ev.code, ev.modifiers) {
            (KeyCode::Char('f'), KeyModifiers::CONTROL) => ctx.ui.picker.is_searching = true,
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                *ctx.pending_g = false;
                ctx.ui.picker.select_next();
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                *ctx.pending_g = false;
                ctx.ui.picker.select_prev();
            }
            (KeyCode::Char('G'), _) => {
                *ctx.pending_g = false;
                if filtered_len > 0 {
                    ctx.ui.picker.selected_idx = filtered_len - 1;
                }
            }
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                if *ctx.pending_g {
                    ctx.ui.picker.selected_idx = 0;
                    *ctx.pending_g = false;
                } else {
                    *ctx.pending_g = true;
                }
            }
            (KeyCode::Char(' '), _) | (KeyCode::Enter, KeyModifiers::NONE) => {
                *ctx.pending_g = false;
                ctx.ui.picker.toggle_selected();
                Self::reapply_active_filter(ctx);
            }
            (KeyCode::Char('o'), KeyModifiers::CONTROL) | (KeyCode::Enter, KeyModifiers::CONTROL) => {
                *ctx.pending_g = false;
                ctx.ui.picker.select_only_current();
                Self::reapply_active_filter(ctx);
            }
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                *ctx.pending_g = false;
                ctx.ui.picker.set_all(true);
                Self::reapply_active_filter(ctx);
            }
            (KeyCode::Char('x'), KeyModifiers::CONTROL) => {
                *ctx.pending_g = false;
                ctx.ui.picker.set_all(false);
                Self::reapply_active_filter(ctx);
            }
            (KeyCode::Char('r'), KeyModifiers::NONE) => {
                Self::dispatch_container_action(ctx, |c, id| async move { let _ = c.restart(&id).await; });
            }
            (KeyCode::Char('s'), KeyModifiers::NONE) => {
                Self::dispatch_container_action(ctx, |c, id| async move { let _ = c.stop(&id).await; });
            }
            (KeyCode::Char('x'), KeyModifiers::NONE) => {
                Self::dispatch_container_action(ctx, |c, id| async move { let _ = c.kill(&id).await; });
            }
            _ => {
                *ctx.pending_g = false;
            }
        }
    }

    fn dispatch_container_action<F, Fut>(ctx: KeyDispatchContext<'_>, action: F)
    where
        F: FnOnce(crate::libs::docker::ContainerEngine, String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let filtered = ctx.ui.picker.filtered_indices();
        if let Some(&actual_idx) = filtered.get(ctx.ui.picker.selected_idx) {
            if let Some(opt) = ctx.ui.picker.options.get(actual_idx) {
                let id = opt.id.clone();
                let engine = ctx.docker.containers();
                tokio::spawn(async move {
                    action(engine, id).await;
                });
            }
        }
    }

    fn handle_vim_key(ev: KeyEvent, ctx: KeyDispatchContext<'_>) -> bool {
        let total = ctx.services.logging.buffer().visible_count();
        let max_content_col = ctx.services.logging.buffer().get_visible(ctx.ui.cursor_row).map_or(0, |l| l.content.chars().count());

        let is_shift = ev.modifiers.contains(KeyModifiers::SHIFT);
        let has_ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);

        match ev.code {
            KeyCode::Char('H') if is_shift || has_ctrl => {
                ctx.ui.pan_viewport_left(5);
                return false;
            }
            KeyCode::Char('h') if has_ctrl && is_shift => {
                ctx.ui.pan_viewport_left(5);
                return false;
            }
            KeyCode::Char('L') if is_shift || has_ctrl => {
                ctx.ui.pan_viewport_right(5, max_content_col);
                return false;
            }
            KeyCode::Char('l') if has_ctrl && is_shift => {
                ctx.ui.pan_viewport_right(5, max_content_col);
                return false;
            }
            KeyCode::Char('J') if is_shift || has_ctrl => {
                ctx.ui.pan_viewport_down(3, total);
                Self::snap_vertical_cursor(ctx);
                return false;
            }
            KeyCode::Char('j') if has_ctrl && is_shift => {
                ctx.ui.pan_viewport_down(3, total);
                Self::snap_vertical_cursor(ctx);
                return false;
            }
            KeyCode::Char('K') if is_shift || has_ctrl => {
                ctx.ui.pan_viewport_up(3);
                Self::snap_vertical_cursor(ctx);
                return false;
            }
            KeyCode::Char('k') if has_ctrl && is_shift => {
                ctx.ui.pan_viewport_up(3);
                Self::snap_vertical_cursor(ctx);
                return false;
            }
            _ => {}
        }

        match (ev.code, ev.modifiers) {
            (KeyCode::Char('q'), KeyModifiers::NONE) => return true,
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => {
                ctx.ui.move_down(1, total);
                ctx.ui.sticky = ctx.ui.cursor_row + 1 >= total;
                Self::snap_vertical_cursor(ctx);
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => {
                ctx.ui.move_up(1, total);
                ctx.ui.sticky = false;
                Self::snap_vertical_cursor(ctx);
            }
            (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                ctx.ui.move_down(5, total);
                ctx.ui.sticky = ctx.ui.cursor_row + 1 >= total;
                Self::snap_vertical_cursor(ctx);
            }
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                ctx.ui.move_up(5, total);
                ctx.ui.sticky = false;
                Self::snap_vertical_cursor(ctx);
            }
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Left, _) => ctx.ui.move_left(1, max_content_col),
            (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Right, _) => ctx.ui.move_right(1, max_content_col),
            (KeyCode::Char('h'), KeyModifiers::CONTROL) => ctx.ui.move_left(5, max_content_col),
            (KeyCode::Char('l'), KeyModifiers::CONTROL) => ctx.ui.move_right(5, max_content_col),
            (KeyCode::Char('0'), KeyModifiers::NONE) => {
                ctx.ui.cursor_col = 0;
                ctx.ui.preferred_col = 0;
                ctx.ui.h_scroll = 0;
            }
            (KeyCode::Char('$'), _) => Self::jump_eol(ctx, max_content_col),
            (KeyCode::Char(' '), KeyModifiers::NONE) => Self::toggle_line(ctx),
            (KeyCode::Char('w'), KeyModifiers::NONE) => Self::jump_word(ctx, true, max_content_col),
            (KeyCode::Char('b'), KeyModifiers::NONE) => Self::jump_word(ctx, false, max_content_col),
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                ctx.ui.move_down(ctx.ui.viewport_height / 2, total);
                ctx.ui.sticky = ctx.ui.cursor_row + 1 >= total;
                Self::snap_vertical_cursor(ctx);
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                ctx.ui.move_up(ctx.ui.viewport_height / 2, total);
                ctx.ui.sticky = false;
                Self::snap_vertical_cursor(ctx);
            }
            (KeyCode::Char('G'), _) => {
                if total > 0 {
                    ctx.ui.cursor_row = total - 1;
                    ctx.ui.scroll_offset = total.saturating_sub(ctx.ui.viewport_height);
                    ctx.ui.sticky = true;
                    Self::snap_vertical_cursor(ctx);
                }
            }
            (KeyCode::Char('g'), KeyModifiers::NONE) => Self::handle_g(ctx),
            (KeyCode::Char('v'), KeyModifiers::NONE) => ctx.ui.toggle_visual(VisualMode::Character),
            (KeyCode::Char('V'), _) | (KeyCode::Char('v'), KeyModifiers::SHIFT) => ctx.ui.toggle_visual(VisualMode::Line),
            (KeyCode::Char('v'), KeyModifiers::CONTROL) => ctx.ui.toggle_visual(VisualMode::Block),
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => ctx.ui.picker.is_open = !ctx.ui.picker.is_open,
            (KeyCode::Char('f'), KeyModifiers::CONTROL) => ctx.services.filter.prompt_mut().activate(),
            (KeyCode::Char('z'), KeyModifiers::NONE) => {
                let logs: Vec<_> = (0..total).filter_map(|i| ctx.services.logging.buffer().get_visible(i)).collect();
                ctx.ui.toggle_line_fold_anchored(&logs);
            }
            (KeyCode::Char('y'), KeyModifiers::NONE) => { Self::yank_text(ctx.ui, ctx.services); ctx.ui.exit_visual(); }
            _ => {}
        }
        false
    }

    fn snap_vertical_cursor(ctx: KeyDispatchContext<'_>) {
        if let Some(log) = ctx.services.logging.buffer().get_visible(ctx.ui.cursor_row) {
            let line_len = log.content.chars().count();
            ctx.ui.snap_to_preferred_col_and_scroll_back(line_len);
        }
    }

    fn reapply_active_filter(ctx: KeyDispatchContext<'_>) {
        let anchor_ts = ctx.services.logging.buffer().get_visible(ctx.ui.cursor_row).and_then(|l| l.timestamp_rfc3339.clone());
        let anchor_screen_y = ctx.ui.cursor_row.saturating_sub(ctx.ui.scroll_offset);

        let ref_now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let svc_map: HashMap<String, bool> = ctx.ui.picker.options.iter().map(|o| (o.name.clone(), o.enabled)).collect();
        ctx.services.logging.apply_filter(ctx.services.filter.active_filter().map(|f| f.query()), &svc_map, ref_now);

        let new_total = ctx.services.logging.buffer().visible_count();
        if new_total == 0 {
            ctx.ui.cursor_row = 0;
            ctx.ui.scroll_offset = 0;
            return;
        }

        let new_cursor = if let Some(ref ts) = anchor_ts {
            Self::find_closest_log_index(ctx.services, ts, new_total)
        } else {
            ctx.ui.cursor_row.min(new_total - 1)
        };

        ctx.ui.cursor_row = new_cursor;
        ctx.ui.scroll_offset = new_cursor.saturating_sub(anchor_screen_y);
        ctx.ui.adjust_scroll(new_total);
    }

    fn find_closest_log_index(services: &ServiceRegistry, target_ts: &str, total: usize) -> usize {
        let mut best_idx = 0;
        let mut min_found = false;

        for idx in 0..total {
            if let Some(log) = services.logging.buffer().get_visible(idx) {
                if let Some(ref ts) = log.timestamp_rfc3339 {
                    if ts.as_str() == target_ts {
                        return idx;
                    }
                    if ts.as_str() > target_ts && !min_found {
                        best_idx = idx;
                        min_found = true;
                    }
                }
            }
        }
        best_idx.min(total - 1)
    }

    fn toggle_line(ctx: KeyDispatchContext<'_>) {
        if ctx.ui.toggled_lines.contains(&ctx.ui.cursor_row) {
            ctx.ui.toggled_lines.remove(&ctx.ui.cursor_row);
        } else {
            ctx.ui.toggled_lines.insert(ctx.ui.cursor_row);
        }
    }

    fn jump_eol(ctx: KeyDispatchContext<'_>, max_content_col: usize) {
        if max_content_col > 0 {
            ctx.ui.cursor_col = max_content_col.saturating_sub(1);
            ctx.ui.preferred_col = usize::MAX;
            ctx.ui.adjust_h_scroll(max_content_col);
        }
    }

    fn jump_word(ctx: KeyDispatchContext<'_>, fwd: bool, max_content_col: usize) {
        if let Some(log) = ctx.services.logging.buffer().get_visible(ctx.ui.cursor_row) {
            let chars: Vec<char> = log.content.chars().collect();
            let mut i = ctx.ui.cursor_col;
            if fwd {
                while i < chars.len() && chars[i].is_alphanumeric() { i += 1; }
                while i < chars.len() && !chars[i].is_alphanumeric() { i += 1; }
            } else {
                while i > 0 && !chars[i - 1].is_alphanumeric() { i -= 1; }
                while i > 0 && chars[i - 1].is_alphanumeric() { i -= 1; }
            }
            ctx.ui.cursor_col = i.min(chars.len().saturating_sub(1));
            ctx.ui.preferred_col = ctx.ui.cursor_col;
            ctx.ui.adjust_h_scroll(max_content_col);
        }
    }

    fn handle_g(ctx: KeyDispatchContext<'_>) {
        if *ctx.pending_g {
            ctx.ui.cursor_row = 0;
            ctx.ui.scroll_offset = 0;
            ctx.ui.sticky = false;
            *ctx.pending_g = false;
            Self::snap_vertical_cursor(ctx);
        } else {
            *ctx.pending_g = true;
        }
    }

    fn yank_text(ui: &mut AppUiState, services: &ServiceRegistry) {
        if !ui.toggled_lines.is_empty() {
            let mut sorted: Vec<usize> = ui.toggled_lines.iter().copied().collect();
            sorted.sort_unstable();
            let lines: Vec<String> = sorted.into_iter().filter_map(|r| services.logging.buffer().get_visible(r).map(|l| l.full_line_text())).collect();
            Clipboard::copy(&lines.join("\n"));
            ui.set_clipboard_notice("Copied toggled lines");
            ui.toggled_lines.clear();
            return;
        }

        let (anchor_r, anchor_c) = match ui.visual_anchor {
            Some(a) if ui.visual_mode != VisualMode::None => a,
            _ => {
                if let Some(log) = services.logging.buffer().get_visible(ui.cursor_row) {
                    Clipboard::copy(&log.full_line_text());
                    ui.set_clipboard_notice("Copied to clipboard");
                }
                return;
            }
        };

        let (r1, r2) = (anchor_r.min(ui.cursor_row), anchor_r.max(ui.cursor_row));
        let mut lines = Vec::new();

        for r in r1..=r2 {
            if let Some(l) = services.logging.buffer().get_visible(r) {
                let chars: Vec<char> = l.content.chars().collect();
                match ui.visual_mode {
                    VisualMode::Line => lines.push(l.full_line_text()),
                    VisualMode::Block => {
                        let (c1, c2) = (anchor_c.min(ui.cursor_col), anchor_c.max(ui.cursor_col));
                        let slice: String = chars.into_iter().skip(c1).take(c2.saturating_sub(c1) + 1).collect();
                        lines.push(slice);
                    }
                    VisualMode::Character => {
                        let (start, end) = if (anchor_r, anchor_c) <= (ui.cursor_row, ui.cursor_col) {
                            ((anchor_r, anchor_c), (ui.cursor_row, ui.cursor_col))
                        } else {
                            ((ui.cursor_row, ui.cursor_col), (anchor_r, anchor_c))
                        };
                        let slice = if start.0 == end.0 {
                            chars.into_iter().skip(start.1).take(end.1.saturating_sub(start.1) + 1).collect()
                        } else if r == start.0 {
                            let text: String = chars.into_iter().skip(start.1).collect();
                            if l.is_system { text } else { format!("{}{}", l.tag_text(), text) }
                        } else if r == end.0 {
                            let text: String = chars.into_iter().take(end.1 + 1).collect();
                            if l.is_system { text } else { format!("{}{}", l.tag_text(), text) }
                        } else {
                            l.full_line_text()
                        };
                        lines.push(slice);
                    }
                    VisualMode::None => {}
                }
            }
        }
        Clipboard::copy(&lines.join("\n"));
        ui.set_clipboard_notice("Copied to clipboard");
    }

    fn yank_mouse_selection(ui: &mut AppUiState, services: &ServiceRegistry) {
        let (anchor, head) = match (ui.mouse_anchor, ui.mouse_head) {
            (Some(a), Some(h)) => (a, h),
            _ => return,
        };

        let (r1, r2) = (anchor.0.min(head.0), anchor.0.max(head.0));
        let mut lines = Vec::new();

        for r in r1..=r2 {
            if let Some(l) = services.logging.buffer().get_visible(r) {
                let chars: Vec<char> = l.content.chars().collect();
                match ui.mouse_mode {
                    VisualMode::Line => lines.push(l.full_line_text()),
                    VisualMode::Character => {
                        let (start, end) = if anchor <= head { (anchor, head) } else { (head, anchor) };
                        let slice = if start.0 == end.0 {
                            chars.into_iter().skip(start.1).take(end.1.saturating_sub(start.1) + 1).collect()
                        } else if r == start.0 {
                            let text: String = chars.into_iter().skip(start.1).collect();
                            if l.is_system { text } else { format!("{}{}", l.tag_text(), text) }
                        } else if r == end.0 {
                            let text: String = chars.into_iter().take(end.1 + 1).collect();
                            if l.is_system { text } else { format!("{}{}", l.tag_text(), text) }
                        } else {
                            l.full_line_text()
                        };
                        lines.push(slice);
                    }
                    _ => {}
                }
            }
        }
        if !lines.is_empty() {
            Clipboard::copy(&lines.join("\n"));
            ui.set_clipboard_notice("Copied to clipboard");
        }
    }
}
