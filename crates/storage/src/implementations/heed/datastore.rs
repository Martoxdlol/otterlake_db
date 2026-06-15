use std::sync::Arc;

use heed3::Database;
use heed3::Env;
use heed3::types::Bytes;

use crate::{implementations::heed::transaction::HeedDatastoreTransaction, traits::Datastore};

#[derive(Clone)]
pub struct HeedStorageEngine {
    /// Heed environment
    pub(super) env: Arc<Env>,
    /// Catalog of index names to index ids + config
    pub(super) indexes_catalog: Database<String, Bytes>,
    /// Catalog of collection names to collection ids + metadata
    pub(super) collections_catalog: Database<String, Bytes>,
    /// Each index entry for all docs
    pub(super) index_entries: Database<Bytes, Bytes>,
    /// Documents themselves
    pub(super) documents: Database<Bytes, Bytes>,
}

impl HeedStorageEngine {
    pub fn new(env: Arc<Env>) -> crate::error::Result<Self> {
        let mut wtxn = env.write_txn()?;

        let indexes_catalog = env.create_database(&mut wtxn, Some("indexes_catalog"))?;
        let collections_catalog = env.create_database(&mut wtxn, Some("collections_catalog"))?;
        let index_entries = env.create_database(&mut wtxn, Some("index_entries"))?;
        let documents = env.create_database(&mut wtxn, Some("documents"))?;

        wtxn.commit()?;

        Ok(Self {
            env,
            indexes_catalog,
            collections_catalog,
            index_entries,
            documents,
        })
    }
}

impl Datastore for HeedStorageEngine {
    fn transaction(
        &self,
        ts: u64,
    ) -> crate::error::Result<impl crate::traits::DatastoreTransaction + '_> {
        let tx = self.env.read_txn()?;

        Ok(HeedDatastoreTransaction::new(self, tx, ts))
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
        Ok(self.env.force_sync()?)
    }

    fn set_ts(&self, ts: u64) -> crate::error::Result<()> {
        todo!()
    }

    fn get_ts(&self) -> crate::error::Result<u64> {
        todo!()
    }
}
