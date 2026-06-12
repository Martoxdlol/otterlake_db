use crate::traits::{DatastoreCursor, DatastoreIndexCursor};

pub struct HeedDatastoreCursor {}

impl DatastoreCursor for HeedDatastoreCursor {
    fn next(&mut self) -> Option<(crate::types::DocumentId, crate::types::Value)> {
        todo!()
    }
}

pub struct HeedDatastoreIndexCursor {}

impl DatastoreIndexCursor for HeedDatastoreIndexCursor {
    fn next(&mut self) -> Option<(crate::types::Value, crate::types::DocumentId)> {
        todo!()
    }
}
