use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashSet},
    iter::Peekable,
    ops::Bound,
    vec::IntoIter,
};

use heed3::types::Bytes;

use crate::{
    implementations::heed::{
        cursors::{
            HeedCollectionCatalogCursor, HeedDocumentCursor, HeedIndexCatalogCursor,
            HeedIndexCursor, HeedIndexCursorOptions, HeedRawCursor, IndexCandidate,
        },
        datastore::HeedStorageEngine,
        encoding::{
            decode_collection_catalog_key, decode_collection_catalog_value, decode_document_key,
            decode_document_value, decode_i64, encode_collection_catalog_key,
            encode_collection_prefix, encode_document_key, encode_index_catalog_key,
        },
    },
    traits::{DatastoreCursor, DatastoreTransaction},
    types::{
        CollectionCatalogEntry, CollectionId, CursorStart, Direction, DocumentEntry, DocumentId,
        IndexCatalogEntry, IndexEntry, IndexId, IndexPosition, Value,
    },
    write_set::{DocumentWrite, IndexWrite, WriteSet},
};

pub struct HeedDatastoreTransaction<'env> {
    engine: &'env HeedStorageEngine,
    tx: heed3::RoTxn<'env, heed3::WithoutTls>,
    ts: u64,
    writes: WriteSet,
}

impl<'env> HeedDatastoreTransaction<'env> {
    pub fn new(
        engine: &'env HeedStorageEngine,
        tx: heed3::RoTxn<'env, heed3::WithoutTls>,
        ts: u64,
    ) -> Self {
        Self {
            engine,
            tx,
            ts,
            writes: WriteSet {
                ts,
                ..WriteSet::default()
            },
        }
    }

    fn raw_cursor(
        &self,
        database: heed3::Database<heed3::types::Bytes, heed3::types::Bytes>,
        bounds: (Bound<Vec<u8>>, Bound<Vec<u8>>),
        direction: Direction,
    ) -> crate::error::Result<HeedRawCursor<'_>> {
        let borrowed_bounds = (borrow_bound(&bounds.0), borrow_bound(&bounds.1));
        Ok(match direction {
            Direction::Forward => {
                HeedRawCursor::Forward(database.range(&self.tx, &borrowed_bounds)?)
            }
            Direction::Reverse => {
                HeedRawCursor::Reverse(database.rev_range(&self.tx, &borrowed_bounds)?)
            }
        })
    }
}

