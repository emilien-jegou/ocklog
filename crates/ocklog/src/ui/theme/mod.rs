pub mod selection_style;

use ratatui::style::Color as RatColor;

pub struct ThemePalette;

impl ThemePalette {
    pub fn service_color(service: &str) -> RatColor {
        const PALETTE: &[RatColor] = &[
            RatColor::Cyan,
            RatColor::LightCyan,
            RatColor::LightGreen,
            RatColor::Yellow,
            RatColor::LightYellow,
            RatColor::Blue,
            RatColor::LightBlue,
            RatColor::Magenta,
            RatColor::LightMagenta,
            RatColor::Green,
        ];
        let mut hash: usize = 5381;
        for b in service.bytes() {
            hash = ((hash << 5).wrapping_add(hash)).wrapping_add(b as usize);
        }
        PALETTE[hash % PALETTE.len()]
    }
}
