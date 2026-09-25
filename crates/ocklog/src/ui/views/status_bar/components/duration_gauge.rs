use crate::utils::time::TimeFormatter;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

pub struct DurationGauge;

impl DurationGauge {
    pub fn from_timestamps(start_ts: &str, end_ts: &str) -> Option<Span<'static>> {
        let (s1, s2) = (
            TimeFormatter::parse_rfc3339_secs(start_ts)?,
            TimeFormatter::parse_rfc3339_secs(end_ts)?,
        );
        let diff = s2.abs_diff(s1);
        let t_start = TimeFormatter::format_hh_mm(start_ts)?;
        let t_end = TimeFormatter::format_hh_mm(end_ts)?;
        let text = format!("{} → {} ({})  ", t_start, t_end, TimeFormatter::format_duration(diff));

        Some(Span::styled(text, Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)))
    }
}
