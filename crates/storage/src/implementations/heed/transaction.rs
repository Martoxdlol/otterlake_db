use crate::{
    implementations::heed::cursors::{HeedDatastoreCursor, HeedDatastoreIndexCursor},
    traits::DatastoreTransaction,
};

pub struct HeedDatastoreTransaction<'env> {
    env: &'env heed3::Env,
    tx: heed3::RoTxn<'env, heed3::WithTls>,
    ts: u64,
}

impl<'env> HeedDatastoreTransaction<'env> {
    pub fn new(env: &'env heed3::Env, tx: heed3::RoTxn<'env, heed3::WithTls>, ts: u64) -> Self {
        Self { env, tx, ts }
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
}
