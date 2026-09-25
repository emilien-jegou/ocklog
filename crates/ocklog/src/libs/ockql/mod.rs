pub mod ast;
pub mod evaluator;
pub mod lexer;
pub mod parser;

pub use ast::Expr;
pub use evaluator::{Evaluator, QueryEvaluationContext};
pub use parser::QueryParser;

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct OckQuery {
    raw: String,
    ast: Expr,
}

#[allow(unused)]
impl OckQuery {
    pub fn parse(raw: impl Into<String>) -> Result<Self, String> {
        let raw = raw.into();
        let ast = QueryParser::parse(&raw)?;
        Ok(Self { raw, ast })
    }

    pub fn matches(&self, ctx: &QueryEvaluationContext<'_>) -> bool {
        Evaluator::matches(&self.ast, ctx)
    }

    pub fn ast(&self) -> &Expr {
        &self.ast
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn has_stream_modifiers(&self) -> bool {
        self.ast.is_stream_modifier()
    }
}
