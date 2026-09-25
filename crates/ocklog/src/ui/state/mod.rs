pub mod selection;
pub mod service_picker;

use crate::services::container_logging::transformer::ProcessedLogRecord;
use crate::ui::views::log_viewport::line_composer::LineComposer;
use selection::VisualMode;
use service_picker::ServicePickerState;
use std::collections::HashSet;
use std::time::Instant;

#[allow(unused)]
pub struct AppUiState {
    pub viewport_height: usize,
    pub viewport_width: usize,
    pub scroll_offset: usize,
    pub scrolloff: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub preferred_col: usize,
    pub h_scroll: usize,
    pub h_scrolloff: usize,
    pub line_fold: bool,
    pub visual_mode: VisualMode,
    pub visual_anchor: Option<(usize, usize)>,
    pub mouse_mode: VisualMode,
    pub mouse_anchor: Option<(usize, usize)>,
    pub mouse_head: Option<(usize, usize)>,
    pub is_mouse_dragging: bool,
    pub last_mouse_pos: Option<(u16, u16)>,
    pub toggled_lines: HashSet<usize>,
    pub sticky: bool,
    pub clipboard_notice: Option<(String, Instant)>,
    pub picker: ServicePickerState,
}

impl AppUiState {
    pub fn new() -> Self {
        Self {
            viewport_height: 1,
            viewport_width: 1,
            scroll_offset: 0,
            scrolloff: 6,
            cursor_row: 0,
            cursor_col: 0,
            preferred_col: 0,
            h_scroll: 0,
            h_scrolloff: 5,
            line_fold: true,
            visual_mode: VisualMode::None,
            visual_anchor: None,
            mouse_mode: VisualMode::None,
            mouse_anchor: None,
            mouse_head: None,
            is_mouse_dragging: false,
            last_mouse_pos: None,
            toggled_lines: HashSet::new(),
            sticky: true,
            clipboard_notice: None,
            picker: ServicePickerState::new(),
        }
    }

