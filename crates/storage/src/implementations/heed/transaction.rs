use std::ops::Bound;

use crate::{
    implementations::heed::{
        cursors::{
            HeedCollectionCatalogCursor, HeedDocumentCursor, HeedIndexCatalogCursor,
            HeedIndexCursor, HeedIndexCursorOptions, HeedRawCursor,
        },
        datastore::HeedStorageEngine,
        encoding::{
            decode_collection_catalog_value, decode_document_key, decode_document_value,
            encode_collection_prefix, encode_document_key, encode_index_catalog_key,
        },
    },
    traits::{DatastoreCursor, DatastoreTransaction},
    types::{
        CollectionCatalogEntry, CollectionId, CursorStart, Direction, DocumentEntry, DocumentId,
        IndexCatalogEntry, IndexEntry, IndexId, IndexPosition, Value,
    },
};

pub struct HeedDatastoreTransaction<'env> {
    engine: &'env HeedStorageEngine,
    tx: heed3::RoTxn<'env, heed3::WithoutTls>,
    ts: u64,
}

impl<'env> HeedDatastoreTransaction<'env> {
    pub fn new(
        engine: &'env HeedStorageEngine,
        tx: heed3::RoTxn<'env, heed3::WithoutTls>,
        ts: u64,
    ) -> Self {
        Self { engine, tx, ts }
    }

    fn raw_cursor(
        &self,
        database: heed3::Database<heed3::types::Bytes, heed3::types::Bytes>,
        bounds: (Bound<Vec<u8>>, Bound<Vec<u8>>),
        direction: Direction,
    ) -> crate::error::Result<HeedRawCursor<'_>> {
        let borrowed_bounds = (borrow_bound(&bounds.0), borrow_bound(&bounds.1));
        Ok(match direction {
            Direction::Forward => {
                HeedRawCursor::Forward(database.range(&self.tx, &borrowed_bounds)?)
            }
            Direction::Reverse => {
                HeedRawCursor::Reverse(database.rev_range(&self.tx, &borrowed_bounds)?)
            }
        })
    }
}

impl DatastoreTransaction for HeedDatastoreTransaction<'_> {
    fn collection(&self, name: &str) -> crate::error::Result<Option<CollectionId>> {
        self.engine
            .collections_catalog
            .get(&self.tx, name.as_bytes())?
            .map(decode_collection_catalog_value)
            .transpose()
            .map(|row| row.map(|(id, _)| id))
    }

    fn get(
        &self,
        collection: CollectionId,
        key: DocumentId,
    ) -> crate::error::Result<Option<Value>> {
        let upper_key = encode_document_key(collection, key, self.ts);
        let Some((stored_key, value)) = self
            .engine
            .documents
            .get_lower_than_or_equal_to(&self.tx, &upper_key)?
        else {
            return Ok(None);
        };

        let (stored_collection, stored_document_id, stored_version) =
            decode_document_key(stored_key)?;
        if stored_collection != collection || stored_document_id != key || stored_version > self.ts
        {
            return Ok(None);
        }

        decode_document_value(value)
    }

    fn get_cursor(
        &self,
        collection: CollectionId,
        start: CursorStart<DocumentId>,
        direction: Direction,
    ) -> crate::error::Result<impl DatastoreCursor<Item = DocumentEntry>> {
        let bounds = document_cursor_bounds(collection, start, direction);
        let raw = self.raw_cursor(self.engine.documents, bounds, direction)?;
        Ok(HeedDocumentCursor::new(
            raw,
            collection,
            self.ts,
            direction == Direction::Reverse,
        ))
    }

    fn get_index_cursor(
        &self,
        collection: CollectionId,
        index: IndexId,
        start: CursorStart<IndexPosition>,
        direction: Direction,
    ) -> crate::error::Result<impl DatastoreCursor<Item = IndexEntry>> {
        HeedIndexCursor::new(HeedIndexCursorOptions {
            tx: &self.tx,
            collection,
            index_id: index,
            index_edges: self.engine.index_edges,
            index_leaves: self.engine.index_leaves,
            documents: self.engine.documents,
            ts: self.ts,
            start,
            direction,
        })
    }

    fn get_collections_catalog_cursor(
        &self,
        start: CursorStart<String>,
        direction: Direction,
    ) -> crate::error::Result<impl DatastoreCursor<Item = CollectionCatalogEntry>> {
        let bounds = string_cursor_bounds(start, direction);
        let raw = self.raw_cursor(self.engine.collections_catalog, bounds, direction)?;
        Ok(HeedCollectionCatalogCursor::new(raw))
    }

    fn get_indexes_catalog_cursor(
        &self,
        collection: CollectionId,
        start: CursorStart<String>,
        direction: Direction,
    ) -> crate::error::Result<impl DatastoreCursor<Item = IndexCatalogEntry>> {
        let bounds = index_catalog_cursor_bounds(collection, start, direction);
        let raw = self.raw_cursor(self.engine.indexes_catalog, bounds, direction)?;
        Ok(HeedIndexCatalogCursor::new(raw, collection))
    }
}