impl DatastoreTransaction for HeedDatastoreTransaction<'_> {
    fn collection(&self, name: &str) -> crate::error::Result<Option<CollectionId>> {
        if let Some((&collection_id, _)) = self
            .writes
            .new_collections
            .iter()
            .find(|(_, collection_name)| collection_name.as_str() == name)
        {
            return Ok(Some(collection_id));
        }

        let Some(value) = self
            .engine
            .collection_ids_by_name
            .get(&self.tx, name.as_bytes())?
        else {
            return Ok(None);
        };

        let collection_id = decode_i64(value)?;
        let upper_key = encode_collection_catalog_key(collection_id, self.ts);
        let Some((stored_key, value)) = self
            .engine
            .collections_catalog
            .get_lower_than_or_equal_to(&self.tx, &upper_key)?
        else {
            return Ok(None);
        };

        let (stored_collection_id, stored_version) = decode_collection_catalog_key(stored_key)?;
        if stored_collection_id != collection_id || stored_version > self.ts {
            return Ok(None);
        }

        decode_collection_catalog_value(value)?;
        Ok(Some(collection_id))
    }

    fn get(
        &self,
        collection: CollectionId,
        key: DocumentId,
    ) -> crate::error::Result<Option<Value>> {
        visible_document(
            &self.writes,
            &self.tx,
            self.engine.documents,
            self.ts,
            collection,
            key,
        )
    }

    fn get_cursor(
        &self,
        collection: CollectionId,
        start: CursorStart<DocumentId>,
        direction: Direction,
    ) -> crate::error::Result<impl DatastoreCursor<Item = DocumentEntry>> {
        let (overlay, touched) =
            overlay_document_entries(&self.writes, collection, &start, direction);
        let bounds = document_cursor_bounds(collection, start, direction);
        let raw = self.raw_cursor(self.engine.documents, bounds, direction)?;
        let base =
            HeedDocumentCursor::new(raw, collection, self.ts, direction == Direction::Reverse);
        Ok(MergedDocumentCursor::new(base, overlay, touched, direction))
    }

    fn get_index_cursor(
        &self,
        collection: CollectionId,
        index: IndexId,
        start: CursorStart<IndexPosition>,
        direction: Direction,
    ) -> crate::error::Result<impl DatastoreCursor<Item = IndexEntry>> {
        let (overlay, touched) = overlay_index_entries(OverlayIndexEntriesOptions {
            writes: &self.writes,
            tx: &self.tx,
            documents: self.engine.documents,
            ts: self.ts,
            collection,
            index,
            start: &start,
            direction,
        })?;
        let base = HeedIndexCursor::new(HeedIndexCursorOptions {
            tx: &self.tx,
            collection,
            index_id: index,
            index_edges: self.engine.index_edges,
            index_leaves: self.engine.index_leaves,
            documents: self.engine.documents,
            ts: self.ts,
            start,
            direction,
        })?;
        Ok(MergedIndexCursor::new(MergedIndexCursorOptions {
            base,
            overlay,
            touched,
            writes: &self.writes,
            tx: &self.tx,
            documents: self.engine.documents,
            ts: self.ts,
            collection,
            direction,
        }))
    }

    fn get_collections_catalog_cursor(
        &self,
        start: CursorStart<String>,
        direction: Direction,
    ) -> crate::error::Result<impl DatastoreCursor<Item = CollectionCatalogEntry>> {
        let (overlay, touched_ids, touched_names) =
            overlay_collection_entries(self, &start, direction)?;
        let bounds = collection_catalog_cursor_bounds(start, direction);
        let raw = self.raw_cursor(self.engine.collection_ids_by_name, bounds, direction)?;
        let base = HeedCollectionCatalogCursor::new(
            &self.tx,
            raw,
            self.engine.collections_catalog,
            self.ts,
        );
        Ok(MergedCollectionCatalogCursor::new(
            base,
            overlay,
            touched_ids,
            touched_names,
            direction,
        ))
    }

    fn get_indexes_catalog_cursor(
        &self,
        collection: CollectionId,
        start: CursorStart<String>,
        direction: Direction,
    ) -> crate::error::Result<impl DatastoreCursor<Item = IndexCatalogEntry>> {
        let (overlay, touched_names) =
            overlay_index_catalog_entries(&self.writes, collection, &start, direction);
        let bounds = index_catalog_cursor_bounds(collection, start, direction);
        let raw = self.raw_cursor(self.engine.indexes_catalog, bounds, direction)?;
        let base = HeedIndexCatalogCursor::new(raw, collection);
        Ok(MergedIndexCatalogCursor::new(
            base,
            overlay,
            touched_names,
            direction,
        ))
    }

    fn put(
        &mut self,
        collection_id: CollectionId,
        key: DocumentId,
        value: Value,
    ) -> crate::error::Result<()> {
        self.writes.put(collection_id, key, value);
        Ok(())
    }

    fn put_many(
        &mut self,
        collection_id: CollectionId,
        documents: Vec<(DocumentId, Value)>,
    ) -> crate::error::Result<()> {
        self.writes.put_many(collection_id, documents);
        Ok(())
    }

    fn put_index_entry(
        &mut self,
        collection_id: CollectionId,
        index_id: IndexId,
        key: Value,
        document_id: DocumentId,
    ) -> crate::error::Result<()> {
        self.writes
            .put_index_entry(collection_id, index_id, key, document_id);
        Ok(())
    }

    fn put_index_entries(
        &mut self,
        collection_id: CollectionId,
        entries: Vec<(IndexId, Value, DocumentId)>,
    ) -> crate::error::Result<()> {
        self.writes.put_index_entries(collection_id, entries);
        Ok(())
    }

    fn delete(&mut self, collection_id: CollectionId, key: DocumentId) -> crate::error::Result<()> {
        self.writes.delete(collection_id, key);
        Ok(())
    }

    fn delete_many(
        &mut self,
        collection_id: CollectionId,
        keys: Vec<DocumentId>,
    ) -> crate::error::Result<()> {
        self.writes.delete_many(collection_id, keys);
        Ok(())
    }

    fn delete_index_entry(
        &mut self,
        collection_id: CollectionId,
        index_id: IndexId,
        key: Value,
        document_id: DocumentId,
    ) -> crate::error::Result<()> {
        self.writes
            .delete_index_entry(collection_id, index_id, key, document_id);
        Ok(())
    }

    fn delete_index_entries(
        &mut self,
        collection_id: CollectionId,
        entries: Vec<(IndexId, Value, DocumentId)>,
    ) -> crate::error::Result<()> {
        self.writes.delete_index_entries(collection_id, entries);
        Ok(())
    }

    fn new_collection(
        &mut self,
        collection_id: CollectionId,
        name: String,
    ) -> crate::error::Result<()> {
        self.writes.new_collection(collection_id, name);
        Ok(())
    }

    fn new_index(
        &mut self,
        collection_id: CollectionId,
        index_id: IndexId,
        name: String,
        metadata: Value,
    ) -> crate::error::Result<()> {
        self.writes
            .new_index(collection_id, index_id, name, metadata);
        Ok(())
    }

    fn update_collection_metadata(
        &mut self,
        collection_id: CollectionId,
        metadata: Value,
    ) -> crate::error::Result<()> {
        self.writes
            .update_collection_metadata(collection_id, metadata);
        Ok(())
    }

    fn into_write_set(mut self, ts: u64) -> WriteSet {
        self.writes.ts = ts;
        self.writes
    }
}

