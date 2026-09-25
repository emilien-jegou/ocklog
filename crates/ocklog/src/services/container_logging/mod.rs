pub mod pipeline;
pub mod state;
pub mod transformer;

use crate::libs::docker::{ContainerSummary, DockerEngine};
use crate::libs::ockql::{Evaluator, OckQuery, QueryEvaluationContext};
use crate::services::container_logging::pipeline::{LoggingPipeline, LoggingPipelineParams};
use crate::services::container_logging::state::IngestionBufferState;
use crate::services::container_logging::transformer::{LogTransformer, ProcessedLogRecord};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinSet;

#[allow(unused)]
pub struct ContainerLoggingService {
    docker: Arc<DockerEngine>,
    buffer: IngestionBufferState,
    record_tx: UnboundedSender<ProcessedLogRecord>,
    record_rx: UnboundedReceiver<ProcessedLogRecord>,
}

#[allow(unused)]
impl ContainerLoggingService {
    pub fn new(docker: Arc<DockerEngine>, capacity: usize) -> Self {
        let (record_tx, record_rx) = unbounded_channel();
        Self {
            docker,
            buffer: IngestionBufferState::new(capacity),
            record_tx,
            record_rx,
        }
    }

    pub fn buffer(&self) -> &IngestionBufferState {
        &self.buffer
    }

    pub fn sender(&self) -> UnboundedSender<ProcessedLogRecord> {
        self.record_tx.clone()
    }

    pub fn inject_notice(&mut self, message: &str) {
        let notice = LogTransformer::system_notice(message);
        self.buffer.push(notice);
    }

    pub fn process_incoming(&mut self) -> usize {
        let mut count = 0;
        while let Ok(record) = self.record_rx.try_recv() {
            self.buffer.push(record);
            count += 1;
        }
        count
    }

    pub async fn fetch_initial_parallel(&self, containers: &[ContainerSummary]) {
        let mut set = JoinSet::new();
        for c in containers {
            let docker = Arc::clone(&self.docker);
            let cid = c.id.clone();
            let name = c.primary_name.clone();
            let tail = if c.is_running || c.has_error { 1000 } else { 100 };

            set.spawn(async move {
                let fetch_params = crate::libs::docker::logging::LogFetchParams {
                    container_id: &cid,
                    follow: false,
                    timestamps: true,
                    tail: Some(tail),
                };
                let mut logs = Vec::new();
                if let Ok(mut stream) = docker.stream_logs(fetch_params).await {
                    let mut line_buf = Vec::new();
                    while let Some(record) = stream.next_record().await {
                        line_buf.extend_from_slice(&record.bytes);
                        while let Some(pos) = line_buf.iter().position(|&b| b == b'\n') {
                            let line_bytes: Vec<u8> = line_buf.drain(..=pos).collect();
                            let is_err = record.stream_type == crate::libs::docker::logging::DockerStreamType::Stderr;
                            if let Some(processed) = LogTransformer::from_raw_bytes(&name, is_err, &line_bytes) {
                                logs.push(processed);
                            }
                        }
                    }
                }
                logs
            });
        }

        let mut initial_logs = Vec::new();
        while let Some(Ok(mut logs)) = set.join_next().await {
            initial_logs.append(&mut logs);
        }

        initial_logs.sort_by(|a, b| a.timestamp_rfc3339.cmp(&b.timestamp_rfc3339));
        for item in initial_logs {
            let _ = self.record_tx.send(item);
        }
    }

    pub async fn start_live_tail(&self, c: &ContainerSummary) -> eyre::Result<()> {
        let tx = self.record_tx.clone();
        let docker = Arc::clone(&self.docker);
        let id = c.id.clone();
        let name = c.primary_name.clone();

        tokio::spawn(async move {
            let params = LoggingPipelineParams { container_id: &id, service_name: &name, follow: true, tail: Some(0) };
            let _ = LoggingPipeline::run_stream(&docker, params, tx).await;
        });
        Ok(())
    }

    pub async fn start_events_listener(&self, status_tx: UnboundedSender<(String, bool, bool)>) -> eyre::Result<()> {
        let mut sub = self.docker.subscribe_events().await?;
        let log_tx = self.record_tx.clone();

        tokio::spawn(async move {
            while let Some(evt) = sub.next_event().await {
                let _ = status_tx.send((evt.container_id.clone(), evt.is_running, evt.has_error));
                let processed = LogTransformer::from_system_event(&evt);
                let _ = log_tx.send(processed);
            }
        });
        Ok(())
    }

    pub fn apply_filter(&mut self, query: Option<&OckQuery>, services: &HashMap<String, bool>, ref_now: u64) {
        let logs = self.buffer.all_logs();
        let base_matches: Vec<usize> = logs.iter().enumerate().filter_map(|(idx, log)| {
            if !services.is_empty() {
                let is_enabled = services.get(&log.service_name).copied().unwrap_or(false);
                if !is_enabled {
                    return None;
                }
            }

            match query {
                None => Some(idx),
                Some(q) => {
                    let ctx = QueryEvaluationContext { content: &log.content, timestamp_secs: log.timestamp_secs, reference_now_secs: ref_now };
                    if q.matches(&ctx) { Some(idx) } else { None }
                }
            }
        }).collect();

        if let Some(q) = query {
            let (visible, synthetic) = Evaluator::execute_stream_modifiers(q.ast(), logs, &base_matches);
            self.buffer.set_visible(visible, synthetic);
        } else {
            self.buffer.set_visible(base_matches, None);
        }
    }
}
