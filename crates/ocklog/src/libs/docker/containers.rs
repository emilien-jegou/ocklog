use crate::libs::docker::client::DockerTransport;
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSummary {
    pub id: String,
    pub names: Vec<String>,
    pub primary_name: String,
    pub is_running: bool,
    pub has_error: bool,
    pub status: String,
}

impl ContainerSummary {
    pub fn from_json_value(item: &serde_json::Value) -> Self {
        let id = item["Id"].as_str().unwrap_or_default().to_string();
        let names = parse_names(&item["Names"]);
        let primary_name = names.first().cloned().unwrap_or_else(|| "unknown".to_string());
        let state = item["State"].as_str().unwrap_or_default();
        let status = item["Status"].as_str().unwrap_or_default().to_lowercase();
        let is_running = state == "running";
        let has_error = status.contains("exit") && !status.contains("exit 0");

        Self {
            id,
            names,
            primary_name,
            is_running,
            has_error,
            status,
        }
    }
}

fn parse_names(names_val: &serde_json::Value) -> Vec<String> {
    names_val
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|n| n.as_str())
                .map(|s| s.trim_start_matches('/').to_string())
                .collect()
        })
        .unwrap_or_default()
}

pub struct ContainerEngine {
    transport: DockerTransport,
}

impl ContainerEngine {
    pub fn new(transport: DockerTransport) -> Self {
        Self { transport }
    }

    pub async fn list(&self, all: bool) -> eyre::Result<Vec<ContainerSummary>> {
        let endpoint = format!("/containers/json?all={}", if all { 1 } else { 0 });
        let mut reader = self.transport.send_get_request(&endpoint, false).await?;

        let mut body = Vec::new();
        reader.read_to_end(&mut body).await?;

        let raw = String::from_utf8_lossy(&body);
        let items: Vec<serde_json::Value> = parse_json_payload(&raw).unwrap_or_default();

        Ok(items.iter().map(ContainerSummary::from_json_value).collect())
    }

    pub async fn restart(&self, id: &str) -> eyre::Result<()> {
        let endpoint = format!("/containers/{}/restart", id);
        self.transport.send_post_request(&endpoint).await
    }

    pub async fn stop(&self, id: &str) -> eyre::Result<()> {
        let endpoint = format!("/containers/{}/stop", id);
        self.transport.send_post_request(&endpoint).await
    }

    pub async fn kill(&self, id: &str) -> eyre::Result<()> {
        let endpoint = format!("/containers/{}/kill", id);
        self.transport.send_post_request(&endpoint).await
    }
}

fn parse_json_payload<T: serde::de::DeserializeOwned>(raw_http: &str) -> Option<T> {
    let (_, body) = raw_http.split_once("\r\n\r\n")?;

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
