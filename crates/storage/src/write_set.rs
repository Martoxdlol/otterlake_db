use std::collections::HashMap;

use crate::types::{CollectionId, DocumentId, IndexId, Value};

pub struct CollectionWriteSet {
    /// Documents to be inserted or updated (document id + serialized document)
    pub documents: Vec<(DocumentId, Vec<u8>)>,
    /// Documents to be deleted
    pub deleted_keys: Vec<DocumentId>,
    /// Index entries to be inserted or updated (index id + serialized index key + document id)
    pub index_entries: Vec<(IndexId, Vec<u8>, DocumentId)>,
    /// Index entries to be deleted (index id + serialized index key + document id)
    pub deleted_index_entries: Vec<(IndexId, Vec<u8>, DocumentId)>,
    /// Metadata of collection
    pub metadata: Option<Vec<u8>>,
}

/// Set of changes to be applied to the database
pub struct WriteSet {
    pub collections: HashMap<CollectionId, CollectionWriteSet>,
    pub new_collections: Vec<(CollectionId, String)>,
    pub new_indexes: Vec<(CollectionId, IndexId, String, Value)>,
    pub ts: u64,
}
