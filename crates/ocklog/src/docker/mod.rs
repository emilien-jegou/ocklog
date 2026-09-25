use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct DockerLogMsg {
    pub container_id: String,
    pub service_name: String,
    pub timestamp: Option<String>,
    pub message: String,
    pub stream_type: StreamType,
    pub is_system_event: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    Stdout,
    Stderr,
    System,
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub is_running: bool,
    pub has_error: bool,
}

const DOCKER_SOCKET: &str = "/var/run/docker.sock";

pub struct DockerClient {
    socket_path: String,
}

impl DockerClient {
    pub fn new() -> Self {
        Self {
            socket_path: DOCKER_SOCKET.to_string(),
        }
    }

    pub fn is_available(&self) -> bool {
        Path::new(&self.socket_path).exists()
    }

    async fn connect(&self) -> eyre::Result<UnixStream> {
        Ok(UnixStream::connect(&self.socket_path).await?)
    }

    pub async fn list_containers(&self) -> eyre::Result<Vec<ContainerInfo>> {
        let mut stream = self.connect().await?;
        let req = "GET /containers/json?all=1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
        stream.write_all(req.as_bytes()).await?;

        let mut reader = BufReader::new(stream);
        let mut response = Vec::new();
        reader.read_to_end(&mut response).await?;

        let raw_http = String::from_utf8_lossy(&response);
        let parsed: Vec<serde_json::Value> = parse_json_body(&raw_http).unwrap_or_default();

        let mut containers = Vec::new();
        for item in parsed {
            let id = item["Id"].as_str().unwrap_or("").to_string();
            let raw_name = item["Names"][0].as_str().unwrap_or("unknown");
            let name = raw_name.trim_start_matches('/').to_string();
            let state = item["State"].as_str().unwrap_or("");
            let status = item["Status"].as_str().unwrap_or("").to_lowercase();

            let is_running = state == "running";
            let has_error = status.contains("exit") && !status.contains("exit 0");

            containers.push(ContainerInfo {
                id,
                name,
                is_running,
                has_error,
            });
        }

        Ok(containers)
    }

    pub async fn fetch_initial_logs(
        &self,
        container_id: &str,
        service_name: &str,
    ) -> Vec<DockerLogMsg> {
        self.fetch_initial_logs_tail(container_id, service_name, 1000).await
    }

    pub async fn fetch_initial_logs_tail(
        &self,
        container_id: &str,
        service_name: &str,
        tail: usize,
    ) -> Vec<DockerLogMsg> {
        let mut results = Vec::new();
        let stream = match self.connect().await {
            Ok(s) => s,
            Err(_) => return results,
        };

        let path = format!(
            "/containers/{}/logs?follow=0&stdout=1&stderr=1&timestamps=1&tail={}",
            container_id, tail
        );
        let req = format!(
            "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            path
        );

        let mut stream = stream;
        if stream.write_all(req.as_bytes()).await.is_err() {
            return results;
        }

        let mut reader = BufReader::new(stream);
        let mut header_buf = String::new();
        let mut is_chunked = false;
        let mut is_multiplexed = true;

        loop {
            header_buf.clear();
            let bytes = match reader.read_line(&mut header_buf).await {
                Ok(b) => b,
                Err(_) => break,
            };
            if bytes == 0 || header_buf == "\r\n" || header_buf == "\n" {
                break;
            }
            let lower = header_buf.to_lowercase();
            if lower.contains("transfer-encoding") && lower.contains("chunked") {
                is_chunked = true;
            }
            if lower.contains("application/vnd.docker.raw-stream") {
                is_multiplexed = false;
            }
        }

        let mut chunk_rem = 0usize;
        let mut header = [0u8; 8];

        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();

        while is_multiplexed {
            let res = if is_chunked {
                read_chunked_exact(&mut reader, &mut header, &mut chunk_rem).await
            } else {
                reader.read_exact(&mut header).await.map(|_| ())
            };

            if res.is_err() {
                break;
            }

            if header[1] != 0 || header[2] != 0 || header[3] != 0 {
                let mut line = String::from_utf8_lossy(&header).to_string();
                let mut rest = String::new();
                let _ = reader.read_line(&mut rest).await;
                line.push_str(&rest);
                if let Some(msg) = parse_log_line(&line, container_id, service_name, StreamType::Stdout) {
                    results.push(msg);
                }
                is_multiplexed = false;
                break;
            }

            let stream_type = match header[0] {
                2 => StreamType::Stderr,
                _ => StreamType::Stdout,
            };
            let payload_size = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;

            let mut payload = vec![0u8; payload_size];
            let payload_res = if is_chunked {
                read_chunked_exact(&mut reader, &mut payload, &mut chunk_rem).await
            } else {
                reader.read_exact(&mut payload).await.map(|_| ())
            };

            if payload_res.is_err() {
                break;
            }

            let buf = match stream_type {
                StreamType::Stderr => &mut stderr_buf,
                _ => &mut stdout_buf,
            };
            buf.extend_from_slice(&payload);
            drain_line_buffer(buf, container_id, service_name, stream_type, &mut results);
        }

        flush_line_buffer(&mut stdout_buf, container_id, service_name, StreamType::Stdout, &mut results);
        flush_line_buffer(&mut stderr_buf, container_id, service_name, StreamType::Stderr, &mut results);

        if !is_multiplexed {
            let mut line = String::new();
            while let Ok(bytes) = reader.read_line(&mut line).await {
                if bytes == 0 {
                    break;
                }
                if let Some(msg) = parse_log_line(&line, container_id, service_name, StreamType::Stdout) {
                    results.push(msg);
                }
                line.clear();
            }
        }

        results
    }