fn visible_document(
    writes: &WriteSet,
    tx: &heed3::RoTxn<'_, heed3::WithoutTls>,
    documents: heed3::Database<Bytes, Bytes>,
    ts: u64,
    collection: CollectionId,
    key: DocumentId,
) -> crate::error::Result<Option<Value>> {
    if let Some(write) = writes
        .collections
        .get(&collection)
        .and_then(|collection| collection.documents.get(&key))
    {
        return Ok(match write {
            DocumentWrite::Put(value) => Some(value.clone()),
            DocumentWrite::Deleted => None,
        });
    }

    stored_document(tx, documents, ts, collection, key)
}

fn stored_document(
    tx: &heed3::RoTxn<'_, heed3::WithoutTls>,
    documents: heed3::Database<Bytes, Bytes>,
    ts: u64,
    collection: CollectionId,
    key: DocumentId,
) -> crate::error::Result<Option<Value>> {
    let upper_key = encode_document_key(collection, key, ts);
    let Some((stored_key, value)) = documents.get_lower_than_or_equal_to(tx, &upper_key)? else {
        return Ok(None);
    };

    let (stored_collection, stored_document_id, stored_version) = decode_document_key(stored_key)?;
    if stored_collection != collection || stored_document_id != key || stored_version > ts {
        return Ok(None);
    }

    decode_document_value(value)
}

fn stored_collection_entry(
    tx: &heed3::RoTxn<'_, heed3::WithoutTls>,
    collections_catalog: heed3::Database<Bytes, Bytes>,
    ts: u64,
    collection_id: CollectionId,
) -> crate::error::Result<Option<CollectionCatalogEntry>> {
    let upper_key = encode_collection_catalog_key(collection_id, ts);
    let Some((stored_key, value)) =
        collections_catalog.get_lower_than_or_equal_to(tx, &upper_key)?
    else {
        return Ok(None);
    };

    let (stored_collection_id, stored_version) = decode_collection_catalog_key(stored_key)?;
    if stored_collection_id != collection_id || stored_version > ts {
        return Ok(None);
    }

    let (name, metadata) = decode_collection_catalog_value(value)?;
    Ok(Some(CollectionCatalogEntry {
        id: collection_id,
        name,
        metadata,
    }))
}

