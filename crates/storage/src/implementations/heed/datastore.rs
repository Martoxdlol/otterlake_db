use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::{collections::HashMap, fs, sync::Arc};

use heed3::types::Bytes;
use heed3::{Database, DatabaseFlags, Env, EnvFlags, EnvOpenOptions, RwTxn, WithoutTls};

use crate::implementations::heed::encoding::{
    DOCUMENT_TOMBSTONE, ROOT_INDEX_CHAIN_ID, decode_collection_catalog_key,
    decode_collection_catalog_value, decode_i64, decode_index_catalog_value, decode_u64,
    encode_collection_catalog_key, encode_collection_catalog_value, encode_document_key,
    encode_document_value, encode_index_catalog_key, encode_index_catalog_value,
    encode_index_document_id, encode_index_node_key, encode_vacuum_target_key, index_node_prefix,
    split_index_key,
};
use crate::{
    error::Error,
    implementations::heed::transaction::HeedDatastoreTransaction,
    traits::Datastore,
    types::{CollectionId, DocumentId, IndexId, Value},
    write_set::{CollectionWriteSet, DocumentWrite, IndexWrite},
};

const METADATA_TS_KEY: &[u8] = &[0x00];
const METADATA_INDEX_CHAIN_ID_KEY: &[u8] = &[0x01];
const METADATA_COLLECTION_ID_KEY: &[u8] = &[0x02];
const METADATA_INDEX_ID_KEY: &[u8] = &[0x03];
const METADATA_VISIBLE_TS_KEY: &[u8] = &[0x04];

#[derive(Clone)]
pub struct HeedStorageEngine {
    /// Heed environment
    pub(super) env: Arc<Env<WithoutTls>>,
    /// Catalog of index names to index ids + config
    pub(super) indexes_catalog: Database<Bytes, Bytes>,
    /// Catalog of collection ids to timestamped names + metadata.
    pub(super) collections_catalog: Database<Bytes, Bytes>,
    /// Secondary lookup from collection name to collection id.
    pub(super) collection_ids_by_name: Database<Bytes, Bytes>,
    /// Index trie edges: `[index id][parent chain id][segment] -> [child chain id]`.
    pub(super) index_edges: Database<Bytes, Bytes>,
    /// Index trie leaves: `[index id][parent chain id][segment] -> duplicate sorted document ids`.
    pub(super) index_leaves: Database<Bytes, Bytes>,
    /// Timestamped document versions.
    pub(super) documents: Database<Bytes, Bytes>,
    /// Metadata database (e.g. global timestamp and id counters).
    pub(super) metadata: Database<Bytes, Bytes>,
    /// Documents with previous versions to be considered by vacuum.
    pub(super) vacuum_targets: Database<Bytes, Bytes>,

    pub(super) ts: Arc<AtomicU64>,
    pub(super) visible_ts: Arc<AtomicU64>,
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

        let mut options = EnvOpenOptions::new().read_txn_without_tls();
        options
            .map_size(Self::DEFAULT_MAP_SIZE)
            .max_dbs(Self::DEFAULT_MAX_DBS);

        // Favor cheap commits; callers should use `flush` for durability checkpoints.
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

    pub fn new(env: Arc<Env<WithoutTls>>) -> crate::error::Result<Self> {
        let mut wtxn = env.write_txn()?;

        let indexes_catalog = env.create_database(&mut wtxn, Some("indexes_catalog"))?;
        let collections_catalog = env.create_database(&mut wtxn, Some("collections_catalog"))?;
        let collection_ids_by_name =
            env.create_database(&mut wtxn, Some("collection_ids_by_name"))?;
        let index_edges = env.create_database(&mut wtxn, Some("index_edges"))?;
        let index_leaves = env
            .database_options()
            .types::<Bytes, Bytes>()
            .flags(DatabaseFlags::DUP_SORT)
            .name("index_leaves")
            .create(&mut wtxn)?;
        let documents = env.create_database(&mut wtxn, Some("documents"))?;
        let metadata = env.create_database(&mut wtxn, Some("metadata"))?;
        let vacuum_targets = env.create_database(&mut wtxn, Some("vacuum_targets"))?;

        let engine = Self {
            env: Arc::clone(&env),
            indexes_catalog,
            collections_catalog,
            collection_ids_by_name,
            index_edges,
            index_leaves,
            documents,
            metadata,
            vacuum_targets,
            ts: Arc::new(AtomicU64::new(0)),
            visible_ts: Arc::new(AtomicU64::new(0)),
            collection_id_counter: Arc::new(AtomicU64::new(0)),
            collection_id_counter_changed: Arc::new(AtomicBool::new(false)),
            index_id_counter: Arc::new(AtomicU64::new(0)),
            index_id_counter_changed: Arc::new(AtomicBool::new(false)),
            index_chain_id_counter: Arc::new(AtomicU64::new(0)),
            index_chain_id_counter_changed: Arc::new(AtomicBool::new(false)),
        };

        engine.load_counters(&wtxn)?;
        wtxn.commit()?;

        Ok(engine)
    }