    pub async fn start_live_ingestion_with_containers(
        self: Arc<Self>,
        containers: &[ContainerInfo],
        tx: mpsc::UnboundedSender<DockerLogMsg>,
        container_event_tx: mpsc::UnboundedSender<(String, bool, bool)>,
    ) -> eyre::Result<()> {
        for c in containers {
            if c.is_running {
                let client = Arc::clone(&self);
                let tx = tx.clone();
                let cid = c.id.clone();
                let name = c.name.clone();

                tokio::spawn(async move {
                    let _ = client.stream_live_logs(&cid, &name, tx).await;
                });
            }
        }

        let client_events = Arc::clone(&self);
        let initial_running: Vec<String> = containers.iter().filter(|c| c.is_running).map(|c| c.id.clone()).collect();
        tokio::spawn(async move {
            let _ = client_events.watch_events(tx, container_event_tx, initial_running).await;
        });

        Ok(())
    }

    pub async fn stream_live_logs(
        &self,
        container_id: &str,
        service_name: &str,
        tx: mpsc::UnboundedSender<DockerLogMsg>,
    ) -> eyre::Result<()> {
        let mut stream = self.connect().await?;
        let path = format!(
            "/containers/{}/logs?follow=1&stdout=1&stderr=1&timestamps=1&tail=0",
            container_id
        );

        let req = format!(
            "GET {} HTTP/1.1\r\nHost: localhost\r\nUpgrade: tcp\r\nConnection: Upgrade\r\n\r\n",
            path
        );
        stream.write_all(req.as_bytes()).await?;

        let mut reader = BufReader::new(stream);
        let mut header_buf = String::new();
        let mut is_chunked = false;
        let mut is_multiplexed = true;

        loop {
            header_buf.clear();
            let bytes = reader.read_line(&mut header_buf).await?;
            if bytes == 0 || header_buf == "\r\n" || header_buf == "\n" {
                break;
            }
            let lower = header_buf.to_lowercase();
            if lower.contains("transfer-encoding") && lower.contains("chunked") {
                is_chunked = true;
            }
            if lower.contains("application/vnd.docker.raw-stream") {
                is_multiplexed = false;
            }
        }

        let mut chunk_rem = 0usize;
        let mut header = [0u8; 8];
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();

        while is_multiplexed {
            let res = if is_chunked {
                read_chunked_exact(&mut reader, &mut header, &mut chunk_rem).await
            } else {
                reader.read_exact(&mut header).await.map(|_| ())
            };

            if res.is_err() {
                break;
            }

            if header[1] != 0 || header[2] != 0 || header[3] != 0 {
                let mut line = String::from_utf8_lossy(&header).to_string();
                let mut rest = String::new();
                let _ = reader.read_line(&mut rest).await;
                line.push_str(&rest);
                if let Some(msg) = parse_log_line(&line, container_id, service_name, StreamType::Stdout) {
                    let _ = tx.send(msg);
                }
                is_multiplexed = false;
                break;
            }

            let stream_type = match header[0] {
                2 => StreamType::Stderr,
                _ => StreamType::Stdout,
            };
            let payload_size = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;

            let mut payload = vec![0u8; payload_size];
            let payload_res = if is_chunked {
                read_chunked_exact(&mut reader, &mut payload, &mut chunk_rem).await
            } else {
                reader.read_exact(&mut payload).await.map(|_| ())
            };

            if payload_res.is_err() {
                break;
            }

            let buf = match stream_type {
                StreamType::Stderr => &mut stderr_buf,
                _ => &mut stdout_buf,
            };
            buf.extend_from_slice(&payload);
            drain_live_line_buffer(buf, container_id, service_name, stream_type, &tx);
        }

        if !is_multiplexed {
            let mut line = String::new();
            while let Ok(bytes) = reader.read_line(&mut line).await {
                if bytes == 0 {
                    break;
                }
                if let Some(msg) = parse_log_line(&line, container_id, service_name, StreamType::Stdout) {
                    let _ = tx.send(msg);
                }
                line.clear();
            }
        }

        Ok(())
    }

