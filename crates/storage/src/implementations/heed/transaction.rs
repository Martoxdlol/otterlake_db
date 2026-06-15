use crate::{
    implementations::heed::{
        cursors::{HeedDatastoreCursor, HeedDatastoreIndexCursor},
        datastore::HeedStorageEngine,
    },
    traits::DatastoreTransaction,
};

pub struct HeedDatastoreTransaction<'env> {
    engine: &'env HeedStorageEngine,
    tx: heed3::RoTxn<'env, heed3::WithTls>,
    ts: u64,
}

impl<'env> HeedDatastoreTransaction<'env> {
    pub fn new(
        engine: &'env HeedStorageEngine,
        tx: heed3::RoTxn<'env, heed3::WithTls>,
        ts: u64,
    ) -> Self {
        Self { engine, tx, ts }
    }
}

impl DatastoreTransaction for HeedDatastoreTransaction<'_> {
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

    fn get_collections_catalog_cursor(
        &self,
        name: &str,
        direction: crate::types::Direction,
    ) -> crate::error::Result<HeedDatastoreCursor> {
        todo!()
    }

    fn get_indexes_catalog_cursor(
        &self,
        collection: crate::types::CollectionId,
        name: &str,
        direction: crate::types::Direction,
    ) -> crate::error::Result<HeedDatastoreCursor> {
        todo!()
    }
}
