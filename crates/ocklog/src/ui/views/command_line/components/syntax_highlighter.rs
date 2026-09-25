use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

pub struct SyntaxHighlighter;

impl SyntaxHighlighter {
    pub fn highlight(query: &str) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let chars: Vec<char> = query.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i].is_whitespace() {
                spans.push(Span::raw(" "));
                i += 1;
                continue;
            }
            if chars[i] == '|' {
                spans.push(Span::styled("|", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
                i += 1;
                continue;
            }
            if chars[i] == '(' || chars[i] == ')' {
                spans.push(Span::styled(chars[i].to_string(), Style::default().fg(Color::Yellow)));
                i += 1;
                continue;
            }

            let rem = &chars[i..];
            if rem.len() >= 3 && rem[0] == 's' && rem[1] == 'r' && is_quote(rem[2]) {
                spans.push(Self::scan_quote(&chars, &mut i, 3, "sr", Color::Cyan));
                continue;
            }
            if rem.len() >= 2 && rem[0] == 'r' && is_quote(rem[1]) {
                spans.push(Self::scan_quote(&chars, &mut i, 2, "r", Color::LightCyan));
                continue;
            }
            if rem.len() >= 3 && rem[0] == 's' && rem[1] == '~' && is_quote(rem[2]) {
                spans.push(Self::scan_quote(&chars, &mut i, 3, "s~", Color::Magenta));
                continue;
            }
            if rem.len() >= 2 && rem[0] == '~' && is_quote(rem[1]) {
                spans.push(Self::scan_quote(&chars, &mut i, 2, "~", Color::LightMagenta));
                continue;
            }
            if is_quote(chars[i]) {
                spans.push(Self::scan_quote(&chars, &mut i, 1, "", Color::LightGreen));
                continue;
            }

            let mut word = String::new();
            while i < chars.len() && !chars[i].is_whitespace() && !matches!(chars[i], '|' | '(' | ')') {
                word.push(chars[i]);
                i += 1;
            }
            spans.push(Self::classify_word(&word));
        }
        spans
    }

    fn scan_quote(chars: &[char], i: &mut usize, skip: usize, prefix: &str, color: Color) -> Span<'static> {
        let delim = chars[*i + skip - 1];
        *i += skip;
        let mut s = String::from(prefix);
        s.push(delim);
        while *i < chars.len() && chars[*i] != delim {
            s.push(chars[*i]);
            *i += 1;
        }
        if *i < chars.len() {
            s.push(chars[*i]);
            *i += 1;
        }
        Span::styled(s, Style::default().fg(color).add_modifier(Modifier::BOLD))
    }

    fn classify_word(w: &str) -> Span<'static> {
        let lower = w.to_lowercase();
        match lower.as_str() {
            "and" | "or" => Span::styled(w.to_string(), Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
            "not" => Span::styled(w.to_string(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            "since" | "before" => Span::styled(w.to_string(), Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
            "context" | "after" => Span::styled(w.to_string(), Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)),
            "dedup" | "all" => Span::styled(w.to_string(), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            _ if w.chars().all(|c| c.is_ascii_digit()) => Span::styled(w.to_string(), Style::default().fg(Color::White)),
            _ => Span::styled(w.to_string(), Style::default().fg(Color::Red).add_modifier(Modifier::UNDERLINED | Modifier::BOLD)),
        }
    }
}

fn is_quote(c: char) -> bool {
    c == '"' || c == '\'' || c == '`'
}
