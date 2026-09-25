use std::io::{IsTerminal, Read, Write};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalPalette {
    pub ansi: [Option<(u8, u8, u8)>; 256],
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
}

impl Default for TerminalPalette {
    fn default() -> Self {
        Self {
            ansi: [None; 256],
            fg: None,
            bg: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalColorMode {
    NoColor,
    Ansi,
    Ansi256,
    TrueColor(TerminalPalette),
}

impl TerminalColorMode {
    pub fn support_true_color(&self) -> bool {
        matches!(self, Self::TrueColor(_))
    }
}

fn parse_osc_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();

    if let Some(rgb_part) = s.strip_prefix("rgb:") {
        let mut parts = rgb_part.split('/');
        let r = parse_channel(parts.next()?)?;
        let g = parse_channel(parts.next()?)?;
        let b = parse_channel(parts.next()?)?;
        return Some((r, g, b));
    }

    if let Some(hex_part) = s.strip_prefix('#') {
        let len = hex_part.len();
        if len % 3 != 0 {
            return None;
        }
        let chunk_size = len / 3;
        let r = parse_channel(&hex_part[0..chunk_size])?;
        let g = parse_channel(&hex_part[chunk_size..chunk_size * 2])?;
        let b = parse_channel(&hex_part[chunk_size * 2..])?;
        return Some((r, g, b));
    }

    None
}

fn parse_channel(c: &str) -> Option<u8> {
    let len = c.len();
    if len == 0 {
        return None;
    }
    let val = u32::from_str_radix(c, 16).ok()?;
    match len {
        1 => Some((val * 17) as u8),
        2 => Some(val as u8),
        3 => Some((val >> 4) as u8),
        4 => Some((val >> 8) as u8),
        _ => Some((val >> ((len - 2) * 4)) as u8),
    }
}

#[cfg(unix)]
fn read_response_timeout(initial_timeout: Duration) -> Option<Vec<u8>> {
    use std::os::unix::io::AsRawFd;

    let stdin = std::io::stdin();
    let fd = stdin.as_raw_fd();
    let mut accumulated = Vec::new();

    let mut poll_fd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    let mut current_timeout_ms = initial_timeout.as_millis() as libc::c_int;

    loop {
        let ret = unsafe { libc::poll(&mut poll_fd, 1, current_timeout_ms) };

        if ret > 0 && (poll_fd.revents & libc::POLLIN) != 0 {
            let mut buf = [0; 1024];
            match std::io::stdin().read(&mut buf) {
                Ok(0) => break,
                Ok(n) => accumulated.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
            current_timeout_ms = 20;
        } else {
            break;
        }
    }

    if accumulated.is_empty() {
        None
    } else {
        Some(accumulated)
    }
}

#[cfg(not(unix))]
fn read_response_timeout(_timeout: Duration) -> Option<Vec<u8>> {
    None
}

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> eyre::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(RawModeGuard)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn determine_terminal_color_mode_from_env() -> TerminalColorMode {
    let term = std::env::var("TERM").unwrap_or_default().to_lowercase();
    let colorterm = std::env::var("COLORTERM").unwrap_or_default().to_lowercase();
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default().to_lowercase();

    if term == "dumb" {
        return TerminalColorMode::NoColor;
    }

    if colorterm == "truecolor" || colorterm == "24bit" || term.contains("-24bit") || term.contains("-truecolor") {
        return TerminalColorMode::TrueColor(TerminalPalette::default());
    }

    if term_program.contains("iterm")
        || term_program.contains("konsole")
        || term_program.contains("wezterm")
        || std::env::var("VTE_VERSION").is_ok()
        || term.starts_with("st")
        || term.contains("kitty")
        || term.contains("alacritty")
        || term.contains("foot")
        || term.contains("console-terminal-emulator1")
    {
        return TerminalColorMode::TrueColor(TerminalPalette::default());
    }

    if colorterm.contains("256") || term.contains("256") {
        return TerminalColorMode::Ansi256;
    }

    if term.contains("putty")
        || term.contains("rxvt")
        || term.contains("tmux")
        || term.contains("screen")
        || term.contains("teken")
        || term.contains("linux")
        || term.contains("xterm")
    {
        return TerminalColorMode::Ansi256;
    }

    if std::env::var("COLORTERM").is_ok()
        || term.contains("interix")
        || term.contains("pcvt")
        || term.contains("cons")
    {
        return TerminalColorMode::Ansi;
    }

    TerminalColorMode::Ansi
}

pub fn detect_color_mode() -> eyre::Result<TerminalColorMode> {
    if std::env::var("NO_COLOR").is_ok() {
        return Ok(TerminalColorMode::NoColor);
    }

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        if let Ok(force_color) = std::env::var("FORCE_COLOR") {
            if force_color == "0" || force_color.eq_ignore_ascii_case("false") {
                return Ok(TerminalColorMode::NoColor);
            }
        } else {
            return Ok(TerminalColorMode::NoColor);
        }
    }

    let args = std::env::args().collect::<Vec<String>>();

    if args.iter().any(|arg| arg.contains("color=never") || arg.contains("no-color") || arg.contains("color=none")) {
        return Ok(TerminalColorMode::NoColor);
    }

    let has_truecolor = args.iter().any(|arg| arg.contains("color=16m") || arg.contains("color=full") || arg.contains("color=truecolor"));
    let has_256 = args.iter().any(|arg| arg.contains("color=256"));
    let has_ansi = args.iter().any(|arg| arg.contains("color=basic") || arg.contains("color=ansi"));

    let base_mode = if has_truecolor {
        TerminalColorMode::TrueColor(TerminalPalette::default())
    } else if has_256 {
        TerminalColorMode::Ansi256
    } else if has_ansi {
        TerminalColorMode::Ansi
    } else if let Ok(force_color) = std::env::var("FORCE_COLOR") {
        match force_color.as_str() {
            "0" | "false" => TerminalColorMode::NoColor,
            "1" | "true" => TerminalColorMode::Ansi,
            "2" => TerminalColorMode::Ansi256,
            "3" => TerminalColorMode::TrueColor(TerminalPalette::default()),
            _ => determine_terminal_color_mode_from_env(),
        }
    } else {
        determine_terminal_color_mode_from_env()
    };

    if let TerminalColorMode::TrueColor(_) = base_mode {
        let _guard = match RawModeGuard::new() {
            Ok(g) => g,
            Err(_) => return Ok(TerminalColorMode::TrueColor(TerminalPalette::default())),
        };

        let mut query = String::new();
        for i in 0..16 {
            query.push_str(&format!("\x1b]4;{};?\x1b\\", i));
        }
        query.push_str("\x1b]10;?\x1b\\\x1b]11;?\x1b\\");

        let mut out = std::io::stdout();
        if out.write_all(query.as_bytes()).is_ok() && out.flush().is_ok() {
            let mut palette = TerminalPalette::default();
            if let Some(data) = read_response_timeout(Duration::from_millis(150)) {
                let response = String::from_utf8_lossy(&data).into_owned();

                for chunk in response.split("\x1b]") {
                    let chunk = chunk.replace("\x1b\\", "").replace("\x07", "");
                    if let Some(rest) = chunk.strip_prefix("4;") {
                        let mut parts = rest.split(';');
                        if let (Some(idx_str), Some(rgb_str)) = (parts.next(), parts.next()) {
                            if let Ok(idx) = idx_str.parse::<usize>() {
                                if idx < 256 {
                                    palette.ansi[idx] = parse_osc_rgb(rgb_str);
                                }
                            }
                        }
                    } else if let Some(rest) = chunk.strip_prefix("10;") {
                        palette.fg = parse_osc_rgb(rest);
                    } else if let Some(rest) = chunk.strip_prefix("11;") {
                        palette.bg = parse_osc_rgb(rest);
                    }
                }
                drop(_guard);
                return Ok(TerminalColorMode::TrueColor(palette));
            }
        }
        drop(_guard);
        return Ok(TerminalColorMode::TrueColor(TerminalPalette::default()));
    }

    Ok(base_mode)
}