fn document_cursor_bounds(
    collection: CollectionId,
    start: CursorStart<DocumentId>,
    direction: Direction,
) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
    let collection_start = Bound::Included(encode_collection_prefix(collection));
    let collection_end = collection_end_bound(collection);

    match direction {
        Direction::Forward => {
            let start = match start {
                CursorStart::Unbounded => collection_start,
                CursorStart::Included(document_id) => {
                    Bound::Included(encode_document_key(collection, document_id, 0))
                }
                CursorStart::Excluded(document_id) => {
                    Bound::Excluded(encode_document_key(collection, document_id, u64::MAX))
                }
            };
            (start, collection_end)
        }
        Direction::Reverse => {
            let end = match start {
                CursorStart::Unbounded => collection_end,
                CursorStart::Included(document_id) => {
                    Bound::Included(encode_document_key(collection, document_id, u64::MAX))
                }
                CursorStart::Excluded(document_id) => {
                    Bound::Excluded(encode_document_key(collection, document_id, 0))
                }
            };
            (collection_start, end)
        }
    }
}

fn string_cursor_bounds(
    start: CursorStart<String>,
    direction: Direction,
) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
    match direction {
        Direction::Forward => (
            cursor_start_to_lower_bound(start, String::into_bytes),
            Bound::Unbounded,
        ),
        Direction::Reverse => (
            Bound::Unbounded,
            cursor_start_to_upper_bound(start, String::into_bytes),
        ),
    }
}

fn index_catalog_cursor_bounds(
    collection: CollectionId,
    start: CursorStart<String>,
    direction: Direction,
) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
    let collection_start = Bound::Included(encode_collection_prefix(collection));
    let collection_end = collection_end_bound(collection);

    match direction {
        Direction::Forward => {
            let start = match start {
                CursorStart::Unbounded => collection_start,
                CursorStart::Included(name) => {
                    Bound::Included(encode_index_catalog_key(collection, &name))
                }
                CursorStart::Excluded(name) => {
                    Bound::Excluded(encode_index_catalog_key(collection, &name))
                }
            };
            (start, collection_end)
        }
        Direction::Reverse => {
            let end = match start {
                CursorStart::Unbounded => collection_end,
                CursorStart::Included(name) => {
                    Bound::Included(encode_index_catalog_key(collection, &name))
                }
                CursorStart::Excluded(name) => {
                    Bound::Excluded(encode_index_catalog_key(collection, &name))
                }
            };
            (collection_start, end)
        }
    }
}

fn cursor_start_to_lower_bound<T>(
    start: CursorStart<T>,
    encode: impl FnOnce(T) -> Vec<u8>,
) -> Bound<Vec<u8>> {
    match start {
        CursorStart::Unbounded => Bound::Unbounded,
        CursorStart::Included(value) => Bound::Included(encode(value)),
        CursorStart::Excluded(value) => Bound::Excluded(encode(value)),
    }
}

fn cursor_start_to_upper_bound<T>(
    start: CursorStart<T>,
    encode: impl FnOnce(T) -> Vec<u8>,
) -> Bound<Vec<u8>> {
    match start {
        CursorStart::Unbounded => Bound::Unbounded,
        CursorStart::Included(value) => Bound::Included(encode(value)),
        CursorStart::Excluded(value) => Bound::Excluded(encode(value)),
    }
}

fn collection_end_bound(collection: CollectionId) -> Bound<Vec<u8>> {
    collection
        .checked_add(1)
        .map(encode_collection_prefix)
        .map_or(Bound::Unbounded, Bound::Excluded)
}

fn borrow_bound(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
    match bound {
        Bound::Included(value) => Bound::Included(value.as_slice()),
        Bound::Excluded(value) => Bound::Excluded(value.as_slice()),
        Bound::Unbounded => Bound::Unbounded,
    }
}
