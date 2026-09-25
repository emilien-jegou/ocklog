use ratatui::style::Color as RatatuiColor;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    Rgb(u8, u8, u8),
    Ansi(u8),
    Ansi256(u8),
    Reset,
    Bg,
    Fg,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
}

impl Color {
    pub fn to_ratatui(&self) -> RatatuiColor {
        match self {
            Color::Rgb(r, g, b) => RatatuiColor::Rgb(*r, *g, *b),
            Color::Ansi(v) | Color::Ansi256(v) => RatatuiColor::Indexed(*v),
            Color::Reset => RatatuiColor::Reset,
            Color::Bg => RatatuiColor::Reset,
            Color::Fg => RatatuiColor::White,
            Color::Black => RatatuiColor::Black,
            Color::Red => RatatuiColor::Red,
            Color::Green => RatatuiColor::Green,
            Color::Yellow => RatatuiColor::Yellow,
            Color::Blue => RatatuiColor::Blue,
            Color::Magenta => RatatuiColor::Magenta,
            Color::Cyan => RatatuiColor::Cyan,
            Color::Gray => RatatuiColor::Gray,
            Color::DarkGray => RatatuiColor::DarkGray,
            Color::LightRed => RatatuiColor::LightRed,
            Color::LightGreen => RatatuiColor::LightGreen,
            Color::LightYellow => RatatuiColor::LightYellow,
            Color::LightBlue => RatatuiColor::LightBlue,
            Color::LightMagenta => RatatuiColor::LightMagenta,
            Color::LightCyan => RatatuiColor::LightCyan,
            Color::White => RatatuiColor::White,
        }
    }
}

#[derive(TypedBuilder, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiTheme {
    #[builder(default = Color::Reset)]
    pub bg: Color,
    #[builder(default = Color::White)]
    pub fg: Color,
    #[builder(default = Color::DarkGray)]
    pub dim: Color,
    #[builder(default = Color::Cyan)]
    pub border: Color,
    #[builder(default = Color::Green)]
    pub status_active: Color,
    #[builder(default = Color::Red)]
    pub status_error: Color,
    #[builder(default = Color::Yellow)]
    pub status_warn: Color,
    #[builder(default = Color::Blue)]
    pub highlight: Color,
    #[builder(default = Color::Rgb(40, 75, 125))]
    pub selection_bg: Color,
    #[builder(default = Color::Rgb(240, 240, 255))]
    pub selection_fg: Color,
}

impl Default for UiTheme {
    fn default() -> Self {
        UiTheme::builder().build()
    }
}
