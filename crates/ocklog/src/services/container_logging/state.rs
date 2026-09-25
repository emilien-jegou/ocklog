use crate::services::container_logging::transformer::ProcessedLogRecord;
use std::collections::{HashSet, VecDeque};

#[derive(Debug)]
pub struct IngestionBufferState {
    logs: VecDeque<ProcessedLogRecord>,
    recent_signatures: HashSet<u64>,
    signature_queue: VecDeque<u64>,
    max_capacity: usize,
    visible_indices: Vec<usize>,
    synthetic_logs: Option<Vec<ProcessedLogRecord>>,
}

impl IngestionBufferState {
    pub fn new(capacity: usize) -> Self {
        Self {
            logs: VecDeque::with_capacity(capacity),
            recent_signatures: HashSet::with_capacity(512),
            signature_queue: VecDeque::with_capacity(512),
            max_capacity: capacity,
            visible_indices: Vec::new(),
            synthetic_logs: None,
        }
    }

    pub fn push(&mut self, record: ProcessedLogRecord) {
        let sig = compute_signature(&record);
        if self.recent_signatures.contains(&sig) {
            return;
        }

        if self.signature_queue.len() >= 500 {
            if let Some(old_sig) = self.signature_queue.pop_front() {
                self.recent_signatures.remove(&old_sig);
            }
        }
        self.recent_signatures.insert(sig);
        self.signature_queue.push_back(sig);

        if self.logs.len() >= self.max_capacity {
            self.logs.pop_front();
            self.shift_visible_on_pop();
        }

        let insert_idx = if let Some(ref ts) = record.timestamp_rfc3339 {
            let mut idx = self.logs.len();
            while idx > 0 && self.logs[idx - 1].timestamp_rfc3339.as_deref() > Some(ts) {
                idx -= 1;
            }
            self.logs.insert(idx, record);
            idx
        } else {
            self.logs.push_back(record);
            self.logs.len() - 1
        };

        if self.synthetic_logs.is_none() {
            Self::insert_visible_index(&mut self.visible_indices, insert_idx);
        }
    }

    fn insert_visible_index(indices: &mut Vec<usize>, insert_idx: usize) {
        if indices.is_empty() || insert_idx >= *indices.last().unwrap() {
            indices.push(insert_idx);
            return;
        }

        for idx in indices.iter_mut() {
            if *idx >= insert_idx {
                *idx += 1;
            }
        }

        match indices.binary_search(&insert_idx) {
            Ok(pos) | Err(pos) => indices.insert(pos, insert_idx),
        }
    }

    fn shift_visible_on_pop(&mut self) {
        self.visible_indices.retain_mut(|idx| {
            if *idx == 0 {
                false
            } else {
                *idx -= 1;
                true
            }
        });
    }

    pub fn get_visible(&self, visible_row: usize) -> Option<&ProcessedLogRecord> {
        if let Some(ref syn) = self.synthetic_logs {
            syn.get(visible_row)
        } else {
            let &actual_idx = self.visible_indices.get(visible_row)?;
            self.logs.get(actual_idx)
        }
    }

    pub fn visible_count(&self) -> usize {
        if let Some(ref syn) = self.synthetic_logs {
            syn.len()
        } else {
            self.visible_indices.len()
        }
    }

    pub fn set_visible(&mut self, indices: Vec<usize>, synthetic: Option<Vec<ProcessedLogRecord>>) {
        self.visible_indices = indices;
        self.synthetic_logs = synthetic;
    }

    pub fn all_logs(&self) -> &VecDeque<ProcessedLogRecord> {
        &self.logs
    }
}

fn compute_signature(record: &ProcessedLogRecord) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    record.service_name.hash(&mut hasher);
    record.content.hash(&mut hasher);
    record.timestamp_rfc3339.hash(&mut hasher);
    hasher.finish()
}
