#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchPattern {
    Literal { pattern: String, case_sensitive: bool },
    Glob { pattern: String, case_sensitive: bool },
    Regex { pattern: String, case_sensitive: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeBound {
    Since(u64),
    Before(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamModifier {
    Context(usize),
    After(usize),
    First(usize),
    Last(usize),
    Dedup { all: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Pattern(MatchPattern),
    Time(TimeBound),
    Modifier(StreamModifier),
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

impl Expr {
    pub fn is_stream_modifier(&self) -> bool {
        match self {
            Expr::Modifier(_) => true,
            Expr::Not(inner) => inner.is_stream_modifier(),
            Expr::And(items) | Expr::Or(items) => items.iter().any(|e| e.is_stream_modifier()),
            _ => false,
        }
    }
}