fn overlay_document_entries(
    writes: &WriteSet,
    collection: CollectionId,
    start: &CursorStart<DocumentId>,
    direction: Direction,
) -> (Vec<DocumentEntry>, BTreeSet<DocumentId>) {
    let Some(collection_writes) = writes.collections.get(&collection) else {
        return (Vec::new(), BTreeSet::new());
    };

    let touched = collection_writes.documents.keys().copied().collect();
    let mut entries = collection_writes
        .documents
        .iter()
        .filter_map(|(&id, write)| {
            if !accepts_document_start(id, start, direction) {
                return None;
            }

            match write {
                DocumentWrite::Put(value) => Some(DocumentEntry {
                    id,
                    value: value.clone(),
                }),
                DocumentWrite::Deleted => None,
            }
        })
        .collect::<Vec<_>>();

    if direction == Direction::Reverse {
        entries.reverse();
    }

    (entries, touched)
}

struct OverlayIndexEntriesOptions<'txn, 'env> {
    writes: &'txn WriteSet,
    tx: &'txn heed3::RoTxn<'env, heed3::WithoutTls>,
    documents: heed3::Database<Bytes, Bytes>,
    ts: u64,
    collection: CollectionId,
    index: IndexId,
    start: &'txn CursorStart<IndexPosition>,
    direction: Direction,
}

fn overlay_index_entries(
    options: OverlayIndexEntriesOptions<'_, '_>,
) -> crate::error::Result<(Vec<IndexEntry>, BTreeSet<IndexPosition>)> {
    let OverlayIndexEntriesOptions {
        writes,
        tx,
        documents,
        ts,
        collection,
        index,
        start,
        direction,
    } = options;
    let Some(index_writes) = writes
        .collections
        .get(&collection)
        .and_then(|collection| collection.index_entries.get(&index))
    else {
        return Ok((Vec::new(), BTreeSet::new()));
    };

    let touched = index_writes.keys().cloned().collect();
    let mut entries = Vec::new();
    for (position, write) in index_writes {
        if *write != IndexWrite::Put || !accepts_index_start(position, start, direction) {
            continue;
        }

        if let Some(document_value) =
            visible_document(writes, tx, documents, ts, collection, position.document_id)?
        {
            entries.push(IndexEntry {
                value: position.value.clone(),
                document_id: position.document_id,
                document_value,
            });
        }
    }

    if direction == Direction::Reverse {
        entries.reverse();
    }

    Ok((entries, touched))
}

fn overlay_collection_entries(
    transaction: &HeedDatastoreTransaction<'_>,
    start: &CursorStart<String>,
    direction: Direction,
) -> crate::error::Result<(
    Vec<CollectionCatalogEntry>,
    HashSet<CollectionId>,
    HashSet<String>,
)> {
    let mut entries = Vec::new();
    let mut touched_ids = HashSet::new();
    let mut touched_names = HashSet::new();

    for (&collection_id, name) in &transaction.writes.new_collections {
        let metadata = transaction
            .writes
            .collections
            .get(&collection_id)
            .and_then(|collection| collection.metadata.clone())
            .unwrap_or_default();
        touched_ids.insert(collection_id);
        touched_names.insert(name.clone());
        entries.push(CollectionCatalogEntry {
            id: collection_id,
            name: name.clone(),
            metadata,
        });
    }

    for (&collection_id, collection_writes) in &transaction.writes.collections {
        let Some(metadata) = collection_writes.metadata.clone() else {
            continue;
        };
        if transaction
            .writes
            .new_collections
            .contains_key(&collection_id)
        {
            continue;
        }

        let Some(mut entry) = stored_collection_entry(
            &transaction.tx,
            transaction.engine.collections_catalog,
            transaction.ts,
            collection_id,
        )?
        else {
            continue;
        };
        entry.metadata = metadata;
        touched_ids.insert(collection_id);
        touched_names.insert(entry.name.clone());
        entries.push(entry);
    }

    entries.retain(|entry| accepts_string_start(&entry.name, start, direction));
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    if direction == Direction::Reverse {
        entries.reverse();
    }

    Ok((entries, touched_ids, touched_names))
}