    async fn watch_events(
        self: Arc<Self>,
        log_tx: mpsc::UnboundedSender<DockerLogMsg>,
        container_status_tx: mpsc::UnboundedSender<(String, bool, bool)>,
        initial_running: Vec<String>,
    ) -> eyre::Result<()> {
        let mut stream = self.connect().await?;
        let req = "GET /events?filters=%7B%22type%22%3A%5B%22container%22%5D%7D HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(req.as_bytes()).await?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();

        let mut known_containers: HashSet<String> = initial_running.into_iter().collect();

        loop {
            line.clear();
            let bytes = reader.read_line(&mut line).await?;
            if bytes == 0 {
                break;
            }

            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                let action = v["Action"].as_str().unwrap_or("");
                let actor_id = v["Actor"]["ID"].as_str().unwrap_or("");
                let actor_name = v["Actor"]["Attributes"]["name"].as_str().unwrap_or("unknown");

                let (is_running, has_err) = match action {
                    "start" | "restart" => (true, false),
                    "die" | "kill" => (false, true),
                    _ => continue,
                };

                let _ = container_status_tx.send((actor_id.to_string(), is_running, has_err));

                let event_time = v["time"].as_i64().map(|secs| {
                    let nanos = v["timeNano"].as_i64().unwrap_or(secs * 1_000_000_000);
                    unix_nanos_to_normalized_rfc3339(nanos)
                });

                let is_restart = action == "restart" || (action == "start" && known_containers.contains(actor_id));

                let lifecycle_msg = match action {
                    "die" => {
                        let code = v["Actor"]["Attributes"]["exitCode"].as_str().unwrap_or("unknown");
                        format!("[{} - DIE - exit code: {}]", actor_name, code)
                    }
                    "restart" => format!("[{} - RESTART]", actor_name),
                    "start" => {
                        if is_restart {
                            format!("[{} - RESTART]", actor_name)
                        } else {
                            known_containers.insert(actor_id.to_string());
                            format!("[{} - START]", actor_name)
                        }
                    }
                    "kill" => {
                        let sig = v["Actor"]["Attributes"]["signal"].as_str().unwrap_or("");
                        if sig.is_empty() {
                            format!("[{} - KILL]", actor_name)
                        } else {
                            format!("[{} - KILL - signal: {}]", actor_name, sig)
                        }
                    }
                    _ => format!("[{} - {}]", actor_name, action.to_uppercase()),
                };

                let _ = log_tx.send(DockerLogMsg {
                    container_id: actor_id.to_string(),
                    service_name: actor_name.to_string(),
                    timestamp: event_time,
                    message: lifecycle_msg,
                    stream_type: StreamType::System,
                    is_system_event: true,
                });

                if is_running {
                    let client = Arc::clone(&self);
                    let tx = log_tx.clone();
                    let cid = actor_id.to_string();
                    let name = actor_name.to_string();

                    tokio::spawn(async move {
                        let _ = client.stream_live_logs(&cid, &name, tx).await;
                    });
                }
            }
        }

        Ok(())
    }
}

fn drain_line_buffer(
    buf: &mut Vec<u8>,
    container_id: &str,
    service_name: &str,
    stream_type: StreamType,
    out: &mut Vec<DockerLogMsg>,
) {
    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
        let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
        let raw = String::from_utf8_lossy(&line_bytes);
        if let Some(msg) = parse_log_line(&raw, container_id, service_name, stream_type) {
            out.push(msg);
        }
    }
}

fn flush_line_buffer(
    buf: &mut Vec<u8>,
    container_id: &str,
    service_name: &str,
    stream_type: StreamType,
    out: &mut Vec<DockerLogMsg>,
) {
    if !buf.is_empty() {
        let raw = String::from_utf8_lossy(buf);
        if let Some(msg) = parse_log_line(&raw, container_id, service_name, stream_type) {
            out.push(msg);
        }
        buf.clear();
    }
}

fn drain_live_line_buffer(
    buf: &mut Vec<u8>,
    container_id: &str,
    service_name: &str,
    stream_type: StreamType,
    tx: &mpsc::UnboundedSender<DockerLogMsg>,
) {
    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
        let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
        let raw = String::from_utf8_lossy(&line_bytes);
        if let Some(msg) = parse_log_line(&raw, container_id, service_name, stream_type) {
            let _ = tx.send(msg);
        }
    }
}

