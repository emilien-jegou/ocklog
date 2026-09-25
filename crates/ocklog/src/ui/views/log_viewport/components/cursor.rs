use ratatui::layout::{Position, Rect};
use ratatui::Frame;

pub struct CursorPositioner;

impl CursorPositioner {
    pub fn place(frame: &mut Frame, area: Rect, visual_x: usize, visual_y: usize) {
        if visual_y < area.height as usize {
            let cx = area.x + (visual_x as u16).min(area.width.saturating_sub(1));
            let cy = area.y + visual_y as u16;
            if cy < area.bottom() && cx < area.right() {
                frame.set_cursor_position(Position::new(cx, cy));
            }
        }
    }
}
