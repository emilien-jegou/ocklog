pub struct TextUtils;

impl TextUtils {
    pub fn char_width(c: char) -> usize {
        if c < ' ' {
            0
        } else if c <= '~' {
            1
        } else if is_wide_char(c) {
            2
        } else {
            1
        }
    }

    pub fn display_width_up_to(s: &str, char_limit: usize) -> usize {
        s.chars().take(char_limit).map(Self::char_width).sum()
    }

    pub fn char_index_at_width(s: &str, target_width: usize) -> usize {
        let mut cur = 0;
        for (i, c) in s.chars().enumerate() {
            let w = Self::char_width(c);
            if cur + w > target_width {
                return i;
            }
            cur += w;
        }
        s.chars().count()
    }

    pub fn truncate(s: &str, max_len: usize) -> String {
        if s.chars().count() > max_len && max_len > 0 {
            let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
            format!("{}…", truncated)
        } else {
            s.to_string()
        }
    }
}

fn is_wide_char(c: char) -> bool {
    ('\u{1100}'..='\u{115F}').contains(&c)
        || ('\u{2329}'..='\u{232A}').contains(&c)
        || ('\u{2E80}'..='\u{303E}').contains(&c)
        || ('\u{3040}'..='\u{A4CF}').contains(&c)
        || ('\u{AC00}'..='\u{D7A3}').contains(&c)
        || ('\u{F900}'..='\u{FAFF}').contains(&c)
        || ('\u{1F300}'..='\u{1FAFF}').contains(&c)
        || ('\u{2600}'..='\u{27BF}').contains(&c)
}