    fn load_counters(&self, tx: &RwTxn) -> crate::error::Result<()> {
        self.ts.store(
            self.metadata
                .get(tx, METADATA_TS_KEY)?
                .map(decode_u64)
                .transpose()?
                .unwrap_or(0),
            Ordering::SeqCst,
        );

        self.visible_ts.store(
            self.metadata
                .get(tx, METADATA_VISIBLE_TS_KEY)?
                .map(decode_u64)
                .transpose()?
                .unwrap_or(0),
            Ordering::SeqCst,
        );

        self.collection_id_counter.store(
            self.metadata
                .get(tx, METADATA_COLLECTION_ID_KEY)?
                .map(decode_u64)
                .transpose()?
                .unwrap_or(0),
            Ordering::SeqCst,
        );

        self.index_id_counter.store(
            self.metadata
                .get(tx, METADATA_INDEX_ID_KEY)?
                .map(decode_u64)
                .transpose()?
                .unwrap_or(0),
            Ordering::SeqCst,
        );

        self.index_chain_id_counter.store(
            self.metadata
                .get(tx, METADATA_INDEX_CHAIN_ID_KEY)?
                .map(decode_u64)
                .transpose()?
                .unwrap_or(0),
            Ordering::SeqCst,
        );

        self.collection_id_counter_changed
            .store(false, Ordering::SeqCst);
        self.index_id_counter_changed.store(false, Ordering::SeqCst);
        self.index_chain_id_counter_changed
            .store(false, Ordering::SeqCst);

        Ok(())
    }

    fn save_counters(&self, tx: &mut RwTxn) -> crate::error::Result<()> {
        if self
            .collection_id_counter_changed
            .swap(false, Ordering::SeqCst)
        {
            let collection_id_counter = self.collection_id_counter.load(Ordering::SeqCst);
            self.metadata.put(
                tx,
                METADATA_COLLECTION_ID_KEY,
                &collection_id_counter.to_be_bytes(),
            )?;
        }

        if self.index_id_counter_changed.swap(false, Ordering::SeqCst) {
            let index_id_counter = self.index_id_counter.load(Ordering::SeqCst);
            self.metadata
                .put(tx, METADATA_INDEX_ID_KEY, &index_id_counter.to_be_bytes())?;
        }

        if self
            .index_chain_id_counter_changed
            .swap(false, Ordering::SeqCst)
        {
            let index_chain_id_counter = self.index_chain_id_counter.load(Ordering::SeqCst);
            self.metadata.put(
                tx,
                METADATA_INDEX_CHAIN_ID_KEY,
                &index_chain_id_counter.to_be_bytes(),
            )?;
        }

        Ok(())
    }

    fn allocate_collection_id(&self) -> CollectionId {
        let id = self.collection_id_counter.fetch_add(1, Ordering::SeqCst) + 1;
        self.collection_id_counter_changed
            .store(true, Ordering::SeqCst);
        id as CollectionId
    }

    fn allocate_index_id(&self) -> IndexId {
        let id = self.index_id_counter.fetch_add(1, Ordering::SeqCst) + 1;
        self.index_id_counter_changed.store(true, Ordering::SeqCst);
        id as IndexId
    }