fn overlay_index_catalog_entries(
    writes: &WriteSet,
    collection: CollectionId,
    start: &CursorStart<String>,
    direction: Direction,
) -> (Vec<IndexCatalogEntry>, HashSet<String>) {
    let Some(indexes) = writes.new_indexes.get(&collection) else {
        return (Vec::new(), HashSet::new());
    };

    let mut touched_names = HashSet::new();
    let mut entries = indexes
        .iter()
        .map(|(&index_id, index)| {
            touched_names.insert(index.name.clone());
            IndexCatalogEntry {
                collection_id: collection,
                id: index_id,
                name: index.name.clone(),
                metadata: index.metadata.clone(),
            }
        })
        .filter(|entry| accepts_string_start(&entry.name, start, direction))
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| left.name.cmp(&right.name));
    if direction == Direction::Reverse {
        entries.reverse();
    }

    (entries, touched_names)
}

fn accepts_document_start(
    document_id: DocumentId,
    start: &CursorStart<DocumentId>,
    direction: Direction,
) -> bool {
    match (start, direction) {
        (CursorStart::Unbounded, _) => true,
        (CursorStart::Included(start), Direction::Forward) => document_id >= *start,
        (CursorStart::Excluded(start), Direction::Forward) => document_id > *start,
        (CursorStart::Included(start), Direction::Reverse) => document_id <= *start,
        (CursorStart::Excluded(start), Direction::Reverse) => document_id < *start,
    }
}

fn accepts_index_start(
    position: &IndexPosition,
    start: &CursorStart<IndexPosition>,
    direction: Direction,
) -> bool {
    match (start, direction) {
        (CursorStart::Unbounded, _) => true,
        (CursorStart::Included(start), Direction::Forward) => position >= start,
        (CursorStart::Excluded(start), Direction::Forward) => position > start,
        (CursorStart::Included(start), Direction::Reverse) => position <= start,
        (CursorStart::Excluded(start), Direction::Reverse) => position < start,
    }
}

fn accepts_string_start(value: &str, start: &CursorStart<String>, direction: Direction) -> bool {
    match (start, direction) {
        (CursorStart::Unbounded, _) => true,
        (CursorStart::Included(start), Direction::Forward) => value >= start,
        (CursorStart::Excluded(start), Direction::Forward) => value > start,
        (CursorStart::Included(start), Direction::Reverse) => value <= start,
        (CursorStart::Excluded(start), Direction::Reverse) => value < start,
    }
}

struct MergedDocumentCursor<'txn> {
    base: HeedDocumentCursor<'txn>,
    overlay: Peekable<IntoIter<DocumentEntry>>,
    touched: BTreeSet<DocumentId>,
    direction: Direction,
    pending_base: Option<DocumentEntry>,
}

impl<'txn> MergedDocumentCursor<'txn> {
    fn new(
        base: HeedDocumentCursor<'txn>,
        overlay: Vec<DocumentEntry>,
        touched: BTreeSet<DocumentId>,
        direction: Direction,
    ) -> Self {
        Self {
            base,
            overlay: overlay.into_iter().peekable(),
            touched,
            direction,
            pending_base: None,
        }
    }

    fn next_base(&mut self) -> crate::error::Result<Option<DocumentEntry>> {
        loop {
            let next = self
                .pending_base
                .take()
                .map_or_else(|| self.base.next(), |entry| Ok(Some(entry)))?;
            let Some(entry) = next else {
                return Ok(None);
            };
            if !self.touched.contains(&entry.id) {
                return Ok(Some(entry));
            }
        }
    }
}

