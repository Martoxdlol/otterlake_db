use serde::de::DeserializeOwned;

/**
* Based on Convex.dev queries:
*

ctx.db.query("messages")                              // QueryInitializer
 .withIndex("by_channel", q => q.eq("channel", c)    // 0..1 — index range
                                .gt("_creationTime", t))
 .filter(q => q.eq("author", a))                     // 0..N — ANDed together
 .order("desc")                                       // 0..1
 .take(10) | .first() | .unique() | .collect() | .paginate()   // terminal (async)

*/
use crate::{
    Document, Result, Transaction,
    document::Value,
    query::{ComparisonOperator, Order},
};

// ---------------------------------------------------------------------------
// Stage 0: the initializer returned by `Transaction::query`.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct QueryBuilder<'a> {
    tx: &'a Transaction,
    collection_name: String,
}

impl<'a> QueryBuilder<'a> {
    pub fn new(tx: &'a Transaction, collection_name: String) -> Self {
        Self {
            tx,
            collection_name,
        }
    }

    /// Pin the scan to an index, restricting the range with a sequence of
    /// equalities optionally followed by a single range bound. May be used at
    /// most once, and only as the first stage.
    pub fn with_index<S, F, R>(self, index_name: S, filter_fn: F) -> QueryBuilderWithIndex<'a>
    where
        S: Into<String>,
        F: FnOnce(WithIndexFilterBuilder) -> R,
        R: Into<WithIndexFilter>,
    {
        let with_index: WithIndexFilter =
            filter_fn(WithIndexFilterBuilder::new(index_name.into())).into();
        QueryBuilderWithIndex {
            tx: self.tx,
            collection_name: self.collection_name,
            with_index,
        }
    }

    /// Add a post-scan filter predicate. Transitions into the filtered stage,
    /// where further `filter` calls are ANDed together.
    pub fn filter<F>(self, filter_fn: F) -> FilteredQueryBuilder<'a>
    where
        F: FnOnce(FilterBuilder) -> FilterExpr,
    {
        FilteredQueryBuilder {
            tx: self.tx,
            collection_name: self.collection_name,
            with_index: None,
            filters: vec![filter_fn(FilterBuilder)],
        }
    }

    /// Set the result ordering and move to the terminal-only stage.
    pub fn order(self, order: Order) -> OrderedQueryBuilder<'a> {
        OrderedQueryBuilder {
            tx: self.tx,
            collection_name: self.collection_name,
            with_index: None,
            filters: Vec::new(),
            order,
        }
    }
}

// ---------------------------------------------------------------------------
// Stage 1: after `with_index`.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct QueryBuilderWithIndex<'a> {
    tx: &'a Transaction,
    collection_name: String,
    with_index: WithIndexFilter,
}

impl<'a> QueryBuilderWithIndex<'a> {
    pub fn filter<F>(self, filter_fn: F) -> FilteredQueryBuilder<'a>
    where
        F: FnOnce(FilterBuilder) -> FilterExpr,
    {
        FilteredQueryBuilder {
            tx: self.tx,
            collection_name: self.collection_name,
            with_index: Some(self.with_index),
            filters: vec![filter_fn(FilterBuilder)],
        }
    }

    pub fn order(self, order: Order) -> OrderedQueryBuilder<'a> {
        OrderedQueryBuilder {
            tx: self.tx,
            collection_name: self.collection_name,
            with_index: Some(self.with_index),
            filters: Vec::new(),
            order,
        }
    }
}

// ---------------------------------------------------------------------------
// The index-range filter builder (the `q` in `with_index`).
//
// A run of equalities (`eq`) narrows successive index fields and keeps the
// builder open; a single range bound (`lt`/`gt`/`lte`/`gte`) closes it.
// ---------------------------------------------------------------------------

pub enum WithIndexFilterUnit {
    LT(String, Value),
    GT(String, Value),
    EQ(String, Value),
    LTE(String, Value),
    GTE(String, Value),
}

pub struct WithIndexFilterBuilder {
    index_name: String,
    units: Vec<WithIndexFilterUnit>,
}

impl WithIndexFilterBuilder {
    pub fn new(index_name: String) -> Self {
        Self {
            index_name,
            units: Vec::new(),
        }
    }

