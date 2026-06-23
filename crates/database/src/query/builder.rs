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
use std::ops::Bound;

use storage::types::DocumentId;

use crate::{
    Document, Error, Result, Transaction,
    document::{Value, from_document},
    encoding::encode_entry,
    query::{ComparisonOperator, Filter, Order, Query, WithIndex},
};

// ---------------------------------------------------------------------------
// Stage 0: the initializer returned by `Transaction::query`.
// ---------------------------------------------------------------------------

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

    pub async fn get(self, document_id: DocumentId) -> Result<Option<Document>> {
        self.tx
            .get_document(self.collection_name, document_id)
            .await
    }

    pub async fn get_as<D: DeserializeOwned>(self, document_id: DocumentId) -> Result<Option<D>> {
        match self
            .tx
            .get_document(self.collection_name, document_id)
            .await?
        {
            Some(doc) => Ok(Some(from_document(doc)?)),
            None => Ok(None),
        }
    }
}

// ---------------------------------------------------------------------------
// Stage 1: after `with_index`.
// ---------------------------------------------------------------------------

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

pub struct OrderedQueryBuilder<'a> {
    tx: &'a Transaction,
    collection_name: String,
    with_index: Option<WithIndexFilter>,
    filters: Vec<FilterExpr>,
    order: Order,
}

// ---------------------------------------------------------------------------
// Assembly: turn accumulated builder state into a `query::Query`.
//
// This is where the decoded builder values cross over into the storage-facing
// representation: index bounds and filter operands are encoded into
// order-preserving key bytes via `encode_entry`. Name -> id resolution stays
// on the worker side of `run_query`.
// ---------------------------------------------------------------------------

/// Build the executable [`Query`] from the parts a stage has accumulated.
fn assemble(
    collection_name: String,
    with_index: Option<WithIndexFilter>,
    filters: Vec<FilterExpr>,
    order: Option<Order>,
    limit: Option<u64>,
) -> Result<Query> {
    Ok(Query {
        collection_name,
        with_index: with_index.map(build_with_index).transpose()?,
        filter: build_filter(filters)?,
        order,
        limit,
    })
}

/// Collapse the ANDed filter list into at most one [`Filter`].
fn build_filter(filters: Vec<FilterExpr>) -> Result<Option<Filter>> {
    let mut converted = Vec::with_capacity(filters.len());
    for expr in filters {
        converted.push(filter_expr_to_filter(expr)?);
    }
    Ok(match converted.len() {
        0 => None,
        1 => Some(converted.pop().unwrap()),
        _ => Some(Filter::And(converted)),
    })
}

/// Lower a builder [`FilterExpr`] (decoded values) into a [`Filter`] (encoded
/// comparison operands).
fn filter_expr_to_filter(expr: FilterExpr) -> Result<Filter> {
    Ok(match expr {
        FilterExpr::And(exprs) => Filter::And(
            exprs
                .into_iter()
                .map(filter_expr_to_filter)
                .collect::<Result<Vec<_>>>()?,
        ),
        FilterExpr::Or(exprs) => Filter::Or(
            exprs
                .into_iter()
                .map(filter_expr_to_filter)
                .collect::<Result<Vec<_>>>()?,
        ),
        FilterExpr::Not(inner) => Filter::Not(Box::new(filter_expr_to_filter(*inner)?)),
        FilterExpr::Comparison {
            field,
            operator,
            value,
        } => Filter::Comparison {
            field,
            operator,
            value: encode_entry(&[Some(&value)])?,
        },
    })
}

/// A single trailing range bound on the index (the unit that closes the `eq`
/// prefix).
enum RangeBound {
    Gt(Value),
    Gte(Value),
    Lt(Value),
    Lte(Value),
}

impl RangeBound {
    fn value(&self) -> &Value {
        match self {
            RangeBound::Gt(v) | RangeBound::Gte(v) | RangeBound::Lt(v) | RangeBound::Lte(v) => v,
        }
    }
}

