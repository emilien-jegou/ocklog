use crate::libs::docker::client::DockerTransport;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct ContainerSystemEvent {
    pub container_id: String,
    pub container_name: String,
    pub action: String,
    pub is_running: bool,
    pub has_error: bool,
    pub is_destroyed: bool,
    pub lifecycle_msg: String,
    pub timestamp_nanos: i64,
}

impl ContainerSystemEvent {
    pub fn from_json_value(v: &serde_json::Value) -> Option<Self> {
        let action = v["Action"].as_str()?.to_string();

        // 1. Ignore routine docker probe/healthcheck exec spam
        if action.starts_with("exec_") {
            return None;
        }

        let actor_id = v["Actor"]["ID"].as_str()?.to_string();
        let actor_name = v["Actor"]["Attributes"]["name"].as_str().unwrap_or("unknown").to_string();

        let is_destroyed = matches!(action.as_str(), "destroy" | "delete" | "prune");
        let (is_running, has_error) = match action.as_str() {
            "start" | "restart" => (true, false),
            "die" | "kill" => (false, true),
            _ => (false, false),
        };

        // 2. Format lifecycle and healthcheck events cleanly
        let msg = match action.as_str() {
            "die" => {
                let code = v["Actor"]["Attributes"]["exitCode"].as_str().unwrap_or("unknown");
                format!("[{} - DIE - exit code: {}]", actor_name, code)
            }
            "restart" => format!("[{} - RESTART]", actor_name),
            "start" => format!("[{} - START]", actor_name),
            "kill" => {
                let sig = v["Actor"]["Attributes"]["signal"].as_str().unwrap_or("");
                if sig.is_empty() { format!("[{} - KILL]", actor_name) } else { format!("[{} - KILL - signal: {}]", actor_name, sig) }
            }
            "health_status: healthy" => format!("[{} - HEALTHY]", actor_name),
            "health_status: unhealthy" => format!("[{} - UNHEALTHY]", actor_name),
            "destroy" | "delete" => format!("[{} - REMOVED]", actor_name),
            _ => return None,
        };

        let secs = v["time"].as_i64().unwrap_or(0);
        let timestamp_nanos = v["timeNano"].as_i64().unwrap_or(secs * 1_000_000_000);

        Some(Self {
            container_id: actor_id,
            container_name: actor_name,
            action,
            is_running,
            has_error,
            is_destroyed,
            lifecycle_msg: msg,
            timestamp_nanos,
        })
    }
}

pub struct EventSubscription {
    reader: BufReader<UnixStream>,
}

impl EventSubscription {
    pub async fn connect(transport: &DockerTransport) -> eyre::Result<Self> {
        let endpoint = "/events?filters=%7B%22type%22%3A%5B%22container%22%5D%7D";
        let reader = transport.send_get_request(endpoint, false).await?;
        Ok(Self { reader })
    }

    pub async fn next_event(&mut self) -> Option<ContainerSystemEvent> {
        let mut line = String::new();
        while self.reader.read_line(&mut line).await.ok()? > 0 {
            let trimmed = line.trim();
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(event) = ContainerSystemEvent::from_json_value(&val) {
                    return Some(event);
                }
            }
            line.clear();
        }
        None
    }
}
