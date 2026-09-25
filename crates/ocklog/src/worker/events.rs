#[derive(Clone, Debug)]
pub struct QueryFilterReq {
    pub query: String,
}

#[derive(Clone, Debug)]
pub struct QueryFilterRes {
    pub matched_indices: Vec<usize>,
}