impl DatastoreCursor for MergedDocumentCursor<'_> {
    type Item = DocumentEntry;

    fn next(&mut self) -> crate::error::Result<Option<Self::Item>> {
        if self.pending_base.is_none() {
            self.pending_base = self.next_base()?;
        }

        match (self.pending_base.as_ref(), self.overlay.peek()) {
            (None, None) => Ok(None),
            (Some(_), None) => Ok(self.pending_base.take()),
            (None, Some(_)) => Ok(self.overlay.next()),
            (Some(base), Some(overlay)) => match compare_order(base.id, overlay.id, self.direction)
            {
                Ordering::Less => Ok(self.pending_base.take()),
                Ordering::Greater => Ok(self.overlay.next()),
                Ordering::Equal => {
                    self.pending_base.take();
                    Ok(self.overlay.next())
                }
            },
        }
    }
}

struct MergedIndexCursor<'txn, 'env> {
    base: HeedIndexCursor<'txn, 'env>,
    overlay: Peekable<IntoIter<IndexEntry>>,
    touched: BTreeSet<IndexPosition>,
    writes: &'txn WriteSet,
    tx: &'txn heed3::RoTxn<'env, heed3::WithoutTls>,
    documents: heed3::Database<Bytes, Bytes>,
    ts: u64,
    collection: CollectionId,
    direction: Direction,
    pending_base: Option<IndexEntry>,
}

struct MergedIndexCursorOptions<'txn, 'env> {
    base: HeedIndexCursor<'txn, 'env>,
    overlay: Vec<IndexEntry>,
    touched: BTreeSet<IndexPosition>,
    writes: &'txn WriteSet,
    tx: &'txn heed3::RoTxn<'env, heed3::WithoutTls>,
    documents: heed3::Database<Bytes, Bytes>,
    ts: u64,
    collection: CollectionId,
    direction: Direction,
}

impl<'txn, 'env> MergedIndexCursor<'txn, 'env> {
    fn new(options: MergedIndexCursorOptions<'txn, 'env>) -> Self {
        Self {
            base: options.base,
            overlay: options.overlay.into_iter().peekable(),
            touched: options.touched,
            writes: options.writes,
            tx: options.tx,
            documents: options.documents,
            ts: options.ts,
            collection: options.collection,
            direction: options.direction,
            pending_base: None,
        }
    }

    fn next_base(&mut self) -> crate::error::Result<Option<IndexEntry>> {
        while let Some(candidate) = self.base.next_candidate()? {
            let position = candidate_position(&candidate);
            if self.touched.contains(&position) {
                continue;
            }

            if let Some(document_value) = visible_document(
                self.writes,
                self.tx,
                self.documents,
                self.ts,
                self.collection,
                candidate.document_id,
            )? {
                return Ok(Some(IndexEntry {
                    value: candidate.value,
                    document_id: candidate.document_id,
                    document_value,
                }));
            }
        }

        Ok(None)
    }
}

impl DatastoreCursor for MergedIndexCursor<'_, '_> {
    type Item = IndexEntry;

    fn next(&mut self) -> crate::error::Result<Option<Self::Item>> {
        if self.pending_base.is_none() {
            self.pending_base = self.next_base()?;
        }

        match (self.pending_base.as_ref(), self.overlay.peek()) {
            (None, None) => Ok(None),
            (Some(_), None) => Ok(self.pending_base.take()),
            (None, Some(_)) => Ok(self.overlay.next()),
            (Some(base), Some(overlay)) => {
                match compare_order(
                    index_position(base),
                    index_position(overlay),
                    self.direction,
                ) {
                    Ordering::Less => Ok(self.pending_base.take()),
                    Ordering::Greater => Ok(self.overlay.next()),
                    Ordering::Equal => {
                        self.pending_base.take();
                        Ok(self.overlay.next())
                    }
                }
            }
        }
    }
}

