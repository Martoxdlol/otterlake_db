use std::ops::Bound;

pub mod builder;

pub enum Order {
    Asc,
    Desc,
}
pub struct Query {
    pub collection_name: String,
    pub with_index: Option<WithIndex>,
    pub filter: Option<Filter>,
    pub order: Option<Order>,
    pub limit: Option<u64>,
}

pub struct WithIndex {
    pub index_name: String,
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
