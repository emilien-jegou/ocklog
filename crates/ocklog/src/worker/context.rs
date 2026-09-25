use crate::state::AppState;
use ocklog_tasker::TaskerProvide;
use parking_lot::RwLock;
use std::sync::Arc;
use typed_builder::TypedBuilder;

#[derive(TypedBuilder, TaskerProvide, Clone)]
pub struct AppWorkerContext {
    pub state: Arc<RwLock<AppState>>,
}