    pub fn eq<K: Into<String>, V: Into<Value>>(mut self, key: K, value: V) -> Self {
        self.units
            .push(WithIndexFilterUnit::EQ(key.into(), value.into()));
        self
    }

    pub fn lt<K: Into<String>, V: Into<Value>>(mut self, key: K, value: V) -> WithIndexFilter {
        self.units
            .push(WithIndexFilterUnit::LT(key.into(), value.into()));
        self.into()
    }

    pub fn gt<K: Into<String>, V: Into<Value>>(mut self, key: K, value: V) -> WithIndexFilter {
        self.units
            .push(WithIndexFilterUnit::GT(key.into(), value.into()));
        self.into()
    }

    pub fn lte<K: Into<String>, V: Into<Value>>(mut self, key: K, value: V) -> WithIndexFilter {
        self.units
            .push(WithIndexFilterUnit::LTE(key.into(), value.into()));
        self.into()
    }

    pub fn gte<K: Into<String>, V: Into<Value>>(mut self, key: K, value: V) -> WithIndexFilter {
        self.units
            .push(WithIndexFilterUnit::GTE(key.into(), value.into()));
        self.into()
    }
}

/// An all-equality index filter never closes the builder, so it converts
/// straight into the finished filter.
impl From<WithIndexFilterBuilder> for WithIndexFilter {
    fn from(builder: WithIndexFilterBuilder) -> Self {
        WithIndexFilter {
            index_name: builder.index_name,
            units: builder.units,
        }
    }
}

pub struct WithIndexFilter {
    pub index_name: String,
    pub units: Vec<WithIndexFilterUnit>,
}

// ---------------------------------------------------------------------------
// The post-scan filter expression builder (the `q` in `filter`).
// ---------------------------------------------------------------------------

/// A post-scan filter predicate, mirroring [`crate::query::Filter`] but holding
/// decoded [`Value`]s; the encoding to the storage form happens when the query
/// is built for execution.
pub enum FilterExpr {
    And(Vec<FilterExpr>),
    Or(Vec<FilterExpr>),
    Not(Box<FilterExpr>),
    Comparison {
        field: String,
        operator: ComparisonOperator,
        value: Value,
    },
}

/// The closure argument handed to `filter`; constructs a [`FilterExpr`] tree.
pub struct FilterBuilder;

impl FilterBuilder {
    fn comparison(
        field: impl Into<String>,
        operator: ComparisonOperator,
        value: impl Into<Value>,
    ) -> FilterExpr {
        FilterExpr::Comparison {
            field: field.into(),
            operator,
            value: value.into(),
        }
    }

    pub fn eq(&self, field: impl Into<String>, value: impl Into<Value>) -> FilterExpr {
        Self::comparison(field, ComparisonOperator::Eq, value)
    }

    pub fn neq(&self, field: impl Into<String>, value: impl Into<Value>) -> FilterExpr {
        Self::comparison(field, ComparisonOperator::Neq, value)
    }

    pub fn gt(&self, field: impl Into<String>, value: impl Into<Value>) -> FilterExpr {
        Self::comparison(field, ComparisonOperator::Gt, value)
    }

    pub fn gte(&self, field: impl Into<String>, value: impl Into<Value>) -> FilterExpr {
        Self::comparison(field, ComparisonOperator::Gte, value)
    }

    pub fn lt(&self, field: impl Into<String>, value: impl Into<Value>) -> FilterExpr {
        Self::comparison(field, ComparisonOperator::Lt, value)
    }

    pub fn lte(&self, field: impl Into<String>, value: impl Into<Value>) -> FilterExpr {
        Self::comparison(field, ComparisonOperator::Lte, value)
    }

    pub fn and(&self, exprs: impl IntoIterator<Item = FilterExpr>) -> FilterExpr {
        FilterExpr::And(exprs.into_iter().collect())
    }

    pub fn or(&self, exprs: impl IntoIterator<Item = FilterExpr>) -> FilterExpr {
        FilterExpr::Or(exprs.into_iter().collect())
    }

    pub fn not(&self, expr: FilterExpr) -> FilterExpr {
        FilterExpr::Not(Box::new(expr))
    }
}

