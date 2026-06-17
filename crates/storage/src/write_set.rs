use std::collections::BTreeMap;

use crate::types::{CollectionId, DocumentId, IndexId, IndexPosition, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentWrite {
    Put(Value),
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexWrite {
    Put,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexCatalogWrite {
    pub name: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollectionWriteSet {
    /// Final document writes keyed by document id.
    pub documents: BTreeMap<DocumentId, DocumentWrite>,
    /// Final index writes keyed by index id, index value, and document id.
    pub index_entries: BTreeMap<IndexId, BTreeMap<IndexPosition, IndexWrite>>,
    /// Metadata of collection.
    pub metadata: Option<Value>,
}

/// Set of changes to be applied to the database.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteSet {
    pub collections: BTreeMap<CollectionId, CollectionWriteSet>,
    pub new_collections: BTreeMap<CollectionId, String>,
    pub new_indexes: BTreeMap<CollectionId, BTreeMap<IndexId, IndexCatalogWrite>>,
    pub ts: u64,
}

impl WriteSet {
    pub fn put(&mut self, collection_id: CollectionId, key: DocumentId, value: Value) {
        self.collection_mut(collection_id)
            .documents
            .insert(key, DocumentWrite::Put(value));
    }

    pub fn put_many<I>(&mut self, collection_id: CollectionId, documents: I)
    where
        I: IntoIterator<Item = (DocumentId, Value)>,
    {
        let collection = self.collection_mut(collection_id);
        for (key, value) in documents {
            collection.documents.insert(key, DocumentWrite::Put(value));
        }
    }

    pub fn delete(&mut self, collection_id: CollectionId, key: DocumentId) {
        self.collection_mut(collection_id)
            .documents
            .insert(key, DocumentWrite::Deleted);
    }

    pub fn delete_many<I>(&mut self, collection_id: CollectionId, keys: I)
    where
        I: IntoIterator<Item = DocumentId>,
    {
        let collection = self.collection_mut(collection_id);
        for key in keys {
            collection.documents.insert(key, DocumentWrite::Deleted);
        }
    }

    pub fn put_index_entry(
        &mut self,
        collection_id: CollectionId,
        index_id: IndexId,
        key: Value,
        document_id: DocumentId,
    ) {
        self.write_index_entry(collection_id, index_id, key, document_id, IndexWrite::Put);
    }

    pub fn put_index_entries<I>(&mut self, collection_id: CollectionId, entries: I)
    where
        I: IntoIterator<Item = (IndexId, Value, DocumentId)>,
    {
        let collection = self.collection_mut(collection_id);
        for (index_id, key, document_id) in entries {
            collection
                .index_entries
                .entry(index_id)
                .or_default()
                .insert(
                    IndexPosition {
                        value: key,
                        document_id,
                    },
                    IndexWrite::Put,
                );
        }
    }

    pub fn delete_index_entry(
        &mut self,
        collection_id: CollectionId,
        index_id: IndexId,
        key: Value,
        document_id: DocumentId,
    ) {
        self.write_index_entry(
            collection_id,
            index_id,
            key,
            document_id,
            IndexWrite::Deleted,
        );
    }

    pub fn delete_index_entries<I>(&mut self, collection_id: CollectionId, entries: I)
    where
        I: IntoIterator<Item = (IndexId, Value, DocumentId)>,
    {
        let collection = self.collection_mut(collection_id);
        for (index_id, key, document_id) in entries {
            collection
                .index_entries
                .entry(index_id)
                .or_default()
                .insert(
                    IndexPosition {
                        value: key,
                        document_id,
                    },
                    IndexWrite::Deleted,
                );
        }
    }

    pub fn new_collection(&mut self, collection_id: CollectionId, name: String) {
        self.new_collections.insert(collection_id, name);
    }

    pub fn new_index(
        &mut self,
        collection_id: CollectionId,
        index_id: IndexId,
        name: String,
        metadata: Value,
    ) {
        self.new_indexes
            .entry(collection_id)
            .or_default()
            .insert(index_id, IndexCatalogWrite { name, metadata });
    }

    pub fn update_collection_metadata(&mut self, collection_id: CollectionId, metadata: Value) {
        self.collection_mut(collection_id).metadata = Some(metadata);
    }

    fn write_index_entry(
        &mut self,
        collection_id: CollectionId,
        index_id: IndexId,
        key: Value,
        document_id: DocumentId,
        write: IndexWrite,
    ) {
        self.collection_mut(collection_id)
            .index_entries
            .entry(index_id)
            .or_default()
            .insert(
                IndexPosition {
                    value: key,
                    document_id,
                },
                write,
            );
    }

    fn collection_mut(&mut self, collection_id: CollectionId) -> &mut CollectionWriteSet {
        self.collections.entry(collection_id).or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_writes_are_last_write_wins() {
        let mut writes = WriteSet::default();

        writes.put(1, 10, b"first".to_vec());
        assert_eq!(
            writes.collections[&1].documents[&10],
            DocumentWrite::Put(b"first".to_vec())
        );

        writes.delete(1, 10);
        assert_eq!(
            writes.collections[&1].documents[&10],
            DocumentWrite::Deleted
        );

        writes.put(1, 10, b"second".to_vec());
        assert_eq!(
            writes.collections[&1].documents[&10],
            DocumentWrite::Put(b"second".to_vec())
        );
    }

    #[test]
    fn document_bulk_writes_are_last_write_wins() {
        let mut writes = WriteSet::default();

        writes.delete(1, 10);
        writes.put_many(1, vec![(10, b"live".to_vec()), (20, b"other".to_vec())]);
        writes.delete_many(1, vec![20]);

        assert_eq!(
            writes.collections[&1].documents[&10],
            DocumentWrite::Put(b"live".to_vec())
        );
        assert_eq!(
            writes.collections[&1].documents[&20],
            DocumentWrite::Deleted
        );
    }

    #[test]
    fn index_writes_are_last_write_wins() {
        let mut writes = WriteSet::default();
        let position = IndexPosition {
            value: b"a".to_vec(),
            document_id: 10,
        };

        writes.put_index_entry(1, 2, b"a".to_vec(), 10);
        assert_eq!(
            writes.collections[&1].index_entries[&2][&position],
            IndexWrite::Put
        );

        writes.delete_index_entry(1, 2, b"a".to_vec(), 10);
        assert_eq!(
            writes.collections[&1].index_entries[&2][&position],
            IndexWrite::Deleted
        );

        writes.put_index_entries(1, vec![(2, b"a".to_vec(), 10)]);
        assert_eq!(
            writes.collections[&1].index_entries[&2][&position],
            IndexWrite::Put
        );
    }
}
