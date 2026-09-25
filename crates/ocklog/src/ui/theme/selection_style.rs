use crate::terminal_colors::TerminalColorMode;
use ratatui::style::{Color as RatColor, Modifier, Style};

pub struct SelectionTheme;

impl SelectionTheme {
    pub fn active_line(color_mode: &TerminalColorMode) -> Style {
        if let TerminalColorMode::TrueColor(palette) = color_mode {
            if let Some((r, g, b)) = palette.bg {
                let lum = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
                let bg_val = if lum < 128.0 {
                    r.saturating_add(20)
                } else {
                    r.saturating_sub(20)
                };
                return Style::default().bg(RatColor::Rgb(bg_val, bg_val, bg_val));
            }
        }
        Style::default().bg(RatColor::Indexed(236))
    }

    pub fn selection(color_mode: &TerminalColorMode) -> Style {
        if let TerminalColorMode::TrueColor(palette) = color_mode {
            if let Some((r, g, b)) = palette.bg {
                let lum = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
                let is_dark = lum < 128.0;
                let (bg_v, fg_v) = if is_dark {
                    (r.saturating_add(35), 255)
                } else {
                    (r.saturating_sub(35), 0)
                };
                return Style::default()
                    .bg(RatColor::Rgb(bg_v, bg_v, bg_v))
                    .fg(RatColor::Rgb(fg_v, fg_v, fg_v))
                    .add_modifier(Modifier::BOLD);
            }
        }
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    }
}