    fn allocate_chain_id(&self) -> u64 {
        let id = self.index_chain_id_counter.fetch_add(1, Ordering::SeqCst) + 1;
        self.index_chain_id_counter_changed
            .store(true, Ordering::SeqCst);
        id
    }

    fn set_ts(&self, tx: &mut RwTxn, ts: u64) -> crate::error::Result<()> {
        loop {
            let current = self.ts.load(Ordering::SeqCst);
            if ts < current {
                return Err(Error::implementation(format!(
                    "timestamp {ts} is lower than current timestamp {current}"
                )));
            }

            if ts == current {
                return Ok(());
            }

            if self
                .ts
                .compare_exchange(current, ts, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                self.metadata.put(tx, METADATA_TS_KEY, &ts.to_be_bytes())?;
                return Ok(());
            }
        }
    }

    /// Creates a new collection or returns the existing collection id for the same name.
    fn create_collection(
        &self,
        tx: &mut RwTxn,
        collection_name: &str,
        metadata: Option<&[u8]>,
        version: u64,
    ) -> crate::error::Result<CollectionId> {
        if let Some(id) = self.collection_id_by_name(tx, collection_name)? {
            if let Some(metadata) = metadata {
                self.put_collection_catalog_entry(tx, id, collection_name, metadata, version)?;
            }
            return Ok(id);
        }

        let id = self.allocate_collection_id();
        self.put_collection_catalog_entry(
            tx,
            id,
            collection_name,
            metadata.unwrap_or_default(),
            version,
        )?;
        self.put_collection_id_by_name(tx, collection_name, id)?;
        Ok(id)
    }

    fn collection_id_by_name(
        &self,
        tx: &RwTxn,
        collection_name: &str,
    ) -> crate::error::Result<Option<CollectionId>> {
        self.collection_ids_by_name
            .get(tx, collection_name.as_bytes())?
            .map(decode_i64)
            .transpose()
    }

    fn collection_by_id(
        &self,
        tx: &RwTxn,
        collection_id: CollectionId,
        version: u64,
    ) -> crate::error::Result<Option<(String, Value)>> {
        let upper_key = encode_collection_catalog_key(collection_id, version);
        let Some((stored_key, value)) = self
            .collections_catalog
            .get_lower_than_or_equal_to(tx, &upper_key)?
        else {
            return Ok(None);
        };

        let (stored_collection_id, stored_version) = decode_collection_catalog_key(stored_key)?;
        if stored_collection_id != collection_id || stored_version > version {
            return Ok(None);
        }

        decode_collection_catalog_value(value).map(Some)
    }

    fn put_collection_catalog_entry(
        &self,
        tx: &mut RwTxn,
        collection_id: CollectionId,
        collection_name: &str,
        metadata: &[u8],
        version: u64,
    ) -> crate::error::Result<()> {
        let key = encode_collection_catalog_key(collection_id, version);
        let value = encode_collection_catalog_value(collection_name, metadata);
        self.collections_catalog.put(tx, &key, &value)?;
        Ok(())
    }

    fn create_index(
        &self,
        tx: &mut RwTxn,
        collection_id: CollectionId,
        index_name: &str,
        index_config: &[u8],
    ) -> crate::error::Result<IndexId> {
        let key = encode_index_catalog_key(collection_id, index_name);
        if let Some(value) = self.indexes_catalog.get(tx, &key)? {
            let (id, _) = decode_index_catalog_value(value)?;
            return Ok(id);
        }

        let id = self.allocate_index_id();
        let value = encode_index_catalog_value(id, index_config);
        self.indexes_catalog.put(tx, &key, &value)?;
        Ok(id)
    }

    fn update_collection_metadata(
        &self,
        tx: &mut RwTxn,
        collection_id: CollectionId,
        metadata: &[u8],
        version: u64,
    ) -> crate::error::Result<()> {
        let (name, _) = self
            .collection_by_id(tx, collection_id, u64::MAX)?
            .ok_or_else(|| {
                Error::implementation(format!("collection id {collection_id} does not exist"))
            })?;
        self.put_collection_catalog_entry(tx, collection_id, &name, metadata, version)?;
        Ok(())
    }

