/// Global auto incrementing id
pub type TS = u64;

/// UUID v7 (it is important that is v7)
pub type DocumentId = u128;

/// Collection id, negative numbers are temporary indicators used in write sets for new collections
pub type CollectionId = i64;

/// Index id, negative numbers are temporary indicators used in write sets for new indexes
pub type IndexId = i64;

// Arbitrary binary data
pub type Value = Vec<u8>;

/// Direction for cursors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Reverse,
}

/// Cursor start position.
///
/// Cursors may start unbounded, or at a key that is included/excluded from
/// the result stream. Cursor limits are controlled by callers consuming
/// `next()`, not by this storage layer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CursorStart<T> {
    #[default]
    Unbounded,
    Included(T),
    Excluded(T),
}

/// A document visible to a timestamped read transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentEntry {
    pub id: DocumentId,
    pub value: Value,
}

/// A logical index position. Ordering is by index value, then document id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndexPosition {
    pub value: Value,
    pub document_id: DocumentId,
}

/// A visible index entry returned by index cursors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub value: Value,
    pub document_id: DocumentId,
    pub document_value: Value,
}

/// A collection catalog row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionCatalogEntry {
    pub id: CollectionId,
    pub name: String,
    pub metadata: Value,
}

/// An index catalog row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexCatalogEntry {
    pub collection_id: CollectionId,
    pub id: IndexId,
    pub name: String,
    pub metadata: Value,
}
