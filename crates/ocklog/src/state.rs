use crate::ansi::{parse_ansi, AnsiSegment};
use crate::query::{parse_timestamp_secs, Query};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualMode {
    None,
    Character,
    Line,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMode {
    None,
    Filter,
}

#[derive(Debug, Clone)]
pub struct LogItem {
    pub service: String,
    pub content: String,
    pub segments: Vec<AnsiSegment>,
    pub level: Option<String>,
    pub timestamp: Option<String>,
    pub is_system: bool,
}

impl LogItem {
    pub fn new(
        service: String,
        raw_message: String,
        level: Option<String>,
        timestamp: Option<String>,
        is_system: bool,
    ) -> Self {
        let (content, segments) = parse_ansi(&raw_message);
        Self {
            service,
            content,
            segments,
            level,
            timestamp,
            is_system,
        }
    }

    pub fn full_line_text(&self) -> String {
        if self.is_system {
            return self.content.clone();
        }
        format!("{}{}", self.tag_text(), self.content)
    }

    pub fn tag_text(&self) -> String {
        format!("[{}] ", truncate_service(&self.service, 16))
    }

    pub fn prefix_len(&self) -> usize {
        if self.is_system {
            0
        } else {
            self.tag_text().chars().count()
        }
    }
}

pub struct ContainerFilterOption {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub is_running: bool,
    pub has_error: bool,
}

pub struct AppState {
    pub logs: VecDeque<LogItem>,
    pub max_capacity: usize,

    // Vertical navigation & Viewport
    pub scroll_offset: usize,
    pub scrolloff: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub viewport_height: usize,
    pub viewport_width: usize,

    // Horizontal navigation & scrolling
    pub h_scroll: usize,
    pub h_scrolloff: usize,

    // Line Fold / Wrap Mode
    pub line_fold: bool,

    // Mouse selection & auto-scroll
    pub visual_mode: VisualMode,
    pub visual_anchor: Option<(usize, usize)>,
    pub is_mouse_dragging: bool,
    pub last_mouse_pos: Option<(u16, u16)>,

    pub toggled_lines: HashSet<usize>,
    pub sticky: bool,

    // Service Switcher (<C-s>)
    pub show_service_menu: bool,
    pub show_service_search: bool,
    pub service_options: Vec<ContainerFilterOption>,
    pub service_menu_selected_idx: usize,
    pub service_menu_scroll: usize,
    pub service_search_query: String,

    // Filter Command Line (<C-f>)
    pub prompt_mode: PromptMode,
    pub input_buffer: String,
    pub filter_cursor_col: usize,
    pub filter_ast: Option<Query>,
    pub active_filter_str: Option<String>,
    pub live_filter_ast: Option<Query>,

    pub visible_indices: Vec<usize>,
    pub synthetic_logs: Option<Vec<LogItem>>,

    pub clipboard_notice: Option<(String, Instant)>,
    pub status_message: Option<String>,
}

impl AppState {
    pub fn new(capacity: usize) -> Self {
        Self {
            logs: VecDeque::with_capacity(capacity),
            max_capacity: capacity,
            scroll_offset: 0,
            scrolloff: 6,
            cursor_row: 0,
            cursor_col: 0,
            viewport_height: 1,
            viewport_width: 1,
            h_scroll: 0,
            h_scrolloff: 2,
            line_fold: true,
            visual_mode: VisualMode::None,
            visual_anchor: None,
            is_mouse_dragging: false,
            last_mouse_pos: None,
            toggled_lines: HashSet::new(),
            sticky: true,
            show_service_menu: false,
            show_service_search: false,
            service_options: Vec::new(),
            service_menu_selected_idx: 0,
            service_menu_scroll: 0,
            service_search_query: String::new(),
            prompt_mode: PromptMode::None,
            input_buffer: String::new(),
            filter_cursor_col: 0,
            filter_ast: None,
            active_filter_str: None,
            live_filter_ast: None,
            visible_indices: Vec::new(),
            synthetic_logs: None,
            clipboard_notice: None,
            status_message: None,
        }
    }

    pub fn toggle_line_fold(&mut self) {
        if self.visible_count() == 0 {
            self.line_fold = !self.line_fold;
            return;
        }

        let current_screen_y = if self.line_fold {
            self.cursor_row.saturating_sub(self.scroll_offset)
        } else {
            let mut y = 0;
            for r in self.scroll_offset..self.cursor_row {
                if let Some(log) = self.get_visible_log(r) {
                    let p_len = log.prefix_len();
                    let content_w = get_unfolded_content_width(self.viewport_width, p_len);
                    y += visual_lines_for_log(log, content_w);
                }
            }
            y
        };
        let current_screen_y = current_screen_y.min(self.viewport_height.saturating_sub(1));

        self.line_fold = !self.line_fold;

        if self.line_fold {
            self.scroll_offset = self.cursor_row.saturating_sub(current_screen_y);
            self.adjust_h_scroll();
        } else {
            self.h_scroll = 0;
            let mut remaining_y = current_screen_y;
            let mut new_offset = self.cursor_row;

            while new_offset > 0 && remaining_y > 0 {
                let prev_idx = new_offset - 1;
                if let Some(log) = self.get_visible_log(prev_idx) {
                    let p_len = log.prefix_len();
                    let content_w = get_unfolded_content_width(self.viewport_width, p_len);
                    let h = visual_lines_for_log(log, content_w);
                    if remaining_y >= h {
                        remaining_y -= h;
                        new_offset = prev_idx;
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

    pub fn is_screen_coords_on_tag(&self, screen_x: usize, screen_y: usize) -> bool {
        let count = self.visible_count();
        if count == 0 {
            return false;
        }

        if self.line_fold {
            let row_idx = (self.scroll_offset + screen_y).min(count.saturating_sub(1));
            if let Some(log) = self.get_visible_log(row_idx) {
                if log.is_system {
                    return false;
                }
                let is_toggled = self
                    .visible_indices
                    .get(row_idx)
                    .map(|idx| self.toggled_lines.contains(idx))
                    .unwrap_or(false);
                let tag_width = log.prefix_len() + if is_toggled { 2 } else { 0 };
                return screen_x <= tag_width;
            }
        } else {
            let mut v_line = 0;
            for r in self.scroll_offset..count {
                if let Some(log) = self.get_visible_log(r) {
                    let p_len = log.prefix_len();
                    let content_w = get_unfolded_content_width(self.viewport_width, p_len);
                    let chunks = get_log_visual_chunks(&log.content, content_w);
                    let is_toggled = self
                        .visible_indices
                        .get(r)
                        .map(|idx| self.toggled_lines.contains(idx))
                        .unwrap_or(false);
                    let tag_width = p_len + if is_toggled { 2 } else { 0 };

                    for _ in 0..chunks.len() {
                        if v_line == screen_y {
                            return screen_x <= tag_width;
                        }
                        v_line += 1;
                    }
                }
            }
        }
        false
    }

    pub fn max_line_len(&self) -> usize {
        let max_c = self
            .logs
            .iter()
            .map(|l| l.full_line_text().chars().count())
            .max()
            .unwrap_or(0);
        max_c.max(self.viewport_width * 3)
    }

    pub fn max_content_len(&self) -> usize {
        self.logs.iter().map(|l| l.content.chars().count()).max().unwrap_or(0)
    }

    pub fn current_line_content_end_col(&self) -> usize {
        if let Some(log) = self.get_visible_log(self.cursor_row) {
            if log.is_system {
                log.content.chars().count().saturating_sub(1)
            } else {
                let p_len = log.prefix_len();
                let c_len = log.content.chars().count();
                p_len + c_len.saturating_sub(1)
            }
        } else {
            0
        }
    }

    pub fn viewport_content_window_width(&self) -> usize {
        let prefix_len = if let Some(log) = self.get_visible_log(self.cursor_row) {
            log.prefix_len()
        } else {
            0
        };
        let avail_width = self.viewport_width.saturating_sub(prefix_len);
        avail_width.saturating_sub(2).max(1)
    }

    pub fn filtered_service_indices(&self) -> Vec<usize> {
        if !self.show_service_search || self.service_search_query.is_empty() {
            return (0..self.service_options.len()).collect();
        }
        let q = self.service_search_query.to_lowercase();
        self.service_options
            .iter()
            .enumerate()
            .filter(|(_, opt)| opt.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn update_service_search(&mut self) {
        let filtered = self.filtered_service_indices();
        if filtered.is_empty() {
            self.service_menu_selected_idx = 0;
            self.service_menu_scroll = 0;
            return;
        }

        if self.service_menu_selected_idx >= filtered.len() {
            self.service_menu_selected_idx = filtered.len() - 1;
        }
    }

    pub fn update_live_filter(&mut self) {
        if self.prompt_mode == PromptMode::Filter {
            let trimmed = self.input_buffer.trim();
            if trimmed.is_empty() {
                self.live_filter_ast = None;
            } else {
                self.live_filter_ast = Query::parse(trimmed).ok();
            }
        } else {
            self.live_filter_ast = None;
        }
    }

    pub fn toggle_current_line(&mut self) {
        if let Some(&actual_idx) = self.visible_indices.get(self.cursor_row) {
            if self.toggled_lines.contains(&actual_idx) {
                self.toggled_lines.remove(&actual_idx);
            } else {
                self.toggled_lines.insert(actual_idx);
            }
        }
    }

    pub fn set_copied_notification(&mut self) {
        self.clipboard_notice = Some(("Copied to clipboard".to_string(), Instant::now()));
    }

    pub fn push_log(&mut self, item: LogItem) {
        let old_count = self.visible_count();
        let was_at_bottom = old_count == 0 || self.cursor_row + 1 >= old_count;

        if self.logs.len() >= self.max_capacity {
            self.logs.pop_front();
            self.cursor_row = self.cursor_row.saturating_sub(1);
            if let Some((anchor_row, anchor_col)) = self.visual_anchor {
                self.visual_anchor = Some((anchor_row.saturating_sub(1), anchor_col));
            }
        }

        let is_at_end = if let Some(ref ts) = item.timestamp {
            let mut insert_idx = self.logs.len();
            while insert_idx > 0 {
                let prev = &self.logs[insert_idx - 1];
                if let Some(ref prev_ts) = prev.timestamp {
                    if prev_ts <= ts {
                        break;
                    }
                    insert_idx -= 1;
                } else {
                    insert_idx -= 1;
                }
            }
            let at_end = insert_idx == self.logs.len();
            self.logs.insert(insert_idx, item);
            at_end
        } else {
            self.logs.push_back(item);
            true
        };

        let has_stream_modifiers = self.filter_ast.as_ref().map_or(false, |q| q.has_stream_modifiers());

        if is_at_end && !self.logs.is_empty() && !has_stream_modifiers {
            let idx = self.logs.len() - 1;
            let ref_now = self.reference_timestamp_secs();
            if self.item_matches_filter(&self.logs[idx], ref_now) {
                self.visible_indices.push(idx);
            }
        } else {
            self.recompute_visible_indices();
        }

        let new_count = self.visible_count();

        if self.visual_mode == VisualMode::None {
            if was_at_bottom {
                if new_count > 0 {
                    self.cursor_row = new_count - 1;
                    self.adjust_scroll();
                    self.move_to_bottom();
                }
            } else {
                let top_margin = self.scrolloff.max(6);
                let max_allowed_scroll = self.cursor_row.saturating_sub(top_margin);
                if self.scroll_offset > max_allowed_scroll {
                    self.scroll_offset = max_allowed_scroll;
                }
            }
        }
    }

    pub fn reference_timestamp_secs(&self) -> u64 {
        for log in self.logs.iter().rev() {
            if let Some(ref ts) = log.timestamp {
                if let Some(secs) = parse_timestamp_secs(ts) {
                    return secs;
                }
            }
        }
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn item_matches_filter(&self, log: &LogItem, ref_now_secs: u64) -> bool {
        if !self.service_options.is_empty() {
            if let Some(opt) = self.service_options.iter().find(|o| o.name == log.service) {
                if !opt.enabled {
                    return false;
                }
            }
        }

        if let Some(ref q) = self.filter_ast {
            if !q.matches(log, ref_now_secs) {
                return false;
            }
        }

        true
    }

    pub fn recompute_visible_indices(&mut self) {
        let top_anchor_raw = self.visible_indices.get(self.scroll_offset).copied();
        let cursor_anchor_raw = self.visible_indices.get(self.cursor_row).copied();

        let enabled_services: HashMap<String, bool> = self
            .service_options
            .iter()
            .map(|opt| (opt.name.clone(), opt.enabled))
            .collect();

        let ref_now_secs = self.reference_timestamp_secs();

        if let Some(ref q) = self.filter_ast {
            let is_enabled = |svc: &str| {
                if enabled_services.is_empty() {
                    true
                } else {
                    enabled_services.get(svc).copied().unwrap_or(true)
                }
            };
            let (visible, synthetic) = q.execute_on_deque(&self.logs, is_enabled, ref_now_secs);
            self.visible_indices = visible;
            self.synthetic_logs = synthetic;
        } else {
            self.synthetic_logs = None;
            let mut visible = Vec::with_capacity(self.logs.len());
            for (idx, log) in self.logs.iter().enumerate() {
                if !self.service_options.is_empty() {
                    if let Some(&enabled) = enabled_services.get(&log.service) {
                        if !enabled {
                            continue;
                        }
                    }
                }
                visible.push(idx);
            }
            self.visible_indices = visible;
        }

        let total_visible = self.visible_count();

        if let Some(target_raw) = top_anchor_raw {
            match self.visible_indices.binary_search(&target_raw) {
                Ok(new_offset) => {
                    self.scroll_offset = new_offset;
                }
                Err(closest_idx) => {
                    self.scroll_offset = closest_idx.min(total_visible.saturating_sub(1));
                }
            }
        }

        if let Some(target_cursor_raw) = cursor_anchor_raw {
            match self.visible_indices.binary_search(&target_cursor_raw) {
                Ok(new_cursor) => {
                    self.cursor_row = new_cursor;
                }
                Err(closest_idx) => {
                    self.cursor_row = closest_idx.min(total_visible.saturating_sub(1));
                }
            }
        }

        let max_scroll = total_visible.saturating_sub(self.viewport_height);
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
        if self.cursor_row >= total_visible {
            self.cursor_row = total_visible.saturating_sub(1);
        }
    }

    pub fn visible_count(&self) -> usize {
        if let Some(ref syn) = self.synthetic_logs {
            syn.len()
        } else {
            self.visible_indices.len()
        }
    }

    pub fn get_visible_log(&self, visible_row: usize) -> Option<&LogItem> {
        if let Some(ref syn) = self.synthetic_logs {
            syn.get(visible_row)
        } else {
            let &actual_idx = self.visible_indices.get(visible_row)?;
            self.logs.get(actual_idx)
        }
    }

    pub fn current_line_len(&self) -> usize {
        self.get_visible_log(self.cursor_row)
            .map(|l| l.full_line_text().chars().count())
            .unwrap_or(0)
    }

    pub fn move_down(&mut self, amount: usize) {
        let count = self.visible_count();
        if count == 0 {
            return;
        }
        self.cursor_row = (self.cursor_row + amount).min(count.saturating_sub(1));
        self.adjust_scroll();

        let line_max_col = self.current_line_content_end_col();
        if self.cursor_col > line_max_col {
            self.cursor_col = line_max_col;
        }
    }

    pub fn move_up(&mut self, amount: usize) {
        self.cursor_row = self.cursor_row.saturating_sub(amount);
        self.adjust_scroll();

        let line_max_col = self.current_line_content_end_col();
        if self.cursor_col > line_max_col {
            self.cursor_col = line_max_col;
        }
    }

    pub fn move_left(&mut self, amount: usize) {
        if self.cursor_col > 0 {
            self.cursor_col = self.cursor_col.saturating_sub(amount);
            if self.line_fold {
                self.adjust_h_scroll();
            }
        } else if self.h_scroll > 0 && self.line_fold {
            self.h_scroll = self.h_scroll.saturating_sub(amount);
        }
    }

    pub fn move_right(&mut self, amount: usize) {
        let line_max_col = self.current_line_content_end_col();
        let window_width = self.viewport_content_window_width();
        let max_h = self.max_content_len().saturating_sub(window_width);

        if self.cursor_col < line_max_col {
            self.cursor_col = (self.cursor_col + amount).min(line_max_col);
            if self.line_fold {
                self.adjust_h_scroll();
            }
        } else if self.line_fold {
            self.h_scroll = (self.h_scroll + amount).min(max_h);
        }
    }

    pub fn move_word_forward(&mut self) {
        let log = match self.get_visible_log(self.cursor_row) {
            Some(l) => l,
            None => return,
        };
        let text = log.full_line_text();
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        if len == 0 || self.cursor_col >= len.saturating_sub(1) {
            return;
        }

        let mut i = self.cursor_col;
        let start_is_word = chars[i].is_alphanumeric() || chars[i] == '_';
        let start_is_space = chars[i].is_whitespace();

        if start_is_space {
            while i < len && chars[i].is_whitespace() {
                i += 1;
            }
        } else if start_is_word {
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            while i < len && chars[i].is_whitespace() {
                i += 1;
            }
        } else {
            while i < len && !chars[i].is_alphanumeric() && chars[i] != '_' && !chars[i].is_whitespace() {
                i += 1;
            }
            while i < len && chars[i].is_whitespace() {
                i += 1;
            }
        }

        self.cursor_col = i.min(len.saturating_sub(1));
        if self.line_fold {
            self.adjust_h_scroll();
        }
    }

    pub fn move_word_backward(&mut self) {
        let log = match self.get_visible_log(self.cursor_row) {
            Some(l) => l,
            None => return,
        };
        let text = log.full_line_text();
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() || self.cursor_col == 0 {
            return;
        }

        let mut i = self.cursor_col.saturating_sub(1);

        while i > 0 && chars[i].is_whitespace() {
            i -= 1;
        }

        let is_word = chars[i].is_alphanumeric() || chars[i] == '_';
        if is_word {
            while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
                i -= 1;
            }
        } else if !chars[i].is_whitespace() {
            while i > 0 && !chars[i - 1].is_alphanumeric() && chars[i - 1] != '_' && !chars[i - 1].is_whitespace() {
                i -= 1;
            }
        }

        self.cursor_col = i;
        if self.line_fold {
            self.adjust_h_scroll();
        }
    }

    pub fn move_word_end(&mut self) {
        let log = match self.get_visible_log(self.cursor_row) {
            Some(l) => l,
            None => return,
        };
        let text = log.full_line_text();
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        if len == 0 || self.cursor_col >= len.saturating_sub(1) {
            return;
        }

        let mut i = self.cursor_col + 1;
        while i < len && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= len {
            return;
        }

        let is_word = chars[i].is_alphanumeric() || chars[i] == '_';
        if is_word {
            while i + 1 < len && (chars[i + 1].is_alphanumeric() || chars[i + 1] == '_') {
                i += 1;
            }
        } else {
            while i + 1 < len && !chars[i + 1].is_alphanumeric() && chars[i + 1] != '_' && !chars[i + 1].is_whitespace() {
                i += 1;
            }
        }

        self.cursor_col = i.min(len.saturating_sub(1));
        if self.line_fold {
            self.adjust_h_scroll();
        }
    }

    pub fn move_to_top(&mut self) {
        self.cursor_row = 0;
        let line_max_col = self.current_line_content_end_col();
        if self.cursor_col > line_max_col {
            self.cursor_col = line_max_col;
        }
        self.adjust_scroll();
    }

    pub fn move_to_bottom(&mut self) {
        let count = self.visible_count();
        if count > 0 {
            self.cursor_row = count - 1;
            let line_max_col = self.current_line_content_end_col();
            if self.cursor_col > line_max_col {
                self.cursor_col = line_max_col;
            }
            self.adjust_scroll();
        }
    }

    pub fn adjust_scroll(&mut self) {
        let count = self.visible_count();
        if self.viewport_height == 0 || count == 0 {
            return;
        }

        if self.line_fold {
            let effective_so = self.scrolloff.min(self.viewport_height.saturating_sub(1) / 2);

            if self.cursor_row < self.scroll_offset + effective_so {
                self.scroll_offset = self.cursor_row.saturating_sub(effective_so);
            }

            if self.cursor_row + effective_so >= self.scroll_offset + self.viewport_height {
                self.scroll_offset = (self.cursor_row + effective_so + 1).saturating_sub(self.viewport_height);
            }

            let max_scroll = count.saturating_sub(self.viewport_height);
            if self.scroll_offset > max_scroll {
                self.scroll_offset = max_scroll;
            }
        } else {
            if self.cursor_row < self.scroll_offset {
                self.scroll_offset = self.cursor_row;
                return;
            }

            let mut total_visual_rows = 0;
            for r in self.scroll_offset..=self.cursor_row {
                if let Some(log) = self.get_visible_log(r) {
                    let p_len = log.prefix_len();
                    let content_w = get_unfolded_content_width(self.viewport_width, p_len);
                    total_visual_rows += visual_lines_for_log(log, content_w);
                }
            }

            while self.scroll_offset < self.cursor_row && total_visual_rows > self.viewport_height {
                if let Some(log) = self.get_visible_log(self.scroll_offset) {
                    let p_len = log.prefix_len();
                    let content_w = get_unfolded_content_width(self.viewport_width, p_len);
                    let h = visual_lines_for_log(log, content_w);
                    total_visual_rows = total_visual_rows.saturating_sub(h);
                }
                self.scroll_offset += 1;
            }
        }
    }

    pub fn adjust_h_scroll(&mut self) {
        let prefix_len = if let Some(log) = self.get_visible_log(self.cursor_row) {
            log.prefix_len()
        } else {
            0
        };

        let avail_width = self.viewport_width.saturating_sub(prefix_len);
        if avail_width <= 2 {
            return;
        }

        let window_width = avail_width.saturating_sub(2);
        if window_width == 0 {
            return;
        }

        let content_col = self.cursor_col.saturating_sub(prefix_len);
        let margin = self.h_scrolloff.min(window_width.saturating_sub(1) / 2);

        if content_col < self.h_scroll + margin {
            self.h_scroll = content_col.saturating_sub(margin);
        }

        if content_col + margin >= self.h_scroll + window_width {
            self.h_scroll = (content_col + margin + 1).saturating_sub(window_width);
        }

        let max_h = self.max_content_len().saturating_sub(window_width);
        if self.h_scroll > max_h {
            self.h_scroll = max_h;
        }
    }

    pub fn screen_coords_to_char_pos(&self, screen_x: usize, screen_y: usize) -> (usize, usize) {
        let count = self.visible_count();
        if count == 0 {
            return (0, 0);
        }

        if self.line_fold {
            let row_idx = (self.scroll_offset + screen_y).min(count.saturating_sub(1));
            let log = match self.get_visible_log(row_idx) {
                Some(l) => l,
                None => return (row_idx, 0),
            };

            if log.is_system {
                let col = char_at_display_width(&log.content, screen_x);
                return (row_idx, col);
            }

            let p_len = log.prefix_len();
            let p_str = log.tag_text();
            let p_width = str_display_width_up_to(&p_str, p_len);

            if screen_x <= p_width {
                (row_idx, 0)
            } else {
                let has_left = self.h_scroll > 0 && log.content.chars().count() > 0;
                let left_pad = if has_left { 1 } else { 0 };
                let content_screen_x = screen_x.saturating_sub(p_width + left_pad);

                let content_slice: String = log.content.chars().skip(self.h_scroll).collect();
                let char_offset = char_at_display_width(&content_slice, content_screen_x);
                let col = (p_len + self.h_scroll + char_offset).min(p_len + log.content.chars().count());
                (row_idx, col)
            }
        } else {
            let mut v_line = 0;
            let mut target_row = self.scroll_offset.min(count.saturating_sub(1));
            let mut target_col = 0;

            for r in self.scroll_offset..count {
                if let Some(log) = self.get_visible_log(r) {
                    target_row = r;
                    let p_len = log.prefix_len();
                    let content_w = get_unfolded_content_width(self.viewport_width, p_len);
                    let chunks = get_log_visual_chunks(&log.content, content_w);
                    let is_toggled = self
                        .visible_indices
                        .get(r)
                        .map(|idx| self.toggled_lines.contains(idx))
                        .unwrap_or(false);

                    for (chunk_i, chunk) in chunks.iter().enumerate() {
                        if v_line == screen_y {
                            if chunk_i == 0 {
                                let check_pad = if is_toggled { 2 } else { 0 };
                                let tag_width = p_len + check_pad;
                                if screen_x <= tag_width {
                                    target_col = 0;
                                } else {
                                    let content_click_x = screen_x.saturating_sub(tag_width);
                                    let char_offset = char_at_display_width(&chunk.text, content_click_x);
                                    target_col = p_len + chunk.content_char_start + char_offset;
                                }
                            } else {
                                let indent = p_len + if is_toggled { 2 } else { 0 };
                                if screen_x <= indent {
                                    target_col = 0;
                                } else {
                                    let content_click_x = screen_x.saturating_sub(indent);
                                    let char_offset = char_at_display_width(&chunk.text, content_click_x);
                                    target_col = p_len + chunk.content_char_start + char_offset;
                                }
                            }
                            return (target_row, target_col);
                        }
                        v_line += 1;
                    }
                }
            }

            (target_row, target_col)
        }
    }

    pub fn trigger_mouse_auto_scroll(&mut self) -> bool {
        if !self.is_mouse_dragging {
            return false;
        }
        let (mx, my) = match self.last_mouse_pos {
            Some(pos) => pos,
            None => return false,
        };

        let h = self.viewport_height as u16;
        let w = self.viewport_width as u16;
        if h == 0 || w == 0 {
            return false;
        }

        let mut scrolled = false;

        if my < 5 {
            let speed = if my == 0 { 2 } else { 1 };
            self.move_up(speed);
            scrolled = true;
        } else if my >= h.saturating_sub(5) {
            let speed = if my >= h.saturating_sub(1) { 2 } else { 1 };
            self.move_down(speed);
            scrolled = true;
        }

        if mx < 5 {
            let speed = if mx == 0 { 4 } else { 2 };
            self.move_left(speed);
            scrolled = true;
        } else if mx >= w.saturating_sub(5) {
            let speed = if mx >= w.saturating_sub(1) { 4 } else { 2 };
            self.move_right(speed);
            scrolled = true;
        }

        if scrolled {
            let (r, c) = self.screen_coords_to_char_pos(mx as usize, my as usize);
            self.cursor_row = r;
            self.cursor_col = c;
        }

        scrolled
    }

    pub fn toggle_visual_mode(&mut self, mode: VisualMode) {
        if self.visual_mode == mode {
            self.visual_mode = VisualMode::None;
            self.visual_anchor = None;
        } else {
            self.visual_mode = mode;
            self.visual_anchor = Some((self.cursor_row, self.cursor_col));
            self.sticky = false;
        }
    }

    pub fn exit_visual_mode(&mut self) {
        self.visual_mode = VisualMode::None;
        self.visual_anchor = None;
        self.is_mouse_dragging = false;
        self.last_mouse_pos = None;
    }

    pub fn get_selected_text(&self) -> Option<String> {
        if !self.toggled_lines.is_empty() {
            let mut sorted_indices: Vec<usize> = self.toggled_lines.iter().copied().collect();
            sorted_indices.sort_unstable();
            let lines: Vec<String> = sorted_indices
                .into_iter()
                .filter_map(|idx| self.logs.get(idx).map(|l| l.full_line_text()))
                .collect();
            return Some(lines.join("\n"));
        }

        let (anchor_row, anchor_col) = self.visual_anchor?;
        if self.visual_mode == VisualMode::None {
            return None;
        }

        match self.visual_mode {
            VisualMode::Line => {
                let start_row = anchor_row.min(self.cursor_row);
                let end_row = anchor_row.max(self.cursor_row);
                let lines: Vec<String> = (start_row..=end_row)
                    .filter_map(|r| self.get_visible_log(r).map(|l| l.full_line_text()))
                    .collect();
                Some(lines.join("\n"))
            }
            VisualMode::Block => {
                let start_row = anchor_row.min(self.cursor_row);
                let end_row = anchor_row.max(self.cursor_row);
                let start_col = anchor_col.min(self.cursor_col);
                let end_col = anchor_col.max(self.cursor_col);

                let mut lines = Vec::new();
                for r in start_row..=end_row {
                    if let Some(log) = self.get_visible_log(r) {
                        let text = log.full_line_text();
                        let chars: Vec<char> = text.chars().collect();
                        let line_slice: String = chars
                            .iter()
                            .skip(start_col)
                            .take(end_col.saturating_sub(start_col) + 1)
                            .collect();
                        lines.push(line_slice);
                    }
                }
                Some(lines.join("\n"))
            }
            VisualMode::Character => {
                let (start, end) = if (anchor_row, anchor_col) <= (self.cursor_row, self.cursor_col) {
                    ((anchor_row, anchor_col), (self.cursor_row, self.cursor_col))
                } else {
                    ((self.cursor_row, self.cursor_col), (anchor_row, anchor_col))
                };

                let mut result = Vec::new();
                for r in start.0..=end.0 {
                    if let Some(log) = self.get_visible_log(r) {
                        let text = log.full_line_text();
                        let chars: Vec<char> = text.chars().collect();

                        let slice = if start.0 == end.0 {
                            chars
                                .iter()
                                .skip(start.1)
                                .take(end.1.saturating_sub(start.1) + 1)
                                .collect::<String>()
                        } else if r == start.0 {
                            chars.iter().skip(start.1).collect::<String>()
                        } else if r == end.0 {
                            chars.iter().take(end.1 + 1).collect::<String>()
                        } else {
                            chars.into_iter().collect::<String>()
                        };

                        result.push(slice);
                    }
                }
                Some(result.join("\n"))
            }
            VisualMode::None => None,
        }
    }
}

pub fn get_unfolded_content_width(viewport_width: usize, prefix_len: usize) -> usize {
    viewport_width.saturating_sub(prefix_len + 2).max(1)
}

#[derive(Debug, Clone)]
pub struct VisualChunk {
    pub text: String,
    pub content_char_start: usize,
    pub has_continuation: bool,
}

pub fn get_log_visual_chunks(content: &str, content_width: usize) -> Vec<VisualChunk> {
    let mut chunks = Vec::new();
    let mut current_char_offset = 0;

    let lines: Vec<&str> = content.split('\n').collect();
    let total_lines = lines.len();

    for (line_idx, line) in lines.iter().enumerate() {
        let is_last_logical_line = line_idx + 1 == total_lines;
        let clean = line.trim_end_matches('\r');
        let chars: Vec<char> = clean.chars().collect();

        if chars.is_empty() {
            chunks.push(VisualChunk {
                text: String::new(),
                content_char_start: current_char_offset,
                has_continuation: !is_last_logical_line,
            });
        } else {
            let line_chunks: Vec<Vec<char>> = chars.chunks(content_width).map(|c| c.to_vec()).collect();
            let num_line_chunks = line_chunks.len();
            let mut chunk_offset = current_char_offset;

            for (c_idx, c_chars) in line_chunks.into_iter().enumerate() {
                let is_last_subchunk = c_idx + 1 == num_line_chunks;
                let has_cont = !(is_last_logical_line && is_last_subchunk);
                let c_len = c_chars.len();
                chunks.push(VisualChunk {
                    text: c_chars.into_iter().collect(),
                    content_char_start: chunk_offset,
                    has_continuation: has_cont,
                });
                chunk_offset += c_len;
            }
        }

        // Add +1 for the '\n' character
        current_char_offset += clean.chars().count() + 1;
    }

    if chunks.is_empty() {
        chunks.push(VisualChunk {
            text: String::new(),
            content_char_start: 0,
            has_continuation: false,
        });
    }

    chunks
}

pub fn visual_lines_for_log(log: &LogItem, content_width: usize) -> usize {
    if content_width == 0 {
        return 1;
    }
    let mut total_lines = 0;
    for line in log.content.split('\n') {
        let clean = line.trim_end_matches('\r');
        let count = clean.chars().count();
        if count == 0 {
            total_lines += 1;
        } else {
            total_lines += (count + content_width - 1) / content_width;
        }
    }
    total_lines.max(1)
}

pub fn char_display_width(c: char) -> usize {
    if c < ' ' {
        0
    } else if c <= '~' {
        1
    } else if ('\u{1100}'..='\u{115F}').contains(&c)
        || ('\u{2329}'..='\u{232A}').contains(&c)
        || ('\u{2E80}'..='\u{303E}').contains(&c)
        || ('\u{3040}'..='\u{A4CF}').contains(&c)
        || ('\u{AC00}'..='\u{D7A3}').contains(&c)
        || ('\u{F900}'..='\u{FAFF}').contains(&c)
        || ('\u{FE10}'..='\u{FE19}').contains(&c)
        || ('\u{FE30}'..='\u{FE6F}').contains(&c)
        || ('\u{FF00}'..='\u{FF60}').contains(&c)
        || ('\u{FFE0}'..='\u{FFE6}').contains(&c)
        || ('\u{1F300}'..='\u{1FAFF}').contains(&c)
        || ('\u{2600}'..='\u{27BF}').contains(&c)
    {
        2
    } else {
        1
    }
}

pub fn str_display_width_up_to(s: &str, char_limit: usize) -> usize {
    s.chars().take(char_limit).map(char_display_width).sum()
}

pub fn char_at_display_width(s: &str, target_width: usize) -> usize {
    let mut current_width = 0;
    for (idx, c) in s.chars().enumerate() {
        let w = char_display_width(c);
        if current_width + w > target_width {
            return idx;
        }
        current_width += w;
    }
    s.chars().count()
}

pub fn truncate_service(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}…", &s[..max_len - 1])
    } else {
        s.to_string()
    }
}