struct MergedCollectionCatalogCursor<'txn, 'env> {
    base: HeedCollectionCatalogCursor<'txn, 'env>,
    overlay: Peekable<IntoIter<CollectionCatalogEntry>>,
    touched_ids: HashSet<CollectionId>,
    touched_names: HashSet<String>,
    direction: Direction,
    pending_base: Option<CollectionCatalogEntry>,
}

impl<'txn, 'env> MergedCollectionCatalogCursor<'txn, 'env> {
    fn new(
        base: HeedCollectionCatalogCursor<'txn, 'env>,
        overlay: Vec<CollectionCatalogEntry>,
        touched_ids: HashSet<CollectionId>,
        touched_names: HashSet<String>,
        direction: Direction,
    ) -> Self {
        Self {
            base,
            overlay: overlay.into_iter().peekable(),
            touched_ids,
            touched_names,
            direction,
            pending_base: None,
        }
    }

    fn next_base(&mut self) -> crate::error::Result<Option<CollectionCatalogEntry>> {
        loop {
            let next = self
                .pending_base
                .take()
                .map_or_else(|| self.base.next(), |entry| Ok(Some(entry)))?;
            let Some(entry) = next else {
                return Ok(None);
            };
            if !self.touched_ids.contains(&entry.id) && !self.touched_names.contains(&entry.name) {
                return Ok(Some(entry));
            }
        }
    }
}

impl DatastoreCursor for MergedCollectionCatalogCursor<'_, '_> {
    type Item = CollectionCatalogEntry;

    fn next(&mut self) -> crate::error::Result<Option<Self::Item>> {
        if self.pending_base.is_none() {
            self.pending_base = self.next_base()?;
        }

        match (self.pending_base.as_ref(), self.overlay.peek()) {
            (None, None) => Ok(None),
            (Some(_), None) => Ok(self.pending_base.take()),
            (None, Some(_)) => Ok(self.overlay.next()),
            (Some(base), Some(overlay)) => {
                match compare_order(base.name.as_str(), overlay.name.as_str(), self.direction) {
                    Ordering::Less => Ok(self.pending_base.take()),
                    Ordering::Greater => Ok(self.overlay.next()),
                    Ordering::Equal => {
                        self.pending_base.take();
                        Ok(self.overlay.next())
                    }
                }
            }
        }
    }
}

struct MergedIndexCatalogCursor<'txn> {
    base: HeedIndexCatalogCursor<'txn>,
    overlay: Peekable<IntoIter<IndexCatalogEntry>>,
    touched_names: HashSet<String>,
    direction: Direction,
    pending_base: Option<IndexCatalogEntry>,
}

impl<'txn> MergedIndexCatalogCursor<'txn> {
    fn new(
        base: HeedIndexCatalogCursor<'txn>,
        overlay: Vec<IndexCatalogEntry>,
        touched_names: HashSet<String>,
        direction: Direction,
    ) -> Self {
        Self {
            base,
            overlay: overlay.into_iter().peekable(),
            touched_names,
            direction,
            pending_base: None,
        }
    }

    fn next_base(&mut self) -> crate::error::Result<Option<IndexCatalogEntry>> {
        loop {
            let next = self
                .pending_base
                .take()
                .map_or_else(|| self.base.next(), |entry| Ok(Some(entry)))?;
            let Some(entry) = next else {
                return Ok(None);
            };
            if !self.touched_names.contains(&entry.name) {
                return Ok(Some(entry));
            }
        }
    }
}

impl DatastoreCursor for MergedIndexCatalogCursor<'_> {
    type Item = IndexCatalogEntry;

    fn next(&mut self) -> crate::error::Result<Option<Self::Item>> {
        if self.pending_base.is_none() {
            self.pending_base = self.next_base()?;
        }

        match (self.pending_base.as_ref(), self.overlay.peek()) {
            (None, None) => Ok(None),
            (Some(_), None) => Ok(self.pending_base.take()),
            (None, Some(_)) => Ok(self.overlay.next()),
            (Some(base), Some(overlay)) => {
                match compare_order(base.name.as_str(), overlay.name.as_str(), self.direction) {
                    Ordering::Less => Ok(self.pending_base.take()),
                    Ordering::Greater => Ok(self.overlay.next()),
                    Ordering::Equal => {
                        self.pending_base.take();
                        Ok(self.overlay.next())
                    }
                }
            }
        }
    }
}

