use std::sync::atomic::{AtomicBool, AtomicU64};
use std::{collections::HashMap, fs, io, sync::Arc};

use heed3::Database;
use heed3::Env;
use heed3::EnvFlags;
use heed3::EnvOpenOptions;
use heed3::RwTxn;
use heed3::types::{Bytes, Str};

use crate::implementations::heed::encoding::decode_u64;
use crate::{
    implementations::heed::transaction::HeedDatastoreTransaction,
    traits::Datastore,
    types::{CollectionId, DocumentId, IndexId},
};

const METADATA_TS_KEY: &[u8] = &[0x00];
const METADATA_INDEX_CHAIN_ID_KEY: &[u8] = &[0x01];
const METADATA_COLLECTION_ID_KEY: &[u8] = &[0x02];
const METADATA_INDEX_ID_KEY: &[u8] = &[0x03];
const DOCUMENT_TOMBSTONE: &[u8] = &[0x00];
const DOCUMENT_VALUE_PREFIX: u8 = 0x01;
const MAX_INDEX_SEGMENT_SIZE: usize = 478;
const INDEX_SEGMENT_MIDDLE: u8 = 0x00;
const INDEX_SEGMENT_LAST: u8 = 0x01;
const INDEX_SEGMENT_FIRST: u8 = 0x02;
const INDEX_SEGMENT_SHORT: u8 = 0x03;

#[derive(Clone)]
pub struct HeedStorageEngine {
    /// Heed environment
    pub(super) env: Arc<Env>,
    /// Catalog of index names to index ids + config
    pub(super) indexes_catalog: Database<Bytes, Bytes>,
    /// Catalog of collection names to collection ids + metadata
    pub(super) collections_catalog: Database<Str, Bytes>,
    /// Each index entry for all docs
    pub(super) index_entries: Database<Bytes, Bytes>,
    /// Documents themselves
    pub(super) documents: Database<Bytes, Bytes>,
    /// Metadata database (e.g. for global timestamp counter)
    pub(super) metadata: Database<Bytes, Bytes>,

    // counters
    pub(super) ts: Arc<AtomicU64>,
    pub(crate) ts_changed: Arc<AtomicBool>,
    pub(super) collection_id_counter: Arc<AtomicU64>,
    pub(super) collection_id_counter_changed: Arc<AtomicBool>,
    pub(super) index_id_counter: Arc<AtomicU64>,
    pub(super) index_id_counter_changed: Arc<AtomicBool>,
    pub(super) index_chain_id_counter: Arc<AtomicU64>,
    pub(super) index_chain_id_counter_changed: Arc<AtomicBool>,
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

        // load counters

        wtxn.commit()?;

