use super::events::{QueryFilterReq, QueryFilterRes};
use crate::state::AppState;
use ocklog_tasker::{Listener, TaskerContext};
use parking_lot::RwLock;
use std::sync::Arc;

pub struct QueryFilterWorker;

#[derive(TaskerContext)]
pub struct QueryFilterCtx {
    pub state: Arc<RwLock<AppState>>,
}

impl Listener<QueryFilterReq, crate::worker::EventSender> for QueryFilterWorker {
    type Context = QueryFilterCtx;

    #[tracing::instrument(skip_all, fields(query = %event.query))]
    async fn handle(
        event: QueryFilterReq,
        ctx: Self::Context,
        tx: crate::worker::EventSender,
    ) -> eyre::Result<()> {
        let state = ctx.state.read();
        let query_lower = event.query.to_lowercase();

        let mut matches = Vec::new();
        for (idx, item) in state.logs.iter().enumerate() {
            if item.content.to_lowercase().contains(&query_lower)
                || item.service.to_lowercase().contains(&query_lower)
            {
                matches.push(idx);
            }
        }

        tx.send(QueryFilterRes {
            matched_indices: matches,
        })?;
        Ok(())
    }
}
