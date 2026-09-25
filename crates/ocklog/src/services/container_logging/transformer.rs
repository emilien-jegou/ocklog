use crate::ansi::{parse_ansi, AnsiSegment};
use crate::libs::docker::events::ContainerSystemEvent;
use crate::utils::text::TextUtils;
use crate::utils::time::TimeFormatter;

#[derive(Debug, Clone)]
pub struct ProcessedLogRecord {
    pub service_name: String,
    pub content: String,
    pub segments: Vec<AnsiSegment>,
    pub timestamp_rfc3339: Option<String>,
    pub timestamp_secs: Option<u64>,
    pub is_system: bool,
}

impl ProcessedLogRecord {
    pub fn tag_text(&self) -> String {
        if self.is_system {
            String::new()
        } else {
            let name = TextUtils::truncate(&self.service_name, 16);
            format!("[{}] ", name)
        }
    }

    pub fn prefix_len(&self) -> usize {
        if self.is_system { 0 } else { self.tag_text().chars().count() }
    }

    pub fn full_line_text(&self) -> String {
        if self.is_system { self.content.clone() } else { format!("{}{}", self.tag_text(), self.content) }
    }
}

pub struct LogTransformer;

impl LogTransformer {
    pub fn from_raw_bytes(service: &str, _is_stderr: bool, raw_bytes: &[u8]) -> Option<ProcessedLogRecord> {
        let line = String::from_utf8_lossy(raw_bytes);
        let trimmed = line.trim_matches(['\r', '\n']);
        if trimmed.trim().is_empty() {
            return None;
        }

        let (timestamp_rfc3339, content_str) = extract_timestamp(trimmed);
        let timestamp_secs = timestamp_rfc3339.as_deref().and_then(TimeFormatter::parse_rfc3339_secs);
        let (content, segments) = parse_ansi(content_str);

        Some(ProcessedLogRecord {
            service_name: service.to_string(),
            content,
            segments,
            timestamp_rfc3339,
            timestamp_secs,
            is_system: false,
        })
    }

    pub fn from_system_event(event: &ContainerSystemEvent) -> ProcessedLogRecord {
        let ts_str = TimeFormatter::unix_nanos_to_rfc3339(event.timestamp_nanos);
        let ts_secs = (event.timestamp_nanos / 1_000_000_000).max(0) as u64;
        let (content, segments) = parse_ansi(&event.lifecycle_msg);

        ProcessedLogRecord {
            service_name: event.container_name.clone(),
            content,
            segments,
            timestamp_rfc3339: Some(ts_str),
            timestamp_secs: Some(ts_secs),
            is_system: true,
        }
    }

    pub fn system_notice(message: &str) -> ProcessedLogRecord {
        let (content, segments) = parse_ansi(message);
        ProcessedLogRecord {
            service_name: "system".to_string(),
            content,
            segments,
            timestamp_rfc3339: None,
            timestamp_secs: None,
            is_system: true,
        }
    }
}

fn extract_timestamp(raw: &str) -> (Option<String>, &str) {
    let bytes = raw.as_bytes();
    if bytes.len() >= 20 && bytes[4] == b'-' && bytes[7] == b'-' && (bytes[10] == b'T' || bytes[10] == b' ') {
        if let Some(space_idx) = raw.find(' ') {
            if space_idx <= 36 {
                let ts = &raw[..space_idx];
                return (Some(TimeFormatter::normalize_rfc3339(ts)), raw[space_idx + 1..].trim_start());
            }
        }
    }
    (None, raw)
}
