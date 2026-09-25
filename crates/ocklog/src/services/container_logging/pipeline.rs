use crate::libs::docker::logging::{DockerLogStream, LogFetchParams};
use crate::libs::docker::DockerEngine;
use crate::services::container_logging::transformer::{LogTransformer, ProcessedLogRecord};
use tokio::sync::mpsc::UnboundedSender;

pub struct LoggingPipelineParams<'a> {
    pub container_id: &'a str,
    pub service_name: &'a str,
    pub follow: bool,
    pub tail: Option<usize>,
}

pub struct LoggingPipeline;

impl LoggingPipeline {
    pub async fn run_stream(
        docker: &DockerEngine,
        params: LoggingPipelineParams<'_>,
        sender: UnboundedSender<ProcessedLogRecord>,
    ) -> eyre::Result<()> {
        let fetch_params = LogFetchParams {
            container_id: params.container_id,
            follow: params.follow,
            timestamps: true,
            tail: params.tail,
        };

        let mut stream: DockerLogStream = docker.stream_logs(fetch_params).await?;
        let mut line_buf = Vec::new();

        while let Some(record) = stream.next_record().await {
            line_buf.extend_from_slice(&record.bytes);
            while let Some(pos) = line_buf.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = line_buf.drain(..=pos).collect();
                let is_err = record.stream_type == crate::libs::docker::logging::DockerStreamType::Stderr;
                if let Some(processed) = LogTransformer::from_raw_bytes(params.service_name, is_err, &line_bytes) {
                    if sender.send(processed).is_err() {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }
}
