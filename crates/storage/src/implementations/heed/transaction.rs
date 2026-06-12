use crate::{
    implementations::heed::cursors::{HeedDatastoreCursor, HeedDatastoreIndexCursor},
    traits::DatastoreTransaction,
};

pub struct HeedDatastoreTransaction {}

impl DatastoreTransaction for HeedDatastoreTransaction {
    fn collection(&self, name: &str) -> crate::error::Result<Option<crate::types::CollectionId>> {
        todo!()
    }

    fn get(
        &self,
        collection: crate::types::CollectionId,
        key: crate::types::DocumentId,
    ) -> crate::error::Result<Option<crate::types::Value>> {
        todo!()
    }

    fn get_cursor(
        &self,
        collection: crate::types::CollectionId,
        key: crate::types::DocumentId,
        direction: crate::types::Direction,
        exclude_start: bool,
    ) -> crate::error::Result<HeedDatastoreCursor> {
        todo!()
    }

    fn get_index_cursor(
        &self,
        index: crate::types::CollectionId,
        key: crate::types::DocumentId,
        direction: crate::types::Direction,
        exclude_start: bool,
    ) -> crate::error::Result<HeedDatastoreIndexCursor> {
        todo!()
    }
}
