#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualMode {
    None,
    Character,
    Line,
    Block,
}

pub struct SelectionCalculator;

impl SelectionCalculator {
    pub fn get_effective_range(
        kb_mode: VisualMode,
        kb_anchor: Option<(usize, usize)>,
        cursor: (usize, usize),
        mouse_mode: VisualMode,
        mouse_anchor: Option<(usize, usize)>,
        mouse_head: Option<(usize, usize)>,
        row: usize,
        total_cols: usize,
    ) -> Option<(usize, usize)> {
        if mouse_mode != VisualMode::None && mouse_anchor.is_some() && mouse_head.is_some() {
            let m_head = mouse_head.unwrap();
            Self::get_line_range(mouse_mode, mouse_anchor, m_head, row, total_cols)
        } else if kb_mode != VisualMode::None {
            Self::get_line_range(kb_mode, kb_anchor, cursor, row, total_cols)
        } else {
            None
        }
    }

    pub fn get_line_range(
        mode: VisualMode,
        anchor: Option<(usize, usize)>,
        head: (usize, usize),
        row: usize,
        total_cols: usize,
    ) -> Option<(usize, usize)> {
        let (anchor_r, anchor_c) = anchor?;
        if mode == VisualMode::None || total_cols == 0 {
            return None;
        }
        match mode {
            VisualMode::Line => Self::eval_line(anchor_r, head.0, row, total_cols),
            VisualMode::Block => Self::eval_block((anchor_r, anchor_c), head, row, total_cols),
            VisualMode::Character => Self::eval_char((anchor_r, anchor_c), head, row, total_cols),
            VisualMode::None => None,
        }
    }

    fn eval_line(ar: usize, cr: usize, r: usize, total: usize) -> Option<(usize, usize)> {
        if r >= ar.min(cr) && r <= ar.max(cr) {
            Some((0, total.saturating_sub(1)))
        } else {
            None
        }
    }

    fn eval_block(a: (usize, usize), c: (usize, usize), r: usize, total: usize) -> Option<(usize, usize)> {
        if r >= a.0.min(c.0) && r <= a.0.max(c.0) {
            let min_c = a.1.min(c.1).min(total.saturating_sub(1));
            let max_c = a.1.max(c.1).min(total.saturating_sub(1));
            Some((min_c, max_c))
        } else {
            None
        }
    }

    fn eval_char(a: (usize, usize), c: (usize, usize), r: usize, total: usize) -> Option<(usize, usize)> {
        let (start, end) = if a <= c { (a, c) } else { (c, a) };
        if r < start.0 || r > end.0 {
            None
        } else if start.0 == end.0 {
            let s_col = start.1.min(total.saturating_sub(1));
            let e_col = end.1.min(total.saturating_sub(1));
            Some((s_col, e_col))
        } else if r == start.0 {
            let s_col = start.1.min(total.saturating_sub(1));
            Some((s_col, total.saturating_sub(1)))
        } else if r == end.0 {
            let e_col = end.1.min(total.saturating_sub(1));
            Some((0, e_col))
        } else {
            Some((0, total.saturating_sub(1)))
        }
    }
}
