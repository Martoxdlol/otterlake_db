use crate::{
    error::Result,
    types::{
        CollectionCatalogEntry, CollectionId, CursorStart, Direction, DocumentEntry, DocumentId,
        IndexCatalogEntry, IndexEntry, IndexId, IndexPosition, Value,
    },
    write_set::WriteSet,
};

pub trait DatastoreCursor {
    type Item;

    /// Returns the next item in cursor order.
    fn next(&mut self) -> Result<Option<Self::Item>>;
}

pub trait DatastoreTransaction {
    /// Get collection id by name
    fn collection(&self, name: &str) -> Result<Option<CollectionId>>;

    /// Get content of a single document
    fn get(&self, collection: CollectionId, key: DocumentId) -> Result<Option<Value>>;

    fn get_cursor(
        &self,
        collection: CollectionId,
        start: CursorStart<DocumentId>,
        direction: Direction,
    ) -> Result<impl DatastoreCursor<Item = DocumentEntry>>;

    fn get_index_cursor(
        &self,
        collection: CollectionId,
        index: IndexId,
        start: CursorStart<IndexPosition>,
        direction: Direction,
    ) -> Result<impl DatastoreCursor<Item = IndexEntry>>;

    fn get_collections_catalog_cursor(
        &self,
        start: CursorStart<String>,
        direction: Direction,
    ) -> Result<impl DatastoreCursor<Item = CollectionCatalogEntry>>;

    fn get_indexes_catalog_cursor(
        &self,
        collection: CollectionId,
        start: CursorStart<String>,
        direction: Direction,
    ) -> Result<impl DatastoreCursor<Item = IndexCatalogEntry>>;

    fn put(&mut self, collection_id: CollectionId, key: DocumentId, value: Value) -> Result<()>;

    fn put_many(
        &mut self,
        collection_id: CollectionId,
        documents: Vec<(DocumentId, Value)>,
    ) -> Result<()>;

    fn put_index_entry(
        &mut self,
        collection_id: CollectionId,
        index_id: IndexId,
        key: Value,
        document_id: DocumentId,
    ) -> Result<()>;

    fn put_index_entries(
        &mut self,
        collection_id: CollectionId,
        entries: Vec<(IndexId, Value, DocumentId)>,
    ) -> Result<()>;

    fn delete(&mut self, collection_id: CollectionId, key: DocumentId) -> Result<()>;

    fn delete_many(&mut self, collection_id: CollectionId, keys: Vec<DocumentId>) -> Result<()>;

    fn delete_index_entry(
        &mut self,
        collection_id: CollectionId,
        index_id: IndexId,
        key: Value,
        document_id: DocumentId,
    ) -> Result<()>;

    fn delete_index_entries(
        &mut self,
        collection_id: CollectionId,
        entries: Vec<(IndexId, Value, DocumentId)>,
    ) -> Result<()>;

    fn new_collection(&mut self, collection_id: CollectionId, name: String) -> Result<()>;

    fn new_index(
        &mut self,
        collection_id: CollectionId,
        index_id: IndexId,
        name: String,
        metadata: Value,
    ) -> Result<()>;

    fn update_collection_metadata(
        &mut self,
        collection_id: CollectionId,
        metadata: Value,
    ) -> Result<()>;
}

pub trait Datastore: Clone + Send + Sync {
    /// Starts a read transaction from version
    fn transaction(&self, version: u64) -> Result<impl DatastoreTransaction + '_>;

    /// Apply a set of changes (will be immediately visible to new transactions)
    fn put(&self, batch: WriteSet) -> Result<()>;

    /// Flushes all pending changes to disk
    fn flush(&self) -> Result<()>;

    /// Set global timestamp counter. Implementations must reject values lower than the current one.
    fn set_ts(&self, ts: u64) -> Result<()>;

    /// Get global timestamp counter
    fn get_ts(&self) -> Result<u64>;
}