async fn read_chunked_exact<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
    buf: &mut [u8],
    chunk_rem: &mut usize,
) -> std::io::Result<()> {
    let mut target_filled = 0;
    while target_filled < buf.len() {
        if *chunk_rem == 0 {
            let mut line = String::new();
            loop {
                line.clear();
                let bytes = reader.read_line(&mut line).await?;
                if bytes == 0 {
                    return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof"));
                }
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let hex_str = trimmed.split(';').next().unwrap_or("0");
                *chunk_rem = usize::from_str_radix(hex_str, 16).map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk header")
                })?;
                break;
            }
            if *chunk_rem == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "zero chunk"));
            }
        }

        let to_read = (buf.len() - target_filled).min(*chunk_rem);
        reader.read_exact(&mut buf[target_filled..target_filled + to_read]).await?;
        target_filled += to_read;
        *chunk_rem -= to_read;

        if *chunk_rem == 0 {
            let mut crlf = [0u8; 2];
            let _ = reader.read_exact(&mut crlf).await;
        }
    }
    Ok(())
}

fn parse_log_line(
    raw: &str,
    container_id: &str,
    service_name: &str,
    stream_type: StreamType,
) -> Option<DockerLogMsg> {
    let trimmed = raw.trim_matches(['\r', '\n']);
    if trimmed.trim().is_empty() {
        return None;
    }
    let (timestamp, message) = extract_docker_timestamp(trimmed);
    Some(DockerLogMsg {
        container_id: container_id.to_string(),
        service_name: service_name.to_string(),
        timestamp,
        message: message.to_string(),
        stream_type,
        is_system_event: false,
    })
}

fn extract_docker_timestamp(raw: &str) -> (Option<String>, &str) {
    let bytes = raw.as_bytes();
    if bytes.len() >= 20 && bytes[4] == b'-' && bytes[7] == b'-' && (bytes[10] == b'T' || bytes[10] == b' ') {
        if let Some(space_idx) = raw.find(' ') {
            if space_idx <= 36 {
                let ts = &raw[..space_idx];
                let normalized = normalize_ts(ts);
                let msg = raw[space_idx + 1..].trim_start();
                return (Some(normalized), msg);
            }
        }
    }
    (None, raw)
}

pub fn normalize_ts(ts: &str) -> String {
    let clean = ts.trim_end_matches('Z');
    if let Some((main, frac)) = clean.split_once('.') {
        let frac_digits: String = frac.chars().filter(|c| c.is_ascii_digit()).collect();
        let mut frac_9 = frac_digits;
        if frac_9.len() > 9 {
            frac_9.truncate(9);
        } else {
            while frac_9.len() < 9 {
                frac_9.push('0');
            }
        }
        format!("{}.{}Z", main, frac_9)
    } else {
        format!("{}.000000000Z", clean)
    }
}

fn unix_nanos_to_normalized_rfc3339(nanos: i64) -> String {
    let secs = nanos / 1_000_000_000;
    let nsec = (nanos % 1_000_000_000).abs();

    let days = (secs / 86400) + 719468;
    let mut time = secs % 86400;
    if time < 0 {
        time += 86400;
    }
    let hour = time / 3600;
    let min = (time % 3600) / 60;
    let sec = time % 60;

    let era = (if days >= 0 { days } else { days - 146096 }) / 146097;
    let doe = (days - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z",
        y, m, d, hour, min, sec, nsec
    )
}

fn parse_json_body<T: serde::de::DeserializeOwned>(raw_http: &str) -> Option<T> {
    let (_headers, body) = raw_http.split_once("\r\n\r\n")?;

    if let Ok(val) = serde_json::from_str::<T>(body) {
        return Some(val);
    }

    let mut unchunked = Vec::new();
    let mut cursor = body.as_bytes();

    while !cursor.is_empty() {
        let newline_pos = cursor.windows(2).position(|w| w == b"\r\n")?;
        let size_str = std::str::from_utf8(&cursor[..newline_pos]).ok()?.trim();
        let size_hex = size_str.split(';').next().unwrap_or("0");
        let chunk_size = usize::from_str_radix(size_hex, 16).ok()?;

        if chunk_size == 0 {
            break;
        }

        let data_start = newline_pos + 2;
        let data_end = data_start + chunk_size;
        if cursor.len() < data_end {
            unchunked.extend_from_slice(&cursor[data_start..]);
            break;
        }

        unchunked.extend_from_slice(&cursor[data_start..data_end]);
        let next_start = (data_end + 2).min(cursor.len());
        cursor = &cursor[next_start..];
    }

    serde_json::from_slice(&unchunked).ok()
}
