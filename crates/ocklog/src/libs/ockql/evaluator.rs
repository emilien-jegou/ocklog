use crate::ansi::AnsiSegment;
use crate::libs::ockql::ast::{Expr, MatchPattern, StreamModifier, TimeBound};
use crate::services::container_logging::transformer::ProcessedLogRecord;
use ratatui::style::{Color, Modifier, Style};
use std::collections::{HashMap, HashSet, VecDeque};

pub struct QueryEvaluationContext<'a> {
    pub content: &'a str,
    pub timestamp_secs: Option<u64>,
    pub reference_now_secs: u64,
}

pub struct Evaluator;

impl Evaluator {
    pub fn matches(expr: &Expr, ctx: &QueryEvaluationContext<'_>) -> bool {
        match expr {
            Expr::Pattern(pattern) => Self::eval_pattern(pattern, ctx.content),
            Expr::Time(time_bound) => Self::eval_time(time_bound, ctx),
            Expr::Not(inner) => !Self::matches(inner, ctx),
            Expr::And(items) => items.iter().all(|e| Self::matches(e, ctx)),
            Expr::Or(items) => items.iter().any(|e| Self::matches(e, ctx)),
            Expr::Modifier(_) => true,
        }
    }

    pub fn execute_stream_modifiers(
        expr: &Expr,
        logs: &VecDeque<ProcessedLogRecord>,
        matched_indices: &[usize],
    ) -> (Vec<usize>, Option<Vec<ProcessedLogRecord>>) {
        let mut modifiers = Vec::new();
        Self::collect_modifiers(expr, &mut modifiers);

        if modifiers.is_empty() {
            return (matched_indices.to_vec(), None);
        }

        let mut current_indices = matched_indices.to_vec();
        let mut synthetic = None;

        for m in modifiers {
            match m {
                StreamModifier::Context(n) => current_indices = Self::apply_context(logs.len(), &current_indices, n, n),
                StreamModifier::After(n) => current_indices = Self::apply_context(logs.len(), &current_indices, 0, n),
                StreamModifier::First(n) => current_indices.truncate(n),
                StreamModifier::Last(n) => {
                    let skip = current_indices.len().saturating_sub(n);
                    current_indices = current_indices.into_iter().skip(skip).collect();
                }
                StreamModifier::Dedup { all } => {
                    let (new_idx, syn) = Self::apply_dedup(logs, &current_indices, all);
                    current_indices = new_idx;
                    synthetic = Some(syn);
                }
            }
        }

        (current_indices, synthetic)
    }

    fn collect_modifiers(expr: &Expr, out: &mut Vec<StreamModifier>) {
        match expr {
            Expr::Modifier(m) => out.push(m.clone()),
            Expr::And(items) | Expr::Or(items) => {
                for item in items {
                    Self::collect_modifiers(item, out);
                }
            }
            Expr::Not(inner) => Self::collect_modifiers(inner, out),
            _ => {}
        }
    }

    fn apply_context(total: usize, indices: &[usize], before: usize, after: usize) -> Vec<usize> {
        let mut set = HashSet::new();
        for &idx in indices {
            let start = idx.saturating_sub(before);
            let end = (idx + after + 1).min(total);
            for i in start..end {
                set.insert(i);
            }
        }
        let mut res: Vec<usize> = set.into_iter().collect();
        res.sort_unstable();
        res
    }

    fn apply_dedup(
        logs: &VecDeque<ProcessedLogRecord>,
        indices: &[usize],
        all: bool,
    ) -> (Vec<usize>, Vec<ProcessedLogRecord>) {
        let mut result_records = Vec::new();
        let mut result_indices = Vec::new();

        if all {
            let mut seen: HashMap<String, (usize, ProcessedLogRecord)> = HashMap::new();
            for &idx in indices {
                if let Some(record) = logs.get(idx) {
                    seen.entry(record.content.clone())
                        .and_modify(|(count, _)| *count += 1)
                        .or_insert((1, record.clone()));
                }
            }
            for (idx, (_, (count, mut rec))) in seen.into_iter().enumerate() {
                if count > 1 {
                    Self::apply_dedup_badge(&mut rec, count);
                }
                result_records.push(rec);
                result_indices.push(idx);
            }
        } else {
            let mut last_content: Option<String> = None;
            let mut repeat_count = 1;
            let mut last_record: Option<ProcessedLogRecord> = None;

            for &idx in indices {
                if let Some(rec) = logs.get(idx) {
                    if last_content.as_deref() == Some(&rec.content) {
                        repeat_count += 1;
                    } else {
                        if let Some(mut prev) = last_record.take() {
                            if repeat_count > 1 {
                                Self::apply_dedup_badge(&mut prev, repeat_count);
                            }
                            result_indices.push(result_records.len());
                            result_records.push(prev);
                        }
                        last_content = Some(rec.content.clone());
                        repeat_count = 1;
                        last_record = Some(rec.clone());
                    }
                }
            }
            if let Some(mut prev) = last_record {
                if repeat_count > 1 {
                    Self::apply_dedup_badge(&mut prev, repeat_count);
                }
                result_indices.push(result_records.len());
                result_records.push(prev);
            }
        }

        (result_indices, result_records)
    }

    fn apply_dedup_badge(rec: &mut ProcessedLogRecord, count: usize) {
        let badge_text = format!("(x{}) ", count);
        rec.content = format!("{}{}", badge_text, rec.content);

        let badge_style = Style::default()
            .bg(Color::Magenta)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD);

        rec.segments.insert(0, AnsiSegment {
            text: badge_text,
            style: badge_style,
        });
    }

    fn eval_pattern(p: &MatchPattern, content: &str) -> bool {
        match p {
            MatchPattern::Literal { pattern, case_sensitive } => {
                if *case_sensitive {
                    content.contains(pattern)
                } else {
                    content.to_lowercase().contains(&pattern.to_lowercase())
                }
            }
            MatchPattern::Glob { pattern, case_sensitive } => {
                let rx = glob_to_regex(pattern);
                Self::eval_regex(&rx, content, *case_sensitive)
            }
            MatchPattern::Regex { pattern, case_sensitive } => {
                Self::eval_regex(pattern, content, *case_sensitive)
            }
        }
    }

    fn eval_regex(pattern: &str, content: &str, case_sensitive: bool) -> bool {
        let pat = if case_sensitive { pattern.to_string() } else { format!("(?i){}", pattern) };
        regex::Regex::new(&pat).map(|r| r.is_match(content)).unwrap_or(false)
    }

    fn eval_time(t: &TimeBound, ctx: &QueryEvaluationContext<'_>) -> bool {
        let ts = match ctx.timestamp_secs {
            Some(time) => time,
            None => return false,
        };
        match t {
            TimeBound::Since(d) => ts >= ctx.reference_now_secs.saturating_sub(*d),
            TimeBound::Before(d) => ts <= ctx.reference_now_secs.saturating_sub(*d),
        }
    }
}

fn glob_to_regex(glob: &str) -> String {
    let mut out = String::from("^");
    for c in glob.chars() {
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            s if "\\.+*?()|[]{}^$".contains(s) => {
                out.push('\\');
                out.push(s);
            }
            normal => out.push(normal),
        }
    }
    out.push('$');
    out
}
