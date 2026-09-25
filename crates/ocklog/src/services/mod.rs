pub mod container_logging;
pub mod filter;

use std::collections::HashMap;
use std::sync::Arc;
use crate::libs::docker::DockerEngine;
use container_logging::ContainerLoggingService;
use filter::FilterService;

pub struct ServiceRegistry {
    pub logging: ContainerLoggingService,
    pub filter: FilterService,
}

impl ServiceRegistry {
    pub fn new(docker: Arc<DockerEngine>, capacity: usize) -> Self {
        Self {
            logging: ContainerLoggingService::new(docker, capacity),
            filter: FilterService::new(),
        }
    }

    pub fn sync(&mut self, services: &HashMap<String, bool>, ref_now: u64) -> usize {
        let new_count = self.logging.process_incoming();
        if new_count > 0 || self.filter.prompt().mode == filter::prompt_state::PromptMode::Active {
            let active = self.filter.active_filter().map(|a| a.query());
            self.logging.apply_filter(active, services, ref_now);
        }
        new_count
    }
}