fn index_position(entry: &IndexEntry) -> IndexPosition {
    IndexPosition {
        value: entry.value.clone(),
        document_id: entry.document_id,
    }
}

fn candidate_position(candidate: &IndexCandidate) -> IndexPosition {
    IndexPosition {
        value: candidate.value.clone(),
        document_id: candidate.document_id,
    }
}

fn compare_order<T: Ord>(left: T, right: T, direction: Direction) -> Ordering {
    match direction {
        Direction::Forward => left.cmp(&right),
        Direction::Reverse => right.cmp(&left),
    }
}

fn document_cursor_bounds(
    collection: CollectionId,
    start: CursorStart<DocumentId>,
    direction: Direction,
) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
    let collection_start = Bound::Included(encode_collection_prefix(collection));
    let collection_end = collection_end_bound(collection);

    match direction {
        Direction::Forward => {
            let start = match start {
                CursorStart::Unbounded => collection_start,
                CursorStart::Included(document_id) => {
                    Bound::Included(encode_document_key(collection, document_id, 0))
                }
                CursorStart::Excluded(document_id) => {
                    Bound::Excluded(encode_document_key(collection, document_id, u64::MAX))
                }
            };
            (start, collection_end)
        }
        Direction::Reverse => {
            let end = match start {
                CursorStart::Unbounded => collection_end,
                CursorStart::Included(document_id) => {
                    Bound::Included(encode_document_key(collection, document_id, u64::MAX))
                }
                CursorStart::Excluded(document_id) => {
                    Bound::Excluded(encode_document_key(collection, document_id, 0))
                }
            };
            (collection_start, end)
        }
    }
}

fn collection_catalog_cursor_bounds(
    start: CursorStart<String>,
    direction: Direction,
) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
    match direction {
        Direction::Forward => (string_start_bound(start), Bound::Unbounded),
        Direction::Reverse => (Bound::Unbounded, string_start_bound(start)),
    }
}

fn string_start_bound(start: CursorStart<String>) -> Bound<Vec<u8>> {
    match start {
        CursorStart::Unbounded => Bound::Unbounded,
        CursorStart::Included(name) => Bound::Included(name.into_bytes()),
        CursorStart::Excluded(name) => Bound::Excluded(name.into_bytes()),
    }
}

fn index_catalog_cursor_bounds(
    collection: CollectionId,
    start: CursorStart<String>,
    direction: Direction,
) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
    let collection_start = Bound::Included(encode_collection_prefix(collection));
    let collection_end = collection_end_bound(collection);

    match direction {
        Direction::Forward => {
            let start = match start {
                CursorStart::Unbounded => collection_start,
                CursorStart::Included(name) => {
                    Bound::Included(encode_index_catalog_key(collection, &name))
                }
                CursorStart::Excluded(name) => {
                    Bound::Excluded(encode_index_catalog_key(collection, &name))
                }
            };
            (start, collection_end)
        }
        Direction::Reverse => {
            let end = match start {
                CursorStart::Unbounded => collection_end,
                CursorStart::Included(name) => {
                    Bound::Included(encode_index_catalog_key(collection, &name))
                }
                CursorStart::Excluded(name) => {
                    Bound::Excluded(encode_index_catalog_key(collection, &name))
                }
            };
            (collection_start, end)
        }
    }
}

fn collection_end_bound(collection: CollectionId) -> Bound<Vec<u8>> {
    collection
        .checked_add(1)
        .map(encode_collection_prefix)
        .map_or(Bound::Unbounded, Bound::Excluded)
}

fn borrow_bound(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
    match bound {
        Bound::Included(value) => Bound::Included(value.as_slice()),
        Bound::Excluded(value) => Bound::Excluded(value.as_slice()),
        Bound::Unbounded => Bound::Unbounded,
    }
}
