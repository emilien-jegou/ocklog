#[derive(Debug, Clone)]
pub struct ServicePickerOption {
    pub id: String,
    pub name: String,
    pub is_running: bool,
    pub has_error: bool,
    pub is_removed: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ServicePickerState {
    pub is_open: bool,
    pub is_searching: bool,
    pub search_query: String,
    pub selected_idx: usize,
    pub scroll_offset: usize,
    pub options: Vec<ServicePickerOption>,
}

impl ServicePickerState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            is_searching: false,
            search_query: String::new(),
            selected_idx: 0,
            scroll_offset: 0,
            options: Vec::new(),
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        if !self.is_searching || self.search_query.is_empty() {
            return (0..self.options.len()).collect();
        }
        let q = self.search_query.to_lowercase();
        self.options
            .iter()
            .enumerate()
            .filter(|(_, o)| o.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn select_next(&mut self) {
        let len = self.filtered_indices().len();
        if len > 0 && self.selected_idx + 1 < len {
            self.selected_idx += 1;
        }
    }

    pub fn select_prev(&mut self) {
        self.selected_idx = self.selected_idx.saturating_sub(1);
    }

    pub fn toggle_selected(&mut self) {
        let filtered = self.filtered_indices();
        if let Some(&idx) = filtered.get(self.selected_idx) {
            if let Some(opt) = self.options.get_mut(idx) {
                opt.enabled = !opt.enabled;
            }
        }
    }

    pub fn set_all(&mut self, enabled: bool) {
        for opt in &mut self.options {
            opt.enabled = enabled;
        }
    }

    pub fn select_only_current(&mut self) {
        let filtered = self.filtered_indices();
        if let Some(&target) = filtered.get(self.selected_idx) {
            for (i, opt) in self.options.iter_mut().enumerate() {
                opt.enabled = i == target;
            }
        }
    }
}
