#[derive(Debug, Clone)]
pub struct LineVisualChunk {
    pub text: String,
    pub content_char_start: usize,
    pub has_continuation: bool,
}

pub struct LineComposer;

impl LineComposer {
    pub fn get_unfolded_chunks(content: &str, content_width: usize) -> Vec<LineVisualChunk> {
        let mut chunks = Vec::new();
        let mut current_char_offset = 0;
        let lines: Vec<&str> = content.split('\n').collect();
        let total_lines = lines.len();

        for (line_idx, line) in lines.iter().enumerate() {
            let is_last_logical = line_idx + 1 == total_lines;
            let clean = line.trim_end_matches('\r');
            let chars: Vec<char> = clean.chars().collect();

            if chars.is_empty() {
                chunks.push(LineVisualChunk { text: String::new(), content_char_start: current_char_offset, has_continuation: !is_last_logical });
            } else {
                Self::slice_line_chunks(&chars, content_width, current_char_offset, is_last_logical, &mut chunks);
            }
            current_char_offset += clean.chars().count() + 1;
        }

        if chunks.is_empty() {
            chunks.push(LineVisualChunk { text: String::new(), content_char_start: 0, has_continuation: false });
        }
        chunks
    }

    fn slice_line_chunks(
        chars: &[char],
        width: usize,
        base_offset: usize,
        is_last_logical: bool,
        out: &mut Vec<LineVisualChunk>,
    ) {
        let total_sub = chars.len().div_ceil(width);
        let mut chunk_offset = base_offset;
        for (idx, slice) in chars.chunks(width).enumerate() {
            let is_last_sub = idx + 1 == total_sub;
            let has_cont = !(is_last_logical && is_last_sub);
            out.push(LineVisualChunk {
                text: slice.iter().collect(),
                content_char_start: chunk_offset,
                has_continuation: has_cont,
            });
            chunk_offset += slice.len();
        }
    }

    pub fn visual_height(content: &str, width: usize) -> usize {
        if width == 0 { return 1; }
        content.split('\n').map(|l| {
            let len = l.trim_end_matches('\r').chars().count();
            if len == 0 { 1 } else { len.div_ceil(width) }
        }).sum::<usize>().max(1)
    }
}
