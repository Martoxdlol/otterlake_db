use crate::{
    error::Result,
    types::{CollectionId, Direction, DocumentId, Value},
    write_set::WriteSet,
};

pub trait DatastoreCursor {
    /// Returns next document id + document value
    fn next(&mut self) -> Option<(DocumentId, Value)>;
}

pub trait DatastoreIndexCursor {
    /// Returns next index entry value + document id
    fn next(&mut self) -> Option<(Value, DocumentId)>;
}

pub trait DatastoreTransaction {
    /// Get collection id by name
    fn collection(&self, name: &str) -> Result<Option<CollectionId>>;

    /// Get content of a single document
    fn get(&self, collection: CollectionId, key: DocumentId) -> Result<Option<Value>>;

    fn get_cursor(
        &self,
        collection: CollectionId,
        key: DocumentId,
        direction: Direction,
        exclude_start: bool,
    ) -> Result<impl DatastoreCursor>;

    fn get_index_cursor(
        &self,
        index: CollectionId,
        key: DocumentId,
        direction: Direction,
        exclude_start: bool,
    ) -> Result<impl DatastoreIndexCursor>;
}

pub trait Datastore: Clone + Send + Sync {
    /// Starts a read transaction from version
    fn transaction(&self, version: u64) -> Result<impl DatastoreTransaction + '_>;

    /// Apply a set of changes (will be immediately visible to new transactions)
    fn put(&self, batch: WriteSet) -> Result<()>;

    /// Create a index with opaque configuration
    /// It does NOT auto index existing documents
    fn create_index(&self, collection: CollectionId, name: &str, config: Value) -> Result<()>;

    /// Flushes all pending changes to disk
    fn flush(&self) -> Result<()>;

    /// Set global timestamp counter
    fn set_ts(&self, ts: u64) -> Result<()>;

    /// Get global timestamp counter
    fn get_ts(&self) -> Result<u64>;
}