/// Pair a bound with the names of the index fields its bytes encode, in index
/// order — this is what lets the worker check the bound against the resolved
/// index definition. An unbounded end constrains nothing, so it carries no
/// names.
fn attach(names: Vec<String>, bound: Bound<Vec<u8>>) -> (Vec<String>, Bound<Vec<u8>>) {
    match bound {
        Bound::Unbounded => (Vec::new(), Bound::Unbounded),
        bound => (names, bound),
    }
}

/// Encode the index filter into a `[lower, upper)`-style key range.
///
/// The equality units form a shared key prefix; an optional trailing range unit
/// bounds the next field. With no range, the result is a prefix scan over all
/// entries sharing the equality prefix. Each bound is tagged with the names of
/// the fields its bytes encode (the eq prefix, plus the range field on the side
/// the range constrains), so the worker can verify them against the index.
fn build_with_index(filter: WithIndexFilter) -> Result<WithIndex> {
    let WithIndexFilter { index_name, units } = filter;

    let mut eq_fields: Vec<String> = Vec::new();
    let mut eq_values: Vec<Value> = Vec::new();
    let mut range: Option<(String, RangeBound)> = None;
    for unit in units {
        match unit {
            WithIndexFilterUnit::EQ(f, v) => {
                eq_fields.push(f);
                eq_values.push(v);
            }
            WithIndexFilterUnit::GT(f, v) => range = Some((f, RangeBound::Gt(v))),
            WithIndexFilterUnit::GTE(f, v) => range = Some((f, RangeBound::Gte(v))),
            WithIndexFilterUnit::LT(f, v) => range = Some((f, RangeBound::Lt(v))),
            WithIndexFilterUnit::LTE(f, v) => range = Some((f, RangeBound::Lte(v))),
        }
    }

    let prefix_fields: Vec<Option<&Value>> = eq_values.iter().map(Some).collect();
    let prefix = encode_entry(&prefix_fields)?;
    let has_prefix = !eq_values.is_empty();

    // The half-open ends of the equality prefix alone, used to bound whichever
    // side of a range stays open (or both sides when there is no range).
    let prefix_start = if has_prefix {
        Bound::Included(prefix.clone())
    } else {
        Bound::Unbounded
    };
    let prefix_end = match prefix_successor(&prefix) {
        Some(successor) => Bound::Excluded(successor),
        None => Bound::Unbounded,
    };

    let (lower, upper) = match range {
        None => (
            attach(eq_fields.clone(), prefix_start),
            attach(eq_fields, prefix_end),
        ),
        Some((range_field, range)) => {
            let range_key = {
                let mut fields = prefix_fields;
                fields.push(Some(range.value()));
                encode_entry(&fields)?
            };
            let mut range_names = eq_fields.clone();
            range_names.push(range_field);
            match range {
                RangeBound::Gt(_) => (
                    attach(range_names, Bound::Excluded(range_key)),
                    attach(eq_fields, prefix_end),
                ),
                RangeBound::Gte(_) => (
                    attach(range_names, Bound::Included(range_key)),
                    attach(eq_fields, prefix_end),
                ),
                RangeBound::Lt(_) => (
                    attach(eq_fields, prefix_start),
                    attach(range_names, Bound::Excluded(range_key)),
                ),
                RangeBound::Lte(_) => (
                    attach(eq_fields, prefix_start),
                    attach(range_names, Bound::Included(range_key)),
                ),
            }
        }
    };

    Ok(WithIndex {
        index_name,
        lower,
        upper,
    })
}