    fn put_collection_id_by_name(
        &self,
        tx: &mut RwTxn,
        collection_name: &str,
        collection_id: CollectionId,
    ) -> crate::error::Result<()> {
        self.collection_ids_by_name.put(
            tx,
            collection_name.as_bytes(),
            &collection_id.to_be_bytes(),
        )?;
        Ok(())
    }

    fn put_index_entry(
        &self,
        tx: &mut RwTxn,
        index_id: IndexId,
        index_key: &[u8],
        document_id: DocumentId,
    ) -> crate::error::Result<()> {
        let segments = split_index_key(index_key);
        let mut chain_id = ROOT_INDEX_CHAIN_ID;

        for segment in &segments[..segments.len() - 1] {
            chain_id = self.find_or_create_index_edge(tx, index_id, chain_id, segment)?;
        }

        let last_segment = segments[segments.len() - 1];
        let key = encode_index_node_key(index_id, chain_id, last_segment);
        let document_id = encode_index_document_id(document_id);
        self.index_leaves
            .delete_one_duplicate(tx, &key, &document_id)?;
        self.index_leaves.put(tx, &key, &document_id)?;
        Ok(())
    }

    fn delete_index_entry(
        &self,
        tx: &mut RwTxn,
        index_id: IndexId,
        index_key: &[u8],
        document_id: DocumentId,
    ) -> crate::error::Result<()> {
        let segments = split_index_key(index_key);
        let mut chain_id = ROOT_INDEX_CHAIN_ID;
        let mut path = Vec::with_capacity(segments.len().saturating_sub(1));

        for segment in &segments[..segments.len() - 1] {
            let key = encode_index_node_key(index_id, chain_id, segment);
            let Some(value) = self.index_edges.get(tx, &key)? else {
                return Ok(());
            };
            let child_chain_id = decode_u64(value)?;
            path.push(IndexPathEdge {
                parent_chain_id: chain_id,
                segment: (*segment).to_vec(),
                child_chain_id,
            });
            chain_id = child_chain_id;
        }

        let last_segment = segments[segments.len() - 1];
        let key = encode_index_node_key(index_id, chain_id, last_segment);
        let document_id = encode_index_document_id(document_id);
        self.index_leaves
            .delete_one_duplicate(tx, &key, &document_id)?;

        for edge in path.into_iter().rev() {
            if self.index_node_has_entries(tx, index_id, edge.child_chain_id)? {
                break;
            }

            let key = encode_index_node_key(index_id, edge.parent_chain_id, &edge.segment);
            self.index_edges.delete(tx, &key)?;
        }

        Ok(())
    }

    fn find_or_create_index_edge(
        &self,
        tx: &mut RwTxn,
        index_id: IndexId,
        parent_chain_id: u64,
        segment: &[u8],
    ) -> crate::error::Result<u64> {
        let key = encode_index_node_key(index_id, parent_chain_id, segment);
        if let Some(value) = self.index_edges.get(tx, &key)? {
            return decode_u64(value);
        }

        let child_chain_id = self.allocate_chain_id();
        self.index_edges
            .put(tx, &key, &child_chain_id.to_be_bytes())?;
        Ok(child_chain_id)
    }

    fn index_node_has_entries(
        &self,
        tx: &RwTxn,
        index_id: IndexId,
        chain_id: u64,
    ) -> crate::error::Result<bool> {
        let prefix = index_node_prefix(index_id, chain_id);
        if self.index_leaves.prefix_iter(tx, &prefix)?.next().is_some() {
            return Ok(true);
        }

        Ok(self.index_edges.prefix_iter(tx, &prefix)?.next().is_some())
    }
}

impl Datastore for HeedStorageEngine {
    type Transaction<'a> = HeedDatastoreTransaction<'a>;

