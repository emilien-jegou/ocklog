use crate::libs::ockql::OckQuery;

#[derive(Debug, Clone)]
pub struct ActiveFilter {
    raw_query: String,
    compiled: OckQuery,
}

impl ActiveFilter {
    pub fn compile(raw: impl Into<String>) -> Result<Self, String> {
        let raw_query = raw.into();
        let compiled = OckQuery::parse(&raw_query)?;
        Ok(Self { raw_query, compiled })
    }

    pub fn raw(&self) -> &str {
        &self.raw_query
    }

    pub fn query(&self) -> &OckQuery {
        &self.compiled
    }
}
