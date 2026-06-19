use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
};

use storage::types::{CollectionId, DocumentId, IndexId};

pub struct ReadSet {
    /// Intervals grouped by `(CollectionId, IndexId)`.
    pub intervals: BTreeMap<(CollectionId, IndexId), Vec<ReadInterval>>,
    /// Document IDs read by id directly
    pub document_ids: Vec<DocumentRead>,
    /// Next query ID to assign.
    next_query_id: u64,
}

impl ReadSet {
    /// Create a new empty read set with query IDs starting at 0.
    pub fn new() -> Self {
        Self {
            intervals: BTreeMap::new(),
            document_ids: Vec::new(),
            next_query_id: 0,
        }
    }

    /// Add an interval to the read set.
    pub fn add_interval(
        &mut self,
        collection_id: CollectionId,
        index_id: IndexId,
        lower: Bound<Vec<u8>>,
        upper: Bound<Vec<u8>>,
        limit_boundary: Option<LimitBoundary>,
    ) {
        self.intervals
            .entry((collection_id, index_id))
            .or_default()
            .push(ReadInterval {
                query_id: self.next_query_id,
                lower,
                upper,
                limit_boundary,
            });
        self.next_query_id += 1;
    }

    /// Add a document ID to the read set.
    pub fn add_document_id(&mut self, document_id: DocumentId) {
        self.document_ids.push(DocumentRead {
            query_id: self.next_query_id,
            document_id,
        });
        self.next_query_id += 1;
    }
}

#[derive(Debug, Clone)]
pub struct ReadInterval {
    /// Which query produced this interval.
    pub query_id: u64,
    /// Original range lower bound (before any LIMIT tightening).
    pub lower: Bound<Vec<u8>>,
    /// Original range upper bound (before any LIMIT tightening).
    pub upper: Bound<Vec<u8>>,
    /// LIMIT tightening. `None` = scan exhausted the range (full original coverage).
    pub limit_boundary: Option<LimitBoundary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitBoundary {
    /// ASC scan stopped after returning the doc with sort key `K`.
    ///
    /// Effective upper = `Excluded(successor(K))`. Cleared when `K`'s doc is
    /// deleted or its key moves outside the interval.
    Upper(Vec<u8>),

    /// DESC scan stopped after returning the doc with sort key `K`.
    ///
    /// Effective lower = `Included(K)`. Cleared when `K`'s doc is deleted or
    /// its key moves outside the interval.
    Lower(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct DocumentRead {
    /// Which query produced this interval.
    pub query_id: u64,
    /// Document ID read directly by id.
    pub document_id: DocumentId,
}
