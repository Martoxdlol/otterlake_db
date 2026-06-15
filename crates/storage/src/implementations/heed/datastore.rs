use std::sync::Arc;

use heed3::Database;
use heed3::Env;
use heed3::types::Bytes;

use crate::{implementations::heed::transaction::HeedDatastoreTransaction, traits::Datastore};

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
    /// Documents themselves
    documents: Database<Bytes, Bytes>,
}

impl Datastore for HeedStorageEngine {
    fn transaction(
        &self,
        ts: u64,
    ) -> crate::error::Result<impl crate::traits::DatastoreTransaction + '_> {
        let tx = self.env.read_txn()?;

        Ok(HeedDatastoreTransaction::new(&self.env, tx, ts))
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