// ---------------------------------------------------------------------------
// Stage 2: after one or more `filter`s. Further `filter`s AND together.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct FilteredQueryBuilder<'a> {
    tx: &'a Transaction,
    collection_name: String,
    with_index: Option<WithIndexFilter>,
    filters: Vec<FilterExpr>,
}

impl<'a> FilteredQueryBuilder<'a> {
    pub fn filter<F>(mut self, filter_fn: F) -> Self
    where
        F: FnOnce(FilterBuilder) -> FilterExpr,
    {
        self.filters.push(filter_fn(FilterBuilder));
        self
    }

    pub fn order(self, order: Order) -> OrderedQueryBuilder<'a> {
        OrderedQueryBuilder {
            tx: self.tx,
            collection_name: self.collection_name,
            with_index: self.with_index,
            filters: self.filters,
            order,
        }
    }
}

// ---------------------------------------------------------------------------
// Stage 3: after `order`. Terminal-only.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct OrderedQueryBuilder<'a> {
    tx: &'a Transaction,
    collection_name: String,
    with_index: Option<WithIndexFilter>,
    filters: Vec<FilterExpr>,
    order: Order,
}

// ---------------------------------------------------------------------------
// Terminals. These are the only stages that touch the database; left
// unimplemented for now. Every chainable stage can terminate.
// ---------------------------------------------------------------------------

macro_rules! impl_terminals {
    ($stage:ident) => {
        impl<'a> $stage<'a> {
            pub async fn take(self, _limit: u64) -> Result<Vec<Document>> {
                unimplemented!()
            }

            pub async fn take_as<D: DeserializeOwned>(self, _limit: u64) -> Result<Vec<D>> {
                unimplemented!()
            }

            pub async fn first(self) -> Result<Option<Document>> {
                unimplemented!()
            }

            pub async fn first_as<D: DeserializeOwned>(self) -> Result<Option<D>> {
                unimplemented!()
            }

            pub async fn unique(self) -> Result<Document> {
                unimplemented!()
            }

            pub async fn unique_as<D: DeserializeOwned>(self) -> Result<D> {
                unimplemented!()
            }

            pub async fn collect(self) -> Result<Vec<Document>> {
                unimplemented!()
            }

            pub async fn collect_as<D: DeserializeOwned>(self) -> Result<Vec<D>> {
                unimplemented!()
            }
        }
    };
}

impl_terminals!(QueryBuilder);
impl_terminals!(QueryBuilderWithIndex);
impl_terminals!(FilteredQueryBuilder);
impl_terminals!(OrderedQueryBuilder);

#[cfg(test)]
mod test {
    use crate::{Transaction, query::Order};

    #[test]
    fn builds_full_chain() {
        let tx = Transaction::mock();

        // Initializer -> with_index -> filter (x2, ANDed) -> order.
        let _q = tx
            .query("messages")
            .with_index("by_channel", |q| {
                q.eq("channel", "general").gt("_creationTime", 1_000)
            })
            .filter(|q| q.eq("author", "ada"))
            .filter(|q| q.or([q.eq("pinned", true), q.gte("score", 10)]))
            .order(Order::Desc);
    }

    #[test]
    fn builds_without_index_or_order() {
        let tx = Transaction::mock();

        let _q = tx.query("users").filter(|q| q.eq("email", "x@gmail.com"));
    }

    /// `filter` is 0..N, so every stage must be able to terminate without one.
    /// Building the terminal futures (without awaiting) is enough to prove the
    /// no-filter paths type-check; the terminal bodies are never run.
    #[test]
    fn filter_is_optional() {
        let tx = Transaction::mock();

        // Initializer straight to a terminal.
        let _a = tx.query("users").take(10);
        // Order, no filter.
        let _b = tx.query("users").order(Order::Desc).collect();
        // Index, no filter.
        let _c = tx
            .query("users")
            .with_index("by_email", |q| q.eq("email", "x"))
            .first();
        // Index + order, no filter.
        let _d = tx
            .query("users")
            .with_index("by_email", |q| q.eq("email", "x"))
            .order(Order::Asc)
            .take(5);
    }
}
