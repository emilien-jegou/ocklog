use crate::libs::ockql::OckQuery;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMode {
    Inactive,
    Active,
}

#[derive(Debug, Clone)]
pub struct PromptState {
    pub mode: PromptMode,
    pub input_buffer: String,
    pub cursor_col: usize,
    pub preview_ast: Option<OckQuery>,
}

impl PromptState {
    pub fn new() -> Self {
        Self {
            mode: PromptMode::Inactive,
            input_buffer: String::new(),
            cursor_col: 0,
            preview_ast: None,
        }
    }

    pub fn activate(&mut self) {
        self.mode = PromptMode::Active;
    }

    pub fn deactivate(&mut self) {
        self.mode = PromptMode::Inactive;
        self.preview_ast = None;
    }

    pub fn insert_char(&mut self, c: char) {
        self.input_buffer.insert(self.cursor_col, c);
        self.cursor_col += 1;
        self.recompile_preview();
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            self.input_buffer.remove(self.cursor_col);
            self.recompile_preview();
        }
    }

    pub fn move_cursor_left(&mut self) {
        self.cursor_col = self.cursor_col.saturating_sub(1);
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_col < self.input_buffer.chars().count() {
            self.cursor_col += 1;
        }
    }

    fn recompile_preview(&mut self) {
        let trimmed = self.input_buffer.trim();
        self.preview_ast = if trimmed.is_empty() {
            None
        } else {
            OckQuery::parse(trimmed).ok()
        };
    }
}
