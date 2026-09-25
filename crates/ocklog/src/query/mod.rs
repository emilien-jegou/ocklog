use crate::state::LogItem;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum Matcher {
    Literal {
        text: String,
        case_sensitive: bool,
    },
    Glob {
        pattern: String,
        case_sensitive: bool,
    },
    Regex {
        pattern: String,
        case_sensitive: bool,
        compiled: regex::Regex,
    },
}

impl PartialEq for Matcher {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Matcher::Literal { text: t1, case_sensitive: c1 },
                Matcher::Literal { text: t2, case_sensitive: c2 },
            ) => t1 == t2 && c1 == c2,
            (
                Matcher::Glob { pattern: p1, case_sensitive: c1 },
                Matcher::Glob { pattern: p2, case_sensitive: c2 },
            ) => p1 == p2 && c1 == c2,
            (
                Matcher::Regex { pattern: p1, case_sensitive: c1, .. },
                Matcher::Regex { pattern: p2, case_sensitive: c2, .. },
            ) => p1 == p2 && c1 == c2,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Match(Matcher),
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupMode {
    Consecutive,
    All,
    Window(Duration),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Modifier {
    Filter(Expr),
    Since(Duration),
    BeforeTime(Duration),
    Context { before: usize, after: usize },
    BeforeContext(usize),
    AfterContext(usize),
    First(usize),
    Last(usize),
    Deduplicate(DedupMode),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub base: Option<Expr>,
    pub pipeline: Vec<Modifier>,
}

impl Query {
    pub fn parse(input: &str) -> eyre::Result<Self> {
        let input = input.trim();
        if input.is_empty() {
            return Ok(Query {
                base: None,
                pipeline: Vec::new(),
            });
        }

        let stages = split_pipeline_stages(input);
        if stages.is_empty() {
            return Ok(Query {
                base: None,
                pipeline: Vec::new(),
            });
        }

        let mut pipeline = Vec::new();
        let mut base = None;

        let stage0 = stages[0].trim();
        if !stage0.is_empty() {
            if let Some(modifier) = parse_modifier_stage(stage0)? {
                match modifier {
                    Modifier::Filter(expr) => base = Some(expr),
                    other => pipeline.push(other),
                }
            }
        }

        for stage_str in &stages[1..] {
            let stage_trimmed = stage_str.trim();
            if stage_trimmed.is_empty() {
                continue;
            }
            if let Some(modifier) = parse_modifier_stage(stage_trimmed)? {
                pipeline.push(modifier);
            }
        }

        Ok(Query { base, pipeline })
    }

    pub fn matches(&self, log: &LogItem, reference_now_secs: u64) -> bool {
        if let Some(ref base_expr) = self.base {
            if !eval_expr(base_expr, log) {
                return false;
            }
        }

        for modifier in &self.pipeline {
            match modifier {
                Modifier::Filter(expr) => {
                    if !eval_expr(expr, log) {
                        return false;
                    }
                }
                Modifier::Since(dur) => {
                    if let Some(ref ts) = log.timestamp {
                        if let Some(log_secs) = parse_timestamp_secs(ts) {
                            let threshold = reference_now_secs.saturating_sub(dur.as_secs());
                            if log_secs < threshold {
                                return false;
                            }
                        }
                    }
                }
                Modifier::BeforeTime(dur) => {
                    if let Some(ref ts) = log.timestamp {
                        if let Some(log_secs) = parse_timestamp_secs(ts) {
                            let threshold = reference_now_secs.saturating_sub(dur.as_secs());
                            if log_secs > threshold {
                                return false;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        true
    }

    pub fn has_stream_modifiers(&self) -> bool {
        self.pipeline.iter().any(|m| {
            matches!(
                m,
                Modifier::Context { .. }
                    | Modifier::BeforeContext(_)
                    | Modifier::AfterContext(_)
                    | Modifier::First(_)
                    | Modifier::Last(_)
                    | Modifier::Deduplicate(_)
            )
        })
    }

    pub fn execute_on_deque(
        &self,
        logs: &VecDeque<LogItem>,
        is_service_enabled: impl Fn(&str) -> bool,
        reference_now_secs: u64,
    ) -> (Vec<usize>, Option<Vec<LogItem>>) {
        let mut current_items: Vec<(usize, LogItem)> = logs
            .iter()
            .enumerate()
            .filter(|(_, log)| {
                is_service_enabled(&log.service)
                    && self.base.as_ref().map_or(true, |base_expr| eval_expr(base_expr, log))
            })
            .map(|(idx, log)| (idx, log.clone()))
            .collect();

        let mut has_deduplicated = false;

        for modifier in &self.pipeline {
            match modifier {
                Modifier::Filter(expr) => {
                    current_items.retain(|(_, log)| eval_expr(expr, log));
                }
                Modifier::Since(dur) => {
                    let threshold = reference_now_secs.saturating_sub(dur.as_secs());
                    current_items.retain(|(_, log)| {
                        log.timestamp
                            .as_deref()
                            .and_then(parse_timestamp_secs)
                            .map_or(true, |secs| secs >= threshold)
                    });
                }
                Modifier::BeforeTime(dur) => {
                    let threshold = reference_now_secs.saturating_sub(dur.as_secs());
                    current_items.retain(|(_, log)| {
                        log.timestamp
                            .as_deref()
                            .and_then(parse_timestamp_secs)
                            .map_or(true, |secs| secs <= threshold)
                    });
                }
                Modifier::Context { before, after } => {
                    current_items = expand_context_deque(&current_items, logs, *before, *after, &is_service_enabled);
                }
                Modifier::BeforeContext(lines) => {
                    current_items = expand_context_deque(&current_items, logs, *lines, 0, &is_service_enabled);
                }
                Modifier::AfterContext(lines) => {
                    current_items = expand_context_deque(&current_items, logs, 0, *lines, &is_service_enabled);
                }
                Modifier::First(n) => {
                    current_items.truncate(*n);
                }
                Modifier::Last(n) => {
                    if current_items.len() > *n {
                        current_items = current_items.split_off(current_items.len() - *n);
                    }
                }
                Modifier::Deduplicate(mode) => {
                    has_deduplicated = true;
                    current_items = deduplicate_items(current_items, *mode);
                }
            }
        }

        let indices = current_items.iter().map(|(idx, _)| *idx).collect();
        if has_deduplicated {
            let synthetic = current_items.into_iter().map(|(_, item)| item).collect();
            (indices, Some(synthetic))
        } else {
            (indices, None)
        }
    }
}

fn expand_context_deque(
    current: &[(usize, LogItem)],
    logs: &VecDeque<LogItem>,
    before: usize,
    after: usize,
    is_service_enabled: &impl Fn(&str) -> bool,
) -> Vec<(usize, LogItem)> {
    let mut needed_indices = BTreeSet::new();
    let total = logs.len();

    for (orig_idx, _) in current {
        let start = orig_idx.saturating_sub(before);
        let end = (*orig_idx + after).min(total.saturating_sub(1));
        for k in start..=end {
            if let Some(log) = logs.get(k) {
                if is_service_enabled(&log.service) {
                    needed_indices.insert(k);
                }
            }
        }
    }

    needed_indices
        .into_iter()
        .filter_map(|idx| logs.get(idx).map(|log| (idx, log.clone())))
        .collect()
}

fn deduplicate_items(
    items: Vec<(usize, LogItem)>,
    mode: DedupMode,
) -> Vec<(usize, LogItem)> {
    match mode {
        DedupMode::Consecutive => deduplicate_consecutive(items),
        DedupMode::All => deduplicate_all(items),
        DedupMode::Window(window) => deduplicate_windowed(items, window),
    }
}

fn deduplicate_consecutive(items: Vec<(usize, LogItem)>) -> Vec<(usize, LogItem)> {
    struct Run {
        orig_index: usize,
        log: LogItem,
        raw_message: String,
        count: usize,
    }

    let mut runs: Vec<Run> = Vec::new();

    for (orig_idx, log) in items {
        let (item_count, raw_message) = {
            let (count, msg) = parse_count_and_message(&log.content);
            (count, msg.to_string())
        };

        if let Some(last_run) = runs.last_mut() {
            if last_run.log.service == log.service && last_run.raw_message == raw_message {
                last_run.count += item_count;
                continue;
            }
        }

        runs.push(Run {
            orig_index: orig_idx,
            log,
            raw_message,
            count: item_count,
        });
    }

    runs.into_iter()
        .map(|run| {
            if run.count > 1 {
                (run.orig_index, build_counted_log_item(&run.log, run.count, &run.raw_message))
            } else {
                (run.orig_index, run.log)
            }
        })
        .collect()
}

fn deduplicate_all(items: Vec<(usize, LogItem)>) -> Vec<(usize, LogItem)> {
    struct Entry {
        orig_index: usize,
        log: LogItem,
        raw_message: String,
        count: usize,
    }

    let mut entries: Vec<Entry> = Vec::new();
    let mut key_map: HashMap<(String, String), usize> = HashMap::new();

    for (orig_idx, log) in items {
        let (item_count, raw_message) = {
            let (count, msg) = parse_count_and_message(&log.content);
            (count, msg.to_string())
        };
        let key = (log.service.clone(), raw_message.clone());

        if let Some(&entry_idx) = key_map.get(&key) {
            entries[entry_idx].count += item_count;
        } else {
            let new_idx = entries.len();
            entries.push(Entry {
                orig_index: orig_idx,
                log,
                raw_message,
                count: item_count,
            });
            key_map.insert(key, new_idx);
        }
    }

    entries
        .into_iter()
        .map(|entry| {
            if entry.count > 1 {
                (entry.orig_index, build_counted_log_item(&entry.log, entry.count, &entry.raw_message))
            } else {
                (entry.orig_index, entry.log)
            }
        })
        .collect()
}

fn deduplicate_windowed(items: Vec<(usize, LogItem)>, window: Duration) -> Vec<(usize, LogItem)> {
    struct Entry {
        orig_index: usize,
        log: LogItem,
        raw_message: String,
        count: usize,
        last_secs: Option<u64>,
    }

    let mut entries: Vec<Entry> = Vec::new();
    let mut key_map: HashMap<(String, String), usize> = HashMap::new();

    for (orig_idx, log) in items {
        let (item_count, raw_message) = {
            let (count, msg) = parse_count_and_message(&log.content);
            (count, msg.to_string())
        };
        let key = (log.service.clone(), raw_message.clone());
        let log_secs = log.timestamp.as_deref().and_then(parse_timestamp_secs);

        let mut merged = false;
        if let Some(&entry_idx) = key_map.get(&key) {
            let entry = &mut entries[entry_idx];
            let within_window = match (entry.last_secs, log_secs) {
                (Some(last), Some(current)) => current.saturating_sub(last) <= window.as_secs(),
                _ => true,
            };

            if within_window {
                entry.count += item_count;
                if log_secs.is_some() {
                    entry.last_secs = log_secs;
                }
                merged = true;
            }
        }

        if !merged {
            let new_idx = entries.len();
            entries.push(Entry {
                orig_index: orig_idx,
                log,
                raw_message,
                count: item_count,
                last_secs: log_secs,
            });
            key_map.insert(key, new_idx);
        }
    }

    entries
        .into_iter()
        .map(|entry| {
            if entry.count > 1 {
                (entry.orig_index, build_counted_log_item(&entry.log, entry.count, &entry.raw_message))
            } else {
                (entry.orig_index, entry.log)
            }
        })
        .collect()
}

fn build_counted_log_item(original: &LogItem, count: usize, raw_message: &str) -> LogItem {
    let formatted = format!("\x1b[30;47;1m {}x \x1b[0m — {}", count, raw_message);
    LogItem::new(
        original.service.clone(),
        formatted,
        original.level.clone(),
        original.timestamp.clone(),
        original.is_system,
    )
}

fn parse_count_and_message(content: &str) -> (usize, &str) {
    let trimmed = content.trim_start();

    if let Some(dash_idx) = trimmed.find(" — ").or_else(|| trimmed.find(" - ")) {
        let prefix = &trimmed[..dash_idx].trim();
        if prefix.ends_with('x') || prefix.ends_with('X') {
            let num_part = &prefix[..prefix.len() - 1];
            if let Ok(count) = num_part.parse::<usize>() {
                let rest = trimmed[dash_idx + 3..].trim_start();
                return (count, rest);
            }
        }
    }

    if trimmed.starts_with('[') {
        if let Some(close_bracket) = trimmed.find(']') {
            let inner = &trimmed[1..close_bracket];
            if inner.ends_with('x') || inner.ends_with('X') {
                let num_part = &inner[..inner.len() - 1];
                if let Ok(count) = num_part.parse::<usize>() {
                    let rest = trimmed[close_bracket + 1..].trim_start();
                    return (count, rest);
                }
            }
        }
    }

    (1, content)
}

fn eval_expr(expr: &Expr, log: &LogItem) -> bool {
    match expr {
        Expr::Match(matcher) => eval_matcher(matcher, &log.content, &log.service),
        Expr::Not(inner) => !eval_expr(inner, log),
        Expr::And(subs) => subs.iter().all(|sub| eval_expr(sub, log)),
        Expr::Or(subs) => subs.iter().any(|sub| eval_expr(sub, log)),
    }
}

fn eval_matcher(matcher: &Matcher, content: &str, service: &str) -> bool {
    match matcher {
        Matcher::Literal { text, case_sensitive } => {
            if *case_sensitive {
                content.contains(text) || service.contains(text)
            } else {
                let target = text.to_lowercase();
                content.to_lowercase().contains(&target) || service.to_lowercase().contains(&target)
            }
        }
        Matcher::Glob { pattern, case_sensitive } => {
            if *case_sensitive {
                glob_contains(pattern, content) || glob_contains(pattern, service)
            } else {
                let target_pat = pattern.to_lowercase();
                glob_contains(&target_pat, &content.to_lowercase())
                    || glob_contains(&target_pat, &service.to_lowercase())
            }
        }
        Matcher::Regex { compiled, .. } => {
            compiled.is_match(content) || compiled.is_match(service)
        }
    }
}

fn glob_contains(pattern: &str, text: &str) -> bool {
    let pat = format!("*{}*", pattern.trim_matches('*'));
    let p_chars: Vec<char> = pat.chars().collect();
    let t_chars: Vec<char> = text.chars().collect();
    match_glob_slice(&p_chars, &t_chars)
}

fn match_glob_slice(pat: &[char], txt: &[char]) -> bool {
    let mut p = 0;
    let mut t = 0;
    let mut star_p = None;
    let mut star_t = 0;

    while t < txt.len() {
        if p < pat.len() && (pat[p] == '?' || pat[p] == txt[t]) {
            p += 1;
            t += 1;
        } else if p < pat.len() && pat[p] == '*' {
            star_p = Some(p);
            p += 1;
            star_t = t;
        } else if let Some(sp) = star_p {
            p = sp + 1;
            star_t += 1;
            t = star_t;
        } else {
            return false;
        }
    }

    while p < pat.len() && pat[p] == '*' {
        p += 1;
    }

    p == pat.len()
}

pub fn is_quote_char(c: char) -> bool {
    c == '"' || c == '\'' || c == '`'
}

/// Splits by pipeline separator '|', ignoring '|' inside "", '', or `` with escape support
fn split_pipeline_stages(input: &str) -> Vec<String> {
    let mut stages = Vec::new();
    let mut current = String::new();
    let mut active_quote: Option<char> = None;
    let mut escaped = false;

    for c in input.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }

        if c == '\\' {
            escaped = true;
            current.push(c);
            continue;
        }

        if let Some(q) = active_quote {
            if c == q {
                active_quote = None;
            }
            current.push(c);
        } else if is_quote_char(c) {
            active_quote = Some(c);
            current.push(c);
        } else if c == '|' {
            stages.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }

    if !current.is_empty() {
        stages.push(current);
    }
    stages
}

fn parse_modifier_stage(stage_trimmed: &str) -> eyre::Result<Option<Modifier>> {
    let (kw, rest) = match stage_trimmed.split_once(char::is_whitespace) {
        Some((k, r)) => (k.to_lowercase(), r.trim()),
        None => (stage_trimmed.to_lowercase(), ""),
    };

    match kw.as_str() {
        "since" => {
            if let Some(dur) = parse_duration(rest) {
                Ok(Some(Modifier::Since(dur)))
            } else {
                eyre::bail!("Invalid duration for 'since': '{}'", rest)
            }
        }
        "before" => {
            if let Some(dur) = parse_duration(rest) {
                Ok(Some(Modifier::BeforeTime(dur)))
            } else if let Ok(n) = rest.parse::<usize>() {
                Ok(Some(Modifier::BeforeContext(n)))
            } else {
                eyre::bail!("Invalid argument for 'before': '{}' (expected duration or line count)", rest)
            }
        }
        "after" => {
            if let Ok(n) = rest.parse::<usize>() {
                Ok(Some(Modifier::AfterContext(n)))
            } else {
                eyre::bail!("Invalid line count for 'after': '{}'", rest)
            }
        }
        "context" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() == 1 {
                let n: usize = parts[0].parse()?;
                Ok(Some(Modifier::Context { before: n, after: n }))
            } else if parts.len() == 2 {
                let b: usize = parts[0].parse()?;
                let a: usize = parts[1].parse()?;
                Ok(Some(Modifier::Context { before: b, after: a }))
            } else {
                eyre::bail!("'context' expects 1 or 2 integer arguments, got: '{}'", rest)
            }
        }
        "first" => {
            let n: usize = rest.parse()?;
            Ok(Some(Modifier::First(n)))
        }
        "last" => {
            let n: usize = rest.parse()?;
            Ok(Some(Modifier::Last(n)))
        }
        "dedup" | "deduplicate" => {
            if rest.is_empty() {
                Ok(Some(Modifier::Deduplicate(DedupMode::Consecutive)))
            } else if rest.eq_ignore_ascii_case("all") {
                Ok(Some(Modifier::Deduplicate(DedupMode::All)))
            } else if let Some(dur) = parse_duration(rest) {
                Ok(Some(Modifier::Deduplicate(DedupMode::Window(dur))))
            } else {
                eyre::bail!("Invalid argument for 'dedup': '{}' (expected empty, 'all', or duration)", rest)
            }
        }
        _ => {
            let expr_opt = parse_stage(stage_trimmed)?;
            Ok(expr_opt.map(Modifier::Filter))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    And,
    Or,
    Not,
    LParen,
    RParen,
    Matcher(Matcher),
}

fn parse_stage(input: &str) -> eyre::Result<Option<Expr>> {
    let tokens = tokenize_stage(input)?;
    if tokens.is_empty() {
        return Ok(None);
    }
    let mut cursor = 0;
    let expr = parse_or(&tokens, &mut cursor)?;
    Ok(Some(expr))
}

fn tokenize_stage(input: &str) -> eyre::Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        if chars[i] == '(' {
            tokens.push(Token::LParen);
            i += 1;
            continue;
        }

        if chars[i] == ')' {
            tokens.push(Token::RParen);
            i += 1;
            continue;
        }

        let rem = &chars[i..];

        // Case-sensitive regex: sr"...", sr'...', sr`...`
        if rem.len() >= 3 && rem[0] == 's' && rem[1] == 'r' && is_quote_char(rem[2]) {
            let delimiter = rem[2];
            i += 3;
            let val = read_quoted_string(&chars, &mut i, delimiter)?;
            let compiled = regex::RegexBuilder::new(&val)
                .case_insensitive(false)
                .build()
                .map_err(|e| eyre::eyre!("Invalid regex '{}': {}", val, e))?;
            tokens.push(Token::Matcher(Matcher::Regex {
                pattern: val,
                case_sensitive: true,
                compiled,
            }));
            continue;
        }

        // Case-insensitive regex: r"...", r'...', r`...`
        if rem.len() >= 2 && rem[0] == 'r' && is_quote_char(rem[1]) {
            let delimiter = rem[1];
            i += 2;
            let val = read_quoted_string(&chars, &mut i, delimiter)?;
            let compiled = regex::RegexBuilder::new(&val)
                .case_insensitive(true)
                .build()
                .map_err(|e| eyre::eyre!("Invalid regex '{}': {}", val, e))?;
            tokens.push(Token::Matcher(Matcher::Regex {
                pattern: val,
                case_sensitive: false,
                compiled,
            }));
            continue;
        }

        // Case-sensitive glob: s~"...", s~'...', s~`...`
        if rem.len() >= 3 && rem[0] == 's' && rem[1] == '~' && is_quote_char(rem[2]) {
            let delimiter = rem[2];
            i += 3;
            let val = read_quoted_string(&chars, &mut i, delimiter)?;
            tokens.push(Token::Matcher(Matcher::Glob {
                pattern: val,
                case_sensitive: true,
            }));
            continue;
        }

        // Case-insensitive glob: ~"...", ~'...', ~`...`
        if rem.len() >= 2 && rem[0] == '~' && is_quote_char(rem[1]) {
            let delimiter = rem[1];
            i += 2;
            let val = read_quoted_string(&chars, &mut i, delimiter)?;
            tokens.push(Token::Matcher(Matcher::Glob {
                pattern: val,
                case_sensitive: false,
            }));
            continue;
        }

        // Case-sensitive literal: s"...", s'...', s`...`
        if rem.len() >= 2 && rem[0] == 's' && is_quote_char(rem[1]) {
            let delimiter = rem[1];
            i += 2;
            let val = read_quoted_string(&chars, &mut i, delimiter)?;
            tokens.push(Token::Matcher(Matcher::Literal {
                text: val,
                case_sensitive: true,
            }));
            continue;
        }

        // Case-insensitive literal: "...", '...', `...`
        if is_quote_char(chars[i]) {
            let delimiter = chars[i];
            i += 1;
            let val = read_quoted_string(&chars, &mut i, delimiter)?;
            tokens.push(Token::Matcher(Matcher::Literal {
                text: val,
                case_sensitive: false,
            }));
            continue;
        }

        let mut term = String::new();
        while i < len && !chars[i].is_whitespace() && chars[i] != '(' && chars[i] != ')' && chars[i] != '|' {
            term.push(chars[i]);
            i += 1;
        }

        if term.eq_ignore_ascii_case("and") {
            tokens.push(Token::And);
        } else if term.eq_ignore_ascii_case("or") {
            tokens.push(Token::Or);
        } else if term.eq_ignore_ascii_case("not") {
            tokens.push(Token::Not);
        } else if !term.is_empty() {
            eyre::bail!("Unexpected unquoted term '{}': search terms must be quoted using \"\", '', or ``", term);
        }
    }

    Ok(tokens)
}

/// Reads until unescaped `delimiter`, supporting escaping the delimiter itself (e.g. \' inside ')
fn read_quoted_string(chars: &[char], i: &mut usize, delimiter: char) -> eyre::Result<String> {
    let mut s = String::new();
    while *i < chars.len() {
        if chars[*i] == '\\' && *i + 1 < chars.len() {
            let next = chars[*i + 1];
            if next == delimiter || next == '\\' {
                s.push(next);
                *i += 2;
                continue;
            }
            // Preserve escape sequences for regex/special chars (e.g. \d, \s, \w, \n)
            s.push('\\');
            s.push(next);
            *i += 2;
            continue;
        }
        if chars[*i] == delimiter {
            *i += 1;
            return Ok(s);
        }
        s.push(chars[*i]);
        *i += 1;
    }
    Ok(s)
}

fn parse_or(tokens: &[Token], cursor: &mut usize) -> eyre::Result<Expr> {
    let mut left = parse_and(tokens, cursor)?;

    while *cursor < tokens.len() {
        if tokens[*cursor] == Token::Or {
            *cursor += 1;
            let right = parse_and(tokens, cursor)?;
            left = match left {
                Expr::Or(mut list) => {
                    list.push(right);
                    Expr::Or(list)
                }
                other => Expr::Or(vec![other, right]),
            };
        } else {
            break;
        }
    }

    Ok(left)
}

fn parse_and(tokens: &[Token], cursor: &mut usize) -> eyre::Result<Expr> {
    let mut left = parse_primary(tokens, cursor)?;

    while *cursor < tokens.len() {
        if tokens[*cursor] == Token::And {
            *cursor += 1;
            let right = parse_primary(tokens, cursor)?;
            left = match left {
                Expr::And(mut list) => {
                    list.push(right);
                    Expr::And(list)
                }
                other => Expr::And(vec![other, right]),
            };
        } else if matches!(tokens[*cursor], Token::Not | Token::LParen | Token::Matcher(_)) {
            let right = parse_primary(tokens, cursor)?;
            left = match left {
                Expr::And(mut list) => {
                    list.push(right);
                    Expr::And(list)
                }
                other => Expr::And(vec![other, right]),
            };
        } else {
            break;
        }
    }

    Ok(left)
}

fn parse_primary(tokens: &[Token], cursor: &mut usize) -> eyre::Result<Expr> {
    if *cursor >= tokens.len() {
        eyre::bail!("Unexpected end of expression");
    }

    match &tokens[*cursor] {
        Token::Not => {
            *cursor += 1;
            let inner = parse_primary(tokens, cursor)?;
            Ok(Expr::Not(Box::new(inner)))
        }
        Token::LParen => {
            *cursor += 1;
            let expr = parse_or(tokens, cursor)?;
            if *cursor < tokens.len() && tokens[*cursor] == Token::RParen {
                *cursor += 1;
                Ok(expr)
            } else {
                eyre::bail!("Unclosed parenthesis");
            }
        }
        Token::Matcher(m) => {
            *cursor += 1;
            Ok(Expr::Match(m.clone()))
        }
        other => eyre::bail!("Unexpected token {:?}", other),
    }
}

pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let mut num_str = String::new();
    let mut unit_str = String::new();

    for c in s.chars() {
        if c.is_ascii_digit() {
            num_str.push(c);
        } else {
            unit_str.push(c);
        }
    }

    let val: u64 = num_str.parse().ok()?;
    let unit = unit_str.trim().to_lowercase();

    match unit.as_str() {
        "ms" => Some(Duration::from_millis(val)),
        "s" | "sec" | "secs" | "second" | "seconds" => Some(Duration::from_secs(val)),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(Duration::from_secs(val * 60)),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(Duration::from_secs(val * 3600)),
        "d" | "day" | "days" => Some(Duration::from_secs(val * 86400)),
        _ => None,
    }
}

pub fn parse_timestamp_secs(ts: &str) -> Option<u64> {
    let clean = ts.trim_end_matches('Z');
    let (date, time) = clean.split_once('T')?;
    let mut d_parts = date.split('-');
    let y: i64 = d_parts.next()?.parse().ok()?;
    let m: i64 = d_parts.next()?.parse().ok()?;
    let d: i64 = d_parts.next()?.parse().ok()?;

    let time_part = time.split('.').next()?;
    let mut t_parts = time_part.split(':');
    let hour: i64 = t_parts.next()?.parse().ok()?;
    let min: i64 = t_parts.next()?.parse().ok()?;
    let sec: i64 = t_parts.next()?.parse().ok()?;

    let m_adj = if m <= 2 { m + 9 } else { m - 3 };
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = y_adj / 400;
    let yoe = y_adj - era * 400;
    let doy = (153 * m_adj + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;

    let secs = days * 86400 + hour * 3600 + min * 60 + sec;
    if secs >= 0 {
        Some(secs as u64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_three_types_of_strings_interchangeable() {
        let q1 = Query::parse(r#""hello""#).unwrap();
        let q2 = Query::parse(r#"'hello'"#).unwrap();
        let q3 = Query::parse(r#"`hello`"#).unwrap();
        assert_eq!(q1, q2);
        assert_eq!(q2, q3);
    }

    #[test]
    fn test_self_escaping() {
        let q_single = Query::parse(r#"'Hello I\'ll'"#).unwrap();
        let q_double = Query::parse(r#""Hello I\"ll""#).unwrap();
        let q_backtick = Query::parse(r#"`Hello I\`ll`"#).unwrap();

        let log = LogItem::new("svc".into(), "Hello I'll".into(), None, None, false);
        assert!(q_single.matches(&log, 0));
        assert!(q_double.matches(&log, 0)); // Matches 'Hello I"ll' if content has it, here test escaped literal
        assert!(q_backtick.matches(&LogItem::new("svc".into(), "Hello I`ll".into(), None, None, false), 0));
    }

    #[test]
    fn test_prefixes_with_single_and_backtick() {
        assert!(Query::parse(r#"r'err\d+'"#).is_ok());
        assert!(Query::parse(r#"sr`DEBUG`"#).is_ok());
        assert!(Query::parse(r#"~'*.log'"#).is_ok());
        assert!(Query::parse(r#"s`EXACT`"#).is_ok());
    }

    #[test]
    fn test_pipeline_inside_quotes() {
        let q = Query::parse(r#"'api | v1' | not `error`"#).unwrap();
        assert_eq!(q.pipeline.len(), 1);
    }
}
