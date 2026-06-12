use std::sync::Arc;

use heed3::Database;
use heed3::Env;
use heed3::types::Bytes;

use crate::implementations::heed::transaction::HeedDatastoreTransaction;
use crate::traits::Datastore;

#[derive(Clone)]
pub struct HeedStorageEngine {
    /// Heed environment
    env: Arc<Env>,
    /// Catalog of index names to index ids + config
    indexes_catalog: Database<String, Bytes>,
    /// Catalog of collection names to collection ids + metadata
    collections_catalog: Database<String, Bytes>,
    /// Each index entry for all docs
    index_entries: Database<Bytes, Bytes>,
    /// Maps every document id to the entries in index_entries (for cleaning up)
    index_tracking: Database<Bytes, Bytes>,
    /// Documents themselves
    documents: Database<Bytes, Bytes>,
}

impl Datastore for HeedStorageEngine {
    fn transaction(&self, version: u64) -> crate::error::Result<HeedDatastoreTransaction> {
        todo!()
    }

    fn put(&self, batch: crate::write_set::WriteSet) -> crate::error::Result<()> {
        todo!()
    }

    fn create_index(
        &self,
        collection: crate::types::CollectionId,
        name: &str,
        config: crate::types::Value,
    ) -> crate::error::Result<()> {
        todo!()
    }

    fn flush(&self) -> crate::error::Result<()> {
        todo!()
    }

    fn set_ts(&self, ts: u64) -> crate::error::Result<()> {
        todo!()
    }

    fn get_ts(&self) -> crate::error::Result<u64> {
        todo!()
    }
}