    fn transaction(&self, ts: u64) -> crate::error::Result<Self::Transaction<'_>> {
        let tx = self.env.read_txn()?;
        Ok(HeedDatastoreTransaction::new(self, tx, ts))
    }

    fn put(&self, batch: crate::write_set::WriteSet) -> crate::error::Result<()> {
        let mut wtxn = self.env.write_txn()?;
        let mut new_collections = HashMap::<CollectionId, CollectionId>::new();

        for (new_collection_tmp_id, new_collection_name) in batch.new_collections {
            let new_collection_id =
                self.create_collection(&mut wtxn, &new_collection_name, None, batch.ts)?;
            new_collections.insert(new_collection_tmp_id, new_collection_id);
        }

        let mut new_indexes = HashMap::<IndexId, IndexId>::new();

        for (collection_id, indexes) in batch.new_indexes {
            let collection_id = resolve_collection_id(collection_id, &new_collections)?;
            for (new_index_tmp_id, index) in indexes {
                let new_index_id =
                    self.create_index(&mut wtxn, collection_id, &index.name, &index.metadata)?;
                new_indexes.insert(new_index_tmp_id, new_index_id);
            }
        }

        for (collection_id, data) in batch.collections {
            let collection_id = resolve_collection_id(collection_id, &new_collections)?;
            let CollectionWriteSet {
                documents,
                index_entries,
                metadata,
            } = data;

            for (document_id, write) in documents {
                let key = encode_document_key(collection_id, document_id, batch.ts);
                match write {
                    DocumentWrite::Put(data) => {
                        let value = encode_document_value(&data);
                        self.documents.put(&mut wtxn, &key, &value)?;
                    }
                    DocumentWrite::Deleted => {
                        self.documents.put(&mut wtxn, &key, DOCUMENT_TOMBSTONE)?;
                    }
                }
                let vacuum_key = encode_vacuum_target_key(collection_id, document_id);
                self.vacuum_targets.put(&mut wtxn, &vacuum_key, &[])?;
            }

            for (index_id, entries) in index_entries {
                let index_id = resolve_index_id(index_id, &new_indexes)?;
                for (position, write) in entries {
                    match write {
                        IndexWrite::Put => self.put_index_entry(
                            &mut wtxn,
                            index_id,
                            &position.value,
                            position.document_id,
                        )?,
                        IndexWrite::Deleted => self.delete_index_entry(
                            &mut wtxn,
                            index_id,
                            &position.value,
                            position.document_id,
                        )?,
                    }
                }
            }

            if let Some(metadata) = metadata {
                self.update_collection_metadata(&mut wtxn, collection_id, &metadata, batch.ts)?;
            }
        }

        self.set_ts(&mut wtxn, batch.ts)?;
        self.save_counters(&mut wtxn)?;
        wtxn.commit()?;

        Ok(())
    }

    fn flush(&self) -> crate::error::Result<()> {
        Ok(self.env.force_sync()?)
    }

    fn set_ts(&self, ts: u64) -> crate::error::Result<()> {
        let mut wtxn = self.env.write_txn()?;
        self.set_ts(&mut wtxn, ts)?;
        wtxn.commit()?;
        Ok(())
    }

    fn get_ts(&self) -> crate::error::Result<u64> {
        Ok(self.ts.load(Ordering::SeqCst))
    }

    fn set_visible_ts(&self, ts: u64) -> crate::error::Result<()> {
        let mut wtxn = self.env.write_txn()?;
        self.visible_ts.store(ts, Ordering::SeqCst);
        self.metadata
            .put(&mut wtxn, METADATA_VISIBLE_TS_KEY, &ts.to_be_bytes())?;
        wtxn.commit()?;
        Ok(())
    }

    fn get_visible_ts(&self) -> crate::error::Result<u64> {
        Ok(self.visible_ts.load(Ordering::SeqCst))
    }
}

struct IndexPathEdge {
    parent_chain_id: u64,
    segment: Value,
    child_chain_id: u64,
}

fn resolve_collection_id(
    collection_id: CollectionId,
    new_collections: &HashMap<CollectionId, CollectionId>,
) -> crate::error::Result<CollectionId> {
    if collection_id >= 0 {
        return Ok(collection_id);
    }

    new_collections.get(&collection_id).copied().ok_or_else(|| {
        Error::implementation(format!("invalid collection id {collection_id} in batch"))
    })
}

fn resolve_index_id(
    index_id: IndexId,
    new_indexes: &HashMap<IndexId, IndexId>,
) -> crate::error::Result<IndexId> {
    if index_id >= 0 {
        return Ok(index_id);
    }

    new_indexes
        .get(&index_id)
        .copied()
        .ok_or_else(|| Error::implementation(format!("invalid index id {index_id} in batch")))
}
