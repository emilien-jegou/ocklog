pub mod context;
pub mod events;
pub mod tasks;

use ocklog_tasker::tasker_registry;

tasker_registry! {
    events = [
        QueryFilterReq => events::QueryFilterReq,
        QueryFilterRes => events::QueryFilterRes,
    ],
    listeners = [
        QueryFilterReq => [tasks::QueryFilterWorker],
    ],
}