    pub fn toggle_line_fold_anchored(&mut self, logs: &[&ProcessedLogRecord]) {
        if logs.is_empty() {
            self.line_fold = !self.line_fold;
            return;
        }

        let screen_y = if self.line_fold {
            self.cursor_row.saturating_sub(self.scroll_offset)
        } else {
            let mut y = 0;
            for item in logs.iter().take(self.cursor_row.min(logs.len())).skip(self.scroll_offset) {
                let p_len = item.prefix_len();
                let content_w = self.viewport_width.saturating_sub(p_len + 2).max(1);
                y += LineComposer::visual_height(&item.content, content_w);
            }
            y
        };
        let screen_y = screen_y.min(self.viewport_height.saturating_sub(1));

        self.line_fold = !self.line_fold;

        if self.line_fold {
            self.scroll_offset = self.cursor_row.saturating_sub(screen_y);
            self.adjust_h_scroll(1000);
        } else {
            self.h_scroll = 0;
            let mut remaining_y = screen_y;
            let mut new_offset = self.cursor_row;

            while new_offset > 0 && remaining_y > 0 {
                let prev = new_offset - 1;
                if let Some(log) = logs.get(prev) {
                    let content_w = self.viewport_width.saturating_sub(log.prefix_len() + 2).max(1);
                    let h = LineComposer::visual_height(&log.content, content_w);
                    if remaining_y >= h {
                        remaining_y -= h;
                        new_offset = prev;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            self.scroll_offset = new_offset;
        }
    }

    pub fn move_down(&mut self, amount: usize, total: usize) {
        if total > 0 {
            self.cursor_row = (self.cursor_row + amount).min(total.saturating_sub(1));
            self.adjust_scroll(total);
        }
    }

    pub fn move_up(&mut self, amount: usize, total: usize) {
        self.cursor_row = self.cursor_row.saturating_sub(amount);
        self.adjust_scroll(total);
    }

    pub fn snap_to_preferred_col_and_scroll_back(&mut self, line_content_len: usize) {
        if line_content_len == 0 {
            self.cursor_col = 0;
            self.h_scroll = 0;
            return;
        }

        let max_col = line_content_len.saturating_sub(1);
        self.cursor_col = self.preferred_col.min(max_col);

        let padding = 5;
        let chars_before = self.cursor_col.saturating_sub(self.h_scroll);
        if self.cursor_col < self.h_scroll || chars_before < padding {
            self.h_scroll = self.cursor_col.saturating_sub(padding);
        } else {
            let win = self.viewport_width.saturating_sub(20).max(5);
            if self.cursor_col >= self.h_scroll + win {
                self.h_scroll = (self.cursor_col + 1).saturating_sub(win);
            }
        }
    }

    pub fn pan_viewport_down(&mut self, amount: usize, total: usize) {
        if total == 0 { return; }
        let max_scroll = total.saturating_sub(self.viewport_height);
        let target_scroll = (self.scroll_offset + amount).min(max_scroll);
        let actual_amount = target_scroll.saturating_sub(self.scroll_offset);

        if actual_amount == 0 { return; }

        let margin = self.scrolloff.min(self.viewport_height.saturating_sub(1) / 2);
        let min_required_cursor = target_scroll + margin;

        if min_required_cursor < total {
            self.scroll_offset = target_scroll;
            if self.cursor_row < min_required_cursor {
                self.cursor_row = min_required_cursor;
            }
        } else {
            self.scroll_offset = total.saturating_sub(self.viewport_height);
            self.cursor_row = total.saturating_sub(1);
        }
    }

    pub fn pan_viewport_up(&mut self, amount: usize) {
        if self.scroll_offset == 0 { return; }
        let target_scroll = self.scroll_offset.saturating_sub(amount);
        self.scroll_offset = target_scroll;

        let margin = self.scrolloff.min(self.viewport_height.saturating_sub(1) / 2);
        let max_allowed_cursor = (target_scroll + self.viewport_height).saturating_sub(margin + 1);

        if self.cursor_row > max_allowed_cursor {
            self.cursor_row = max_allowed_cursor;
        }
    }

    pub fn pan_viewport_left(&mut self, amount: usize) {
        if self.h_scroll == 0 { return; }
        self.h_scroll = self.h_scroll.saturating_sub(amount);

        let win = self.viewport_width.saturating_sub(20).max(5);
        let max_allowed_cursor = self.h_scroll + win.saturating_sub(5);

        if self.cursor_col > max_allowed_cursor {
            self.cursor_col = max_allowed_cursor;
            self.preferred_col = self.cursor_col;
        }
    }

    pub fn pan_viewport_right(&mut self, amount: usize, max_len: usize) {
        if max_len == 0 { return; }
        let last_col = max_len.saturating_sub(1);
        let max_scroll = last_col.saturating_sub(5);

        if self.h_scroll >= max_scroll {
            return;
        }

        self.h_scroll = (self.h_scroll + amount).min(max_scroll);
        let min_required_cursor = self.h_scroll + 5;

        if self.cursor_col < min_required_cursor {
            self.cursor_col = min_required_cursor.min(last_col);
            self.preferred_col = self.cursor_col;
        }
    }

    pub fn scroll_viewport_down(&mut self, amount: usize, total: usize) {
        if total == 0 { return; }
        let max_scroll = total.saturating_sub(self.viewport_height);
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);

        let margin = self.scrolloff.min(self.viewport_height.saturating_sub(1) / 2);
        let min_allowed_cursor = (self.scroll_offset + margin).min(total.saturating_sub(1));
        if self.cursor_row < min_allowed_cursor {
            self.cursor_row = min_allowed_cursor;
        }
    }

    pub fn scroll_viewport_up(&mut self, amount: usize, total: usize) {
        if total == 0 { return; }
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);

        let margin = self.scrolloff.min(self.viewport_height.saturating_sub(1) / 2);
        let max_allowed_cursor = (self.scroll_offset + self.viewport_height).saturating_sub(margin + 1);
        if self.cursor_row > max_allowed_cursor {
            self.cursor_row = max_allowed_cursor.min(total.saturating_sub(1));
        }
    }

    pub fn drag_scroll_viewport_down(&mut self, amount: usize, total: usize) {
        if total == 0 { return; }
        let max_scroll = total.saturating_sub(self.viewport_height);
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
    }

    pub fn drag_scroll_viewport_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn move_left(&mut self, amount: usize, _max_len: usize) {
        if self.cursor_col > 0 {
            self.cursor_col = self.cursor_col.saturating_sub(amount);
            self.preferred_col = self.cursor_col;
            let padding = 5;
            let chars_before = self.cursor_col.saturating_sub(self.h_scroll);
            if self.cursor_col < self.h_scroll || chars_before < padding {
                self.h_scroll = self.cursor_col.saturating_sub(padding);
            }
        }
    }

    pub fn move_right(&mut self, amount: usize, max_len: usize) {
        if max_len == 0 {
            return;
        }

        let last_col = max_len.saturating_sub(1);
        if self.cursor_col < last_col {
            self.cursor_col = (self.cursor_col + amount).min(last_col);
            self.preferred_col = self.cursor_col;
            if self.line_fold {
                self.adjust_h_scroll(max_len);
            }
        }
    }

    pub fn adjust_scroll(&mut self, total: usize) {
        if self.viewport_height == 0 || total == 0 { return; }
        let margin = self.scrolloff.min(self.viewport_height.saturating_sub(1) / 2);
        if self.cursor_row < self.scroll_offset + margin {
            self.scroll_offset = self.cursor_row.saturating_sub(margin);
        }
        if self.cursor_row + margin >= self.scroll_offset + self.viewport_height {
            self.scroll_offset = (self.cursor_row + margin + 1).saturating_sub(self.viewport_height);
        }
        let max_scroll = total.saturating_sub(self.viewport_height);
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
    }

    pub fn adjust_h_scroll(&mut self, max_len: usize) {
        let win = self.viewport_width.saturating_sub(20).max(5);
        let margin = 5.min(win.saturating_sub(1) / 2);

        if self.cursor_col < self.h_scroll + margin {
            self.h_scroll = self.cursor_col.saturating_sub(margin);
        }
        if self.cursor_col + margin >= self.h_scroll + win {
            self.h_scroll = (self.cursor_col + 1).saturating_sub(win);
        }

        let max_push = max_len.saturating_sub(1).saturating_sub(5);
        if self.h_scroll > max_push {
            self.h_scroll = max_push;
        }
    }

    pub fn toggle_visual(&mut self, mode: VisualMode) {
        if self.visual_mode == mode {
            self.exit_visual();
        } else {
            self.visual_mode = mode;
            self.visual_anchor = Some((self.cursor_row, self.cursor_col));
            self.sticky = false;
        }
    }

    pub fn exit_visual(&mut self) {
        self.visual_mode = VisualMode::None;
        self.visual_anchor = None;
    }

    pub fn clear_mouse_selection(&mut self) {
        self.mouse_mode = VisualMode::None;
        self.mouse_anchor = None;
        self.mouse_head = None;
        self.is_mouse_dragging = false;
        self.last_mouse_pos = None;
    }

    pub fn set_clipboard_notice(&mut self, msg: impl Into<String>) {
        self.clipboard_notice = Some((msg.into(), Instant::now()));
    }
}
