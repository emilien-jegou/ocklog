use crate::libs::docker::client::{read_chunked_exact, DockerTransport, StreamFlags};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::net::UnixStream;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockerStreamType {
    Stdout,
    Stderr,
    Raw,
}

#[derive(Debug, Clone)]
pub struct RawLogRecord {
    pub stream_type: DockerStreamType,
    pub bytes: Vec<u8>,
}

pub struct LogFetchParams<'a> {
    pub container_id: &'a str,
    pub follow: bool,
    pub timestamps: bool,
    pub tail: Option<usize>,
}

pub struct DockerLogStream {
    reader: BufReader<UnixStream>,
    flags: StreamFlags,
    chunk_rem: usize,
    buffer: Vec<u8>,
}

impl DockerLogStream {
    pub async fn open(transport: &DockerTransport, params: LogFetchParams<'_>) -> eyre::Result<Self> {
        let tail_str = params.tail.map(|t| t.to_string()).unwrap_or_else(|| "all".to_string());
        let endpoint = format!(
            "/containers/{}/logs?follow={}&stdout=1&stderr=1&timestamps={}&tail={}",
            params.container_id,
            if params.follow { 1 } else { 0 },
            if params.timestamps { 1 } else { 0 },
            tail_str
        );
        let mut reader = transport.send_get_request(&endpoint, params.follow).await?;
        let flags = StreamFlags::detect(&mut reader).await;

        Ok(Self {
            reader,
            flags,
            chunk_rem: 0,
            buffer: Vec::new(),
        })
    }

    pub async fn next_record(&mut self) -> Option<RawLogRecord> {
        if self.flags.is_multiplexed {
            self.read_multiplexed_frame().await
        } else {
            self.read_raw_line().await
        }
    }

    async fn read_multiplexed_frame(&mut self) -> Option<RawLogRecord> {
        let mut header = [0u8; 8];
        self.read_exact_bytes(&mut header).await.ok()?;

        if header[1] != 0 || header[2] != 0 || header[3] != 0 {
            let mut line = Vec::from(header);
            let _ = self.reader.read_until(b'\n', &mut line).await;
            self.flags.is_multiplexed = false;
            return Some(RawLogRecord { stream_type: DockerStreamType::Raw, bytes: line });
        }

        let stream_type = match header[0] {
            2 => DockerStreamType::Stderr,
            _ => DockerStreamType::Stdout,
        };
        let size = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let mut payload = vec![0u8; size];
        self.read_exact_bytes(&mut payload).await.ok()?;

        Some(RawLogRecord { stream_type, bytes: payload })
    }

    async fn read_raw_line(&mut self) -> Option<RawLogRecord> {
        self.buffer.clear();
        let bytes = self.reader.read_until(b'\n', &mut self.buffer).await.ok()?;
        if bytes == 0 {
            return None;
        }
        Some(RawLogRecord {
            stream_type: DockerStreamType::Raw,
            bytes: self.buffer.clone(),
        })
    }

    async fn read_exact_bytes(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        if self.flags.is_chunked {
            read_chunked_exact(&mut self.reader, buf, &mut self.chunk_rem).await
        } else {
            self.reader.read_exact(buf).await.map(|_| ())
        }
    }
}
