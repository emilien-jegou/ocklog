pub mod active_query;
pub mod prompt_state;

use active_query::ActiveFilter;
use prompt_state::PromptState;

#[derive(Debug)]
pub struct FilterService {
    active_filter: Option<ActiveFilter>,
    prompt: PromptState,
}

#[allow(unused)]
impl FilterService {
    pub fn new() -> Self {
        Self {
            active_filter: None,
            prompt: PromptState::new(),
        }
    }

    pub fn prompt(&self) -> &PromptState {
        &self.prompt
    }

    pub fn prompt_mut(&mut self) -> &mut PromptState {
        &mut self.prompt
    }

    pub fn active_filter(&self) -> Option<&ActiveFilter> {
        self.active_filter.as_ref()
    }

    pub fn submit_prompt(&mut self) -> Result<(), String> {
        let input = self.prompt.input_buffer.trim();
        if input.is_empty() {
            self.active_filter = None;
        } else {
            let filter = ActiveFilter::compile(input)?;
            self.active_filter = Some(filter);
        }
        self.prompt.deactivate();
        Ok(())
    }

    pub fn clear_active(&mut self) {
        self.active_filter = None;
        self.prompt.input_buffer.clear();
        self.prompt.cursor_col = 0;
    }
}
