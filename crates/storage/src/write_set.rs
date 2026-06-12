use std::collections::HashMap;

use crate::types::{CollectionId, DocumentId, IndexId};

pub struct CollectionWriteSet {
    /// Documents to be inserted or updated (document id + serialized document)
    pub documents: Vec<(DocumentId, Vec<u8>)>,
    /// Documents to be deleted
    pub deleted_keys: Vec<Vec<u8>>,
    /// Index entries to be inserted or updated (index id + serialized index key + document id)
    pub index_entries: Vec<(IndexId, Vec<u8>, DocumentId)>,
}

/// Set of changes to be applied to the database
pub struct WriteSet {
    pub collections: HashMap<CollectionId, CollectionWriteSet>,
}
