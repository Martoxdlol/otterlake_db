use std::ops::Bound;

use storage::types::{CollectionId, IndexId};

pub struct Document {
    pub _id: Vec<u8>,
    pub data: Vec<u8>,
}

pub struct Query {
    pub collection_id: CollectionId,
    pub with_index: Option<WithIndex>,
    pub filter: Option<Filter>,
    pub limit: Option<u64>,
}

pub struct WithIndex {
    pub index_id: IndexId,
    pub lower: Bound<Vec<u8>>,
    pub upper: Bound<Vec<u8>>,
}

pub enum Filter {
    And(Vec<Filter>),
    Or(Vec<Filter>),
    Not(Box<Filter>),
    Comparison {
        field: String,
        operator: ComparisonOperator,
        value: Vec<u8>,
    },
}

pub enum ComparisonOperator {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
}
