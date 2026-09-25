use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsiSegment {
    pub text: String,
    pub style: Style,
}

pub fn parse_ansi(input: &str) -> (String, Vec<AnsiSegment>) {
    let mut clean_text = String::with_capacity(input.len());
    let mut segments = Vec::new();
    let mut current_style = Style::default();
    let mut current_buf = String::new();

    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Flush text accumulated before this escape sequence
            if !current_buf.is_empty() {
                clean_text.push_str(&current_buf);
                segments.push(AnsiSegment {
                    text: std::mem::take(&mut current_buf),
                    style: current_style,
                });
            }

            let start = i + 2;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b';' || bytes[end] == b'?') {
                end += 1;
            }

            if end < bytes.len() {
                let cmd = bytes[end];
                if cmd == b'm' {
                    let params = std::str::from_utf8(&bytes[start..end]).unwrap_or("");
                    apply_sgr_params(&mut current_style, params);
                }
                i = end + 1;
                continue;
            }
        }

        let ch = input[i..].chars().next().unwrap_or(' ');
        current_buf.push(ch);
        i += ch.len_utf8();
    }

    if !current_buf.is_empty() {
        clean_text.push_str(&current_buf);
        segments.push(AnsiSegment {
            text: current_buf,
            style: current_style,
        });
    }

    (clean_text, segments)
}

fn apply_sgr_params(style: &mut Style, params: &str) {
    if params.is_empty() || params == "0" {
        *style = Style::default();
        return;
    }

    let parts: Vec<u32> = params
        .split(';')
        .filter_map(|s| s.parse::<u32>().ok())
        .collect();

    let mut idx = 0;
    while idx < parts.len() {
        match parts[idx] {
            0 => *style = Style::default(),
            1 => *style = style.add_modifier(Modifier::BOLD),
            2 => *style = style.add_modifier(Modifier::DIM),
            3 => *style = style.add_modifier(Modifier::ITALIC),
            4 => *style = style.add_modifier(Modifier::UNDERLINED),
            7 => *style = style.add_modifier(Modifier::REVERSED),
            22 => *style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => *style = style.remove_modifier(Modifier::ITALIC),
            24 => *style = style.remove_modifier(Modifier::UNDERLINED),
            27 => *style = style.remove_modifier(Modifier::REVERSED),

            // Standard Foreground
            30 => *style = style.fg(Color::Black),
            31 => *style = style.fg(Color::Red),
            32 => *style = style.fg(Color::Green),
            33 => *style = style.fg(Color::Yellow),
            34 => *style = style.fg(Color::Blue),
            35 => *style = style.fg(Color::Magenta),
            36 => *style = style.fg(Color::Cyan),
            37 => *style = style.fg(Color::Gray),
            39 => *style = style.fg(Color::Reset),

            // Bright Foreground
            90 => *style = style.fg(Color::DarkGray),
            91 => *style = style.fg(Color::LightRed),
            92 => *style = style.fg(Color::LightGreen),
            93 => *style = style.fg(Color::LightYellow),
            94 => *style = style.fg(Color::LightBlue),
            95 => *style = style.fg(Color::LightMagenta),
            96 => *style = style.fg(Color::LightCyan),
            97 => *style = style.fg(Color::White),

            // Standard Background
            40 => *style = style.bg(Color::Black),
            41 => *style = style.bg(Color::Red),
            42 => *style = style.bg(Color::Green),
            43 => *style = style.bg(Color::Yellow),
            44 => *style = style.bg(Color::Blue),
            45 => *style = style.bg(Color::Magenta),
            46 => *style = style.bg(Color::Cyan),
            47 => *style = style.bg(Color::Gray),
            49 => *style = style.bg(Color::Reset),

            // Extended Color (38 / 48)
            38 => {
                if idx + 2 < parts.len() && parts[idx + 1] == 5 {
                    let col = parts[idx + 2] as u8;
                    *style = style.fg(Color::Indexed(col));
                    idx += 2;
                } else if idx + 4 < parts.len() && parts[idx + 1] == 2 {
                    let r = parts[idx + 2] as u8;
                    let g = parts[idx + 3] as u8;
                    let b = parts[idx + 4] as u8;
                    *style = style.fg(Color::Rgb(r, g, b));
                    idx += 4;
                }
            }
            48 => {
                if idx + 2 < parts.len() && parts[idx + 1] == 5 {
                    let col = parts[idx + 2] as u8;
                    *style = style.bg(Color::Indexed(col));
                    idx += 2;
                } else if idx + 4 < parts.len() && parts[idx + 1] == 2 {
                    let r = parts[idx + 2] as u8;
                    let g = parts[idx + 3] as u8;
                    let b = parts[idx + 4] as u8;
                    *style = style.bg(Color::Rgb(r, g, b));
                    idx += 4;
                }
            }
            _ => {}
        }
        idx += 1;
    }
}