        Ok(Self {
            env,
            indexes_catalog,
            collections_catalog,
            index_entries,
            documents,
            metadata,
            ts: Arc::new(AtomicU64::new(0)),
            ts_changed: Arc::new(AtomicBool::new(false)),
            collection_id_counter: Arc::new(AtomicU64::new(0)),
            collection_id_counter_changed: Arc::new(AtomicBool::new(false)),
            index_id_counter: Arc::new(AtomicU64::new(0)),
            index_id_counter_changed: Arc::new(AtomicBool::new(false)),
            index_chain_id_counter: Arc::new(AtomicU64::new(0)),
            index_chain_id_counter_changed: Arc::new(AtomicBool::new(false)),
        })
    }

    fn load_counters(&mut self, tx: &RwTxn) -> crate::error::Result<()> {
        let ts = self
            .metadata
            .get(tx, METADATA_TS_KEY)?
            .map(decode_u64)
            .transpose()?
            .unwrap_or(0);
        self.ts.store(ts, std::sync::atomic::Ordering::SeqCst);

        let collection_id_counter = self
            .metadata
            .get(tx, METADATA_COLLECTION_ID_KEY)?
            .map(decode_u64)
            .transpose()?
            .unwrap_or(0);
        self.collection_id_counter
            .store(collection_id_counter, std::sync::atomic::Ordering::SeqCst);

        let index_id_counter = self
            .metadata
            .get(tx, METADATA_INDEX_ID_KEY)?
            .map(decode_u64)
            .transpose()?
            .unwrap_or(0);
        self.index_id_counter
            .store(index_id_counter, std::sync::atomic::Ordering::SeqCst);

        let index_chain_id_counter = self
            .metadata
            .get(tx, METADATA_INDEX_CHAIN_ID_KEY)?
            .map(decode_u64)
            .transpose()?
            .unwrap_or(0);
        self.index_chain_id_counter
            .store(index_chain_id_counter, std::sync::atomic::Ordering::SeqCst);

        self.ts_changed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.collection_id_counter_changed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.index_id_counter_changed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.index_chain_id_counter_changed
            .store(false, std::sync::atomic::Ordering::SeqCst);

        Ok(())
    }

    fn save_counters(&mut self, tx: &mut RwTxn) -> crate::error::Result<()> {
        if self.ts_changed.load(std::sync::atomic::Ordering::SeqCst) {
            let ts = self.ts.load(std::sync::atomic::Ordering::SeqCst);
            self.metadata.put(tx, METADATA_TS_KEY, &ts.to_le_bytes())?;
            self.ts_changed
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }

        if self
            .collection_id_counter_changed
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            let collection_id_counter = self
                .collection_id_counter
                .load(std::sync::atomic::Ordering::SeqCst);
            self.metadata.put(
                tx,
                METADATA_COLLECTION_ID_KEY,
                &collection_id_counter.to_be_bytes(),
            )?;
            self.collection_id_counter_changed
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }

        if self
            .index_id_counter_changed
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            let index_id_counter = self
                .index_id_counter
                .load(std::sync::atomic::Ordering::SeqCst);
            self.metadata
                .put(tx, METADATA_INDEX_ID_KEY, &index_id_counter.to_be_bytes())?;
            self.index_id_counter_changed
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }

        if self
            .index_chain_id_counter_changed
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            let index_chain_id_counter = self
                .index_chain_id_counter
                .load(std::sync::atomic::Ordering::SeqCst);
            self.metadata.put(
                tx,
                METADATA_INDEX_CHAIN_ID_KEY,
                &index_chain_id_counter.to_be_bytes(),
            )?;
            self.index_chain_id_counter_changed
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }

        Ok(())
    }

    fn allocate_collection_id(&self, tx: &mut RwTxn) -> crate::error::Result<CollectionId> {
        let id = self
            .collection_id_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        self.collection_id_counter_changed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(id as CollectionId)
    }

    fn allocate_index_id(&self, tx: &mut RwTxn) -> crate::error::Result<IndexId> {
        let id = self
            .index_id_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        self.index_id_counter_changed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(id as IndexId)
    }

    fn allocate_chain_id(&self, tx: &mut RwTxn) -> crate::error::Result<u64> {
        let id = self
            .index_chain_id_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        self.index_chain_id_counter_changed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(id)
    }

    /// Creates new collection or returns existing collection id if collection with the same name already exists
    fn create_collection(
        &self,
        tx: &mut RwTxn,
        collection_name: &str,
        metadata: Option<&[u8]>,
    ) -> crate::error::Result<CollectionId> {
        todo!()
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
        let mut wtxn = self.env.write_txn()?;

        let mut new_collections = HashMap::<CollectionId, CollectionId>::new();

        for (new_collection_tmp_id, new_collection_name) in batch.new_collections {
            let new_collection_id =
                self.create_collection(&mut wtxn, &new_collection_name, None)?;
            new_collections.insert(new_collection_tmp_id, new_collection_id);
        }

        for (collection_id, data) in batch.collections {
            let collection_id: CollectionId = if collection_id < 0 {
                new_collections
                    .get(&(collection_id as CollectionId))
                    .copied()
                    .ok_or_else(|| {
                        crate::error::Error::implementation(format!(
                            "invalid collection id {} in batch",
                            collection_id
                        ))
                    })?
            } else {
                collection_id as CollectionId
            };
        }

        todo!()
    }

    fn flush(&self) -> crate::error::Result<()> {
        Ok(self.env.force_sync()?)
    }

    fn set_ts(&self, ts: u64) -> crate::error::Result<()> {
        self.ts.store(ts, std::sync::atomic::Ordering::SeqCst);
        self.ts_changed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn get_ts(&self) -> crate::error::Result<u64> {
        Ok(self.ts.load(std::sync::atomic::Ordering::SeqCst))
    }
}