/// The smallest key strictly greater than every key beginning with `prefix`:
/// drop trailing `0xFF` bytes and increment the last remaining byte. Returns
/// `None` when `prefix` is empty or all `0xFF` — there is no finite successor,
/// so that side of the scan is unbounded.
fn prefix_successor(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut out = prefix.to_vec();
    while let Some(&last) = out.last() {
        if last == 0xFF {
            out.pop();
        } else {
            *out.last_mut().unwrap() = last + 1;
            return Some(out);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Per-stage assembly: each stage builds the `Query` from the fields it holds.
// ---------------------------------------------------------------------------

impl QueryBuilder<'_> {
    fn into_query(self, limit: Option<u64>) -> Result<Query> {
        assemble(self.collection_name, None, Vec::new(), None, limit)
    }
}

impl QueryBuilderWithIndex<'_> {
    fn into_query(self, limit: Option<u64>) -> Result<Query> {
        assemble(
            self.collection_name,
            Some(self.with_index),
            Vec::new(),
            None,
            limit,
        )
    }
}

impl FilteredQueryBuilder<'_> {
    fn into_query(self, limit: Option<u64>) -> Result<Query> {
        assemble(
            self.collection_name,
            self.with_index,
            self.filters,
            None,
            limit,
        )
    }
}

impl OrderedQueryBuilder<'_> {
    fn into_query(self, limit: Option<u64>) -> Result<Query> {
        assemble(
            self.collection_name,
            self.with_index,
            self.filters,
            Some(self.order),
            limit,
        )
    }
}

// ---------------------------------------------------------------------------
// Terminals. The only stages that touch the database: assemble the `Query`,
// run it, and shape the result. The `_as` variants deserialize each document
// into the caller's type. Every chainable stage can terminate.
// ---------------------------------------------------------------------------

macro_rules! impl_terminals {
    ($stage:ident) => {
        impl<'a> $stage<'a> {
            /// The first `limit` results.
            pub async fn take(self, limit: u64) -> Result<Vec<Document>> {
                let tx = self.tx;
                let query = self.into_query(Some(limit))?;
                tx.run_query(query).await
            }

            pub async fn take_as<D: DeserializeOwned>(self, limit: u64) -> Result<Vec<D>> {
                let tx = self.tx;
                let query = self.into_query(Some(limit))?;
                deserialize_all(tx.run_query(query).await?)
            }

            /// The first result, if any.
            pub async fn first(self) -> Result<Option<Document>> {
                let tx = self.tx;
                let query = self.into_query(Some(1))?;
                Ok(tx.run_query(query).await?.into_iter().next())
            }

            pub async fn first_as<D: DeserializeOwned>(self) -> Result<Option<D>> {
                let tx = self.tx;
                let query = self.into_query(Some(1))?;
                match tx.run_query(query).await?.into_iter().next() {
                    Some(doc) => Ok(Some(from_document(doc)?)),
                    None => Ok(None),
                }
            }

            /// The single result; errors unless exactly one document matched.
            pub async fn unique(self) -> Result<Document> {
                let tx = self.tx;
                let query = self.into_query(Some(2))?;
                exactly_one(tx.run_query(query).await?)
            }

            pub async fn unique_as<D: DeserializeOwned>(self) -> Result<D> {
                let tx = self.tx;
                let query = self.into_query(Some(2))?;
                Ok(from_document(exactly_one(tx.run_query(query).await?)?)?)
            }

            /// Every matching document.
            pub async fn collect(self) -> Result<Vec<Document>> {
                let tx = self.tx;
                let query = self.into_query(None)?;
                tx.run_query(query).await
            }

            pub async fn collect_as<D: DeserializeOwned>(self) -> Result<Vec<D>> {
                let tx = self.tx;
                let query = self.into_query(None)?;
                deserialize_all(tx.run_query(query).await?)
            }
        }
    };
}

impl_terminals!(QueryBuilder);
impl_terminals!(QueryBuilderWithIndex);
impl_terminals!(FilteredQueryBuilder);
impl_terminals!(OrderedQueryBuilder);

/// Deserialize every document into `D`.
fn deserialize_all<D: DeserializeOwned>(docs: Vec<Document>) -> Result<Vec<D>> {
    docs.into_iter()
        .map(|doc| from_document(doc).map_err(Error::from))
        .collect()
}

/// Unwrap the sole document, or error if there were zero or several.
fn exactly_one(mut docs: Vec<Document>) -> Result<Document> {
    match docs.len() {
        1 => Ok(docs.pop().unwrap()),
        n => Err(Error::Other(
            format!("unique() expected exactly one document, found {n}").into(),
        )),
    }
}

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
