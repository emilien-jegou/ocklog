use crate::libs::ockql::ast::{Expr, MatchPattern, StreamModifier, TimeBound};
use crate::libs::ockql::lexer::{Lexer, Token};

pub struct QueryParser {
    tokens: Vec<Token>,
    pos: usize,
}

impl QueryParser {
    pub fn parse(input: &str) -> Result<Expr, String> {
        let tokens = Lexer::tokenize(input);
        if tokens.is_empty() {
            return Err("Empty query".to_string());
        }
        let mut parser = Self { tokens, pos: 0 };
        let expr = parser.parse_or_expr()?;
        if parser.pos < parser.tokens.len() {
            return Err(format!("Unexpected token at pos {}", parser.pos));
        }
        Ok(expr)
    }

    fn parse_or_expr(&mut self) -> Result<Expr, String> {
        let mut exprs = vec![self.parse_and_expr()?];
        while self.match_token(|t| matches!(t, Token::Pipe) || matches!(t, Token::Word(w) if w.eq_ignore_ascii_case("or"))) {
            exprs.push(self.parse_and_expr()?);
        }
        Ok(if exprs.len() == 1 { exprs.remove(0) } else { Expr::Or(exprs) })
    }

    fn parse_and_expr(&mut self) -> Result<Expr, String> {
        let mut exprs = vec![self.parse_unary_expr()?];
        while self.has_and_ahead() {
            let _ = self.match_token(|t| matches!(t, Token::Word(w) if w.eq_ignore_ascii_case("and")));
            exprs.push(self.parse_unary_expr()?);
        }
        Ok(if exprs.len() == 1 { exprs.remove(0) } else { Expr::And(exprs) })
    }

    fn has_and_ahead(&self) -> bool {
        if self.pos >= self.tokens.len() {
            return false;
        }
        !matches!(self.tokens[self.pos], Token::Pipe | Token::CloseParen)
    }

    fn parse_unary_expr(&mut self) -> Result<Expr, String> {
        if self.match_token(|t| matches!(t, Token::Word(w) if w.eq_ignore_ascii_case("not"))) {
            return Ok(Expr::Not(Box::new(self.parse_unary_expr()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        let tok = self.tokens.get(self.pos).cloned().ok_or("Unexpected EOF")?;
        self.pos += 1;

        match tok {
            Token::OpenParen => {
                let inner = self.parse_or_expr()?;
                self.expect_close_paren()?;
                Ok(inner)
            }
            Token::Quoted { text, prefix } => Ok(Expr::Pattern(compile_pattern(&text, &prefix))),
            Token::Word(w) => self.parse_word_directive(&w),
            _ => Err("Invalid syntax token".to_string()),
        }
    }

    fn parse_word_directive(&mut self, word: &str) -> Result<Expr, String> {
        match word.to_lowercase().as_str() {
            "since" => Ok(Expr::Time(TimeBound::Since(self.parse_duration_val()?))),
            "before" => Ok(Expr::Time(TimeBound::Before(self.parse_duration_val()?))),
            "context" => Ok(Expr::Modifier(StreamModifier::Context(self.parse_count_val()?))),
            "after" => Ok(Expr::Modifier(StreamModifier::After(self.parse_count_val()?))),
            "first" => Ok(Expr::Modifier(StreamModifier::First(self.parse_count_val()?))),
            "last" => Ok(Expr::Modifier(StreamModifier::Last(self.parse_count_val()?))),
            "dedup" => Ok(Expr::Modifier(StreamModifier::Dedup { all: self.check_all_modifier() })),
            other => Err(format!("Unquoted string '{}' is not a valid filter keyword. Enclose in quotes: \"{}\"", other, other)),
        }
    }

    fn parse_count_val(&mut self) -> Result<usize, String> {
        let next = self.tokens.get(self.pos).ok_or("Expected integer argument")?;
        if let Token::Word(s) = next {
            self.pos += 1;
            s.parse::<usize>().map_err(|_| "Invalid integer".to_string())
        } else {
            Err("Expected number".to_string())
        }
    }

    fn parse_duration_val(&mut self) -> Result<u64, String> {
        let next = self.tokens.get(self.pos).ok_or("Expected duration argument")?;
        if let Token::Word(s) = next {
            self.pos += 1;
            parse_duration_str(s).ok_or_else(|| "Invalid duration format".to_string())
        } else {
            Err("Expected duration string".to_string())
        }
    }

    fn check_all_modifier(&mut self) -> bool {
        if let Some(Token::Word(w)) = self.tokens.get(self.pos) {
            if w.eq_ignore_ascii_case("all") {
                self.pos += 1;
                return true;
            }
        }
        false
    }

    fn match_token<F: Fn(&Token) -> bool>(&mut self, predicate: F) -> bool {
        if self.pos < self.tokens.len() && predicate(&self.tokens[self.pos]) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_close_paren(&mut self) -> Result<(), String> {
        if self.match_token(|t| matches!(t, Token::CloseParen)) {
            Ok(())
        } else {
            Err("Expected ')'".to_string())
        }
    }
}

fn compile_pattern(text: &str, prefix: &str) -> MatchPattern {
    let cs = prefix.contains('s');
    if prefix.contains('r') {
        MatchPattern::Regex { pattern: text.to_string(), case_sensitive: cs }
    } else if prefix.contains('~') {
        MatchPattern::Glob { pattern: text.to_string(), case_sensitive: cs }
    } else {
        MatchPattern::Literal { pattern: text.to_string(), case_sensitive: cs }
    }
}

fn parse_duration_str(s: &str) -> Option<u64> {
    let split_pos = s.find(|c: char| !c.is_ascii_digit())?;
    let val: u64 = s[..split_pos].parse().ok()?;
    let mult = match &s[split_pos..] {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return None,
    };
    Some(val * mult)
}
