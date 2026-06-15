use std::{fs, sync::Arc};

use heed3::Database;
use heed3::Env;
use heed3::EnvFlags;
use heed3::EnvOpenOptions;
use heed3::types::Bytes;

use crate::{implementations::heed::transaction::HeedDatastoreTransaction, traits::Datastore};

const METADATA_TS_KEY: &[u8] = b"ts";
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
    /// Metadata database (e.g. for global timestamp counter)
    pub(super) metadata: Database<Bytes, Bytes>,
}

impl HeedStorageEngine {
    const DEFAULT_MAP_SIZE: usize = 1024 * 1024 * 1024;
    const DEFAULT_MAX_DBS: u32 = 16;

    pub fn open(path: &str) -> crate::error::Result<Self> {
        fs::create_dir_all(path)?;

        let mut options = EnvOpenOptions::new();
        options
            .map_size(Self::DEFAULT_MAP_SIZE)
            .max_dbs(Self::DEFAULT_MAX_DBS);

        // Favor cheap commits; callers should use `flush`/`force_sync` for durability checkpoints.
        unsafe {
            options.flags(
                EnvFlags::NO_SYNC
                    | EnvFlags::NO_META_SYNC
                    | EnvFlags::WRITE_MAP
                    | EnvFlags::MAP_ASYNC,
            );
        }

        let env = unsafe { options.open(path)? };

        Self::new(Arc::new(env))
    }

    pub fn new(env: Arc<Env>) -> crate::error::Result<Self> {
        let mut wtxn = env.write_txn()?;

        let indexes_catalog = env.create_database(&mut wtxn, Some("indexes_catalog"))?;
        let collections_catalog = env.create_database(&mut wtxn, Some("collections_catalog"))?;
        let index_entries = env.create_database(&mut wtxn, Some("index_entries"))?;
        let documents = env.create_database(&mut wtxn, Some("documents"))?;
        let metadata = env.create_database(&mut wtxn, Some("metadata"))?;

        wtxn.commit()?;

        Ok(Self {
            env,
            indexes_catalog,
            collections_catalog,
            index_entries,
            documents,
            metadata,
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

    fn flush(&self) -> crate::error::Result<()> {
        Ok(self.env.force_sync()?)
    }

    fn set_ts(&self, ts: u64) -> crate::error::Result<()> {
        let mut tx = self.env.write_txn()?;
        self.metadata
            .put(&mut tx, METADATA_TS_KEY, &ts.to_le_bytes())?;
        tx.commit()?;
        Ok(())
    }

    fn get_ts(&self) -> crate::error::Result<u64> {
        let tx = self.env.read_txn()?;
        let ts = self
            .metadata
            .get(&tx, METADATA_TS_KEY)?
            .and_then(|bytes| bytes.try_into().ok())
            .map(u64::from_le_bytes)
            .unwrap_or(0);

        Ok(ts)
    }
}
