#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(String),
    Quoted { text: String, prefix: String },
    Pipe,
    OpenParen,
    CloseParen,
}

pub struct Lexer;

impl Lexer {
    pub fn tokenize(input: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i].is_whitespace() {
                i += 1;
                continue;
            }
            if let Some((tok, next_i)) = Self::scan_syntax_or_quote(&chars, i) {
                tokens.push(tok);
                i = next_i;
            } else {
                let (word, next_i) = Self::scan_word(&chars, i);
                tokens.push(Token::Word(word));
                i = next_i;
            }
        }
        tokens
    }

    fn scan_syntax_or_quote(chars: &[char], i: usize) -> Option<(Token, usize)> {
        match chars[i] {
            '|' => Some((Token::Pipe, i + 1)),
            '(' => Some((Token::OpenParen, i + 1)),
            ')' => Some((Token::CloseParen, i + 1)),
            '"' | '\'' | '`' => Some(Self::scan_quoted(chars, i, String::new())),
            's' | 'r' | '~' => Self::scan_prefixed_quote(chars, i),
            _ => None,
        }
    }

    fn scan_prefixed_quote(chars: &[char], start: usize) -> Option<(Token, usize)> {
        let mut idx = start;
        let mut prefix = String::new();
        while idx < chars.len() && (chars[idx] == 's' || chars[idx] == 'r' || chars[idx] == '~') {
            prefix.push(chars[idx]);
            idx += 1;
        }
        if idx < chars.len() && is_quote_delim(chars[idx]) {
            Some(Self::scan_quoted(chars, idx, prefix))
        } else {
            None
        }
    }

    fn scan_quoted(chars: &[char], start: usize, prefix: String) -> (Token, usize) {
        let delim = chars[start];
        let mut text = String::new();
        let mut i = start + 1;

        while i < chars.len() && chars[i] != delim {
            if chars[i] == '\\' && i + 1 < chars.len() {
                text.push(chars[i + 1]);
                i += 2;
            } else {
                text.push(chars[i]);
                i += 1;
            }
        }
        (Token::Quoted { text, prefix }, (i + 1).min(chars.len()))
    }

    fn scan_word(chars: &[char], start: usize) -> (String, usize) {
        let mut word = String::new();
        let mut i = start;
        while i < chars.len() && !chars[i].is_whitespace() && !matches!(chars[i], '|' | '(' | ')' | '"' | '\'' | '`') {
            word.push(chars[i]);
            i += 1;
        }
        (word, i)
    }
}

fn is_quote_delim(c: char) -> bool {
    matches!(c, '"' | '\'' | '`')
}
