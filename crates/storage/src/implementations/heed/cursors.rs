use heed3::types::Bytes;

use crate::{
    error::Error,
    implementations::heed::encoding::{
        ROOT_INDEX_CHAIN_ID, decode_collection_catalog_key, decode_collection_catalog_value,
        decode_document_key, decode_document_value, decode_i64, decode_index_catalog_key,
        decode_index_catalog_value, decode_index_document_id, decode_index_node_key, decode_u64,
        decode_utf8, encode_collection_catalog_key, encode_document_key, index_node_prefix,
    },
    traits::DatastoreCursor,
    types::{
        CollectionCatalogEntry, CollectionId, CursorStart, Direction, DocumentEntry, DocumentId,
        IndexCatalogEntry, IndexEntry, IndexId, IndexPosition, Value,
    },
};
use std::cmp::Ordering;

type ForwardRawCursor<'txn> = heed3::RoRange<'txn, Bytes, Bytes>;
type ReverseRawCursor<'txn> = heed3::RoRevRange<'txn, Bytes, Bytes>;
type ForwardPrefixCursor<'txn> = heed3::RoPrefix<'txn, Bytes, Bytes>;
type ReversePrefixCursor<'txn> = heed3::RoRevPrefix<'txn, Bytes, Bytes>;

pub(super) enum HeedRawCursor<'txn> {
    Forward(ForwardRawCursor<'txn>),
    Reverse(ReverseRawCursor<'txn>),
}

impl<'txn> HeedRawCursor<'txn> {
    pub(super) fn next_raw(&mut self) -> crate::error::Result<Option<(&'txn [u8], &'txn [u8])>> {
        match self {
            Self::Forward(cursor) => cursor.next().transpose().map_err(Into::into),
            Self::Reverse(cursor) => cursor.next().transpose().map_err(Into::into),
        }
    }
}

pub(super) struct HeedDocumentCursor<'txn> {
    raw: HeedRawCursor<'txn>,
    collection: CollectionId,
    ts: u64,
    reverse: bool,
    pending: Option<DecodedDocument>,
}

impl<'txn> HeedDocumentCursor<'txn> {
    pub(super) fn new(
        raw: HeedRawCursor<'txn>,
        collection: CollectionId,
        ts: u64,
        reverse: bool,
    ) -> Self {
        Self {
            raw,
            collection,
            ts,
            reverse,
            pending: None,
        }
    }

    fn next_physical(&mut self) -> crate::error::Result<Option<DecodedDocument>> {
        loop {
            let Some((key, value)) = self.raw.next_raw()? else {
                return Ok(None);
            };

            let (collection, document_id, version) = decode_document_key(key)?;
            if collection != self.collection {
                return Ok(None);
            }

            if version > self.ts {
                continue;
            }

            return Ok(Some(DecodedDocument {
                document_id,
                value: decode_document_value(value)?,
            }));
        }
    }
}

impl DatastoreCursor for HeedDocumentCursor<'_> {
    type Item = DocumentEntry;

    fn next(&mut self) -> crate::error::Result<Option<Self::Item>> {
        loop {
            let Some(first) = self
                .pending
                .take()
                .map_or_else(|| self.next_physical(), |row| Ok(Some(row)))?
            else {
                return Ok(None);
            };

            let document_id = first.document_id;
            let mut visible_value = first.value;

            while let Some(next) = self.next_physical()? {
                if next.document_id != document_id {
                    self.pending = Some(next);
                    break;
                }

                if !self.reverse {
                    visible_value = next.value;
                }
            }

            if let Some(value) = visible_value {
                return Ok(Some(DocumentEntry {
                    id: document_id,
                    value,
                }));
            }
        }
    }
}

struct DecodedDocument {
    document_id: DocumentId,
    value: Option<Value>,
}

pub(super) struct HeedCollectionCatalogCursor<'txn, 'env> {
    tx: &'txn heed3::RoTxn<'env, heed3::WithoutTls>,
    raw: HeedRawCursor<'txn>,
    collections_catalog: heed3::Database<Bytes, Bytes>,
    ts: u64,
}

impl<'txn, 'env> HeedCollectionCatalogCursor<'txn, 'env> {
    pub(super) fn new(
        tx: &'txn heed3::RoTxn<'env, heed3::WithoutTls>,
        raw: HeedRawCursor<'txn>,
        collections_catalog: heed3::Database<Bytes, Bytes>,
        ts: u64,
    ) -> Self {
        Self {
            tx,
            raw,
            collections_catalog,
            ts,
        }
    }

    fn visible_collection(
        &self,
        collection_id: CollectionId,
    ) -> crate::error::Result<Option<(String, Value)>> {
        let upper_key = encode_collection_catalog_key(collection_id, self.ts);
        let Some((stored_key, value)) = self
            .collections_catalog
            .get_lower_than_or_equal_to(self.tx, &upper_key)?
        else {
            return Ok(None);
        };

        let (stored_collection_id, stored_version) = decode_collection_catalog_key(stored_key)?;
        if stored_collection_id != collection_id || stored_version > self.ts {
            return Ok(None);
        }

        decode_collection_catalog_value(value).map(Some)
    }
}

impl DatastoreCursor for HeedCollectionCatalogCursor<'_, '_> {
    type Item = CollectionCatalogEntry;

    fn next(&mut self) -> crate::error::Result<Option<Self::Item>> {
        loop {
            let Some((key, value)) = self.raw.next_raw()? else {
                return Ok(None);
            };

            let index_name = decode_utf8(key)?;
            let id = decode_i64(value)?;
            let Some((catalog_name, metadata)) = self.visible_collection(id)? else {
                continue;
            };
            if catalog_name != index_name {
                return Err(Error::implementation(format!(
                    "collection name index points to id {id}, but catalog row has name {catalog_name}"
                )));
            }

            return Ok(Some(CollectionCatalogEntry {
                id,
                name: catalog_name,
                metadata,
            }));
        }
    }
}

pub(super) struct HeedIndexCatalogCursor<'txn> {
    raw: HeedRawCursor<'txn>,
    collection: CollectionId,
}

impl<'txn> HeedIndexCatalogCursor<'txn> {
    pub(super) fn new(raw: HeedRawCursor<'txn>, collection: CollectionId) -> Self {
        Self { raw, collection }
    }
}

impl DatastoreCursor for HeedIndexCatalogCursor<'_> {
    type Item = IndexCatalogEntry;

    fn next(&mut self) -> crate::error::Result<Option<Self::Item>> {
        let Some((key, value)) = self.raw.next_raw()? else {
            return Ok(None);
        };

        let (collection_id, name) = decode_index_catalog_key(key)?;
        if collection_id != self.collection {
            return Ok(None);
        }

        let (id, metadata) = decode_index_catalog_value(value)?;
        Ok(Some(IndexCatalogEntry {
            collection_id,
            id,
            name,
            metadata,
        }))
    }
}

pub(super) enum HeedPrefixCursor<'txn> {
    Forward(ForwardPrefixCursor<'txn>),
    Reverse(ReversePrefixCursor<'txn>),
}

impl<'txn> HeedPrefixCursor<'txn> {
    fn next_raw(&mut self) -> crate::error::Result<Option<(&'txn [u8], &'txn [u8])>> {
        match self {
            Self::Forward(cursor) => cursor.next().transpose().map_err(Into::into),
            Self::Reverse(cursor) => cursor.next().transpose().map_err(Into::into),
        }
    }
}

pub(super) struct HeedIndexCursor<'txn, 'env> {
    tx: &'txn heed3::RoTxn<'env, heed3::WithoutTls>,
    collection: CollectionId,
    index_id: IndexId,
    index_edges: heed3::Database<Bytes, Bytes>,
    index_leaves: heed3::Database<Bytes, Bytes>,
    documents: heed3::Database<Bytes, Bytes>,
    ts: u64,
    direction: Direction,
    start: CursorStart<IndexPosition>,
    stack: Vec<IndexCursorFrame<'txn>>,
}

impl<'txn, 'env> HeedIndexCursor<'txn, 'env> {
    pub(super) fn new(options: HeedIndexCursorOptions<'txn, 'env>) -> crate::error::Result<Self> {
        let HeedIndexCursorOptions {
            tx,
            collection,
            index_id,
            index_edges,
            index_leaves,
            documents,
            ts,
            start,
            direction,
        } = options;
        let mut cursor = Self {
            tx,
            collection,
            index_id,
            index_edges,
            index_leaves,
            documents,
            ts,
            direction,
            start,
            stack: Vec::new(),
        };
        cursor.push_frame(ROOT_INDEX_CHAIN_ID, Vec::new())?;
        Ok(cursor)
    }

    fn push_frame(&mut self, chain_id: u64, value_prefix: Value) -> crate::error::Result<()> {
        self.stack.push(IndexCursorFrame::new(
            self.tx,
            self.index_id,
            chain_id,
            value_prefix,
            self.index_edges,
            self.index_leaves,
            self.direction,
        )?);
        Ok(())
    }

    fn next_unfiltered(&mut self) -> crate::error::Result<Option<IndexCandidate>> {
        loop {
            let Some(frame) = self.stack.last_mut() else {
                return Ok(None);
            };

            match frame.next_action(self.direction)? {
                IndexCursorAction::Leaf(entry) => return Ok(Some(entry)),
                IndexCursorAction::Edge {
                    chain_id,
                    value_prefix,
                } => self.push_frame(chain_id, value_prefix)?,
                IndexCursorAction::Exhausted => {
                    self.stack.pop();
                }
            }
        }
    }

    fn accepts_start(&self, entry: &IndexCandidate) -> bool {
        let position = IndexPosition {
            value: entry.value.clone(),
            document_id: entry.document_id,
        };

        match (&self.start, self.direction) {
            (CursorStart::Unbounded, _) => true,
            (CursorStart::Included(start), Direction::Forward) => &position >= start,
            (CursorStart::Excluded(start), Direction::Forward) => &position > start,
            (CursorStart::Included(start), Direction::Reverse) => &position <= start,
            (CursorStart::Excluded(start), Direction::Reverse) => &position < start,
        }
    }

    fn visible_document(&self, document_id: DocumentId) -> crate::error::Result<Option<Value>> {
        let upper_key = encode_document_key(self.collection, document_id, self.ts);
        let Some((stored_key, value)) = self
            .documents
            .get_lower_than_or_equal_to(self.tx, &upper_key)?
        else {
            return Ok(None);
        };

        let (stored_collection, stored_document_id, stored_version) =
            decode_document_key(stored_key)?;
        if stored_collection != self.collection
            || stored_document_id != document_id
            || stored_version > self.ts
        {
            return Ok(None);
        }

        decode_document_value(value)
    }
}

pub(super) struct HeedIndexCursorOptions<'txn, 'env> {
    pub(super) tx: &'txn heed3::RoTxn<'env, heed3::WithoutTls>,
    pub(super) collection: CollectionId,
    pub(super) index_id: IndexId,
    pub(super) index_edges: heed3::Database<Bytes, Bytes>,
    pub(super) index_leaves: heed3::Database<Bytes, Bytes>,
    pub(super) documents: heed3::Database<Bytes, Bytes>,
    pub(super) ts: u64,
    pub(super) start: CursorStart<IndexPosition>,
    pub(super) direction: Direction,
}

impl DatastoreCursor for HeedIndexCursor<'_, '_> {
    type Item = IndexEntry;

    fn next(&mut self) -> crate::error::Result<Option<Self::Item>> {
        while let Some(entry) = self.next_unfiltered()? {
            if !self.accepts_start(&entry) {
                continue;
            }

            if let Some(document_value) = self.visible_document(entry.document_id)? {
                return Ok(Some(IndexEntry {
                    value: entry.value,
                    document_id: entry.document_id,
                    document_value,
                }));
            }
        }

        Ok(None)
    }
}

struct IndexCursorFrame<'txn> {
    value_prefix: Value,
    leaves: HeedPrefixCursor<'txn>,
    edges: HeedPrefixCursor<'txn>,
    pending_leaf: Option<LeafCandidate>,
    pending_edge: Option<EdgeCandidate>,
}

impl<'txn> IndexCursorFrame<'txn> {
    fn new(
        tx: &'txn heed3::RoTxn<'_, heed3::WithoutTls>,
        index_id: IndexId,
        chain_id: u64,
        value_prefix: Value,
        index_edges: heed3::Database<Bytes, Bytes>,
        index_leaves: heed3::Database<Bytes, Bytes>,
        direction: Direction,
    ) -> crate::error::Result<Self> {
        let prefix = index_node_prefix(index_id, chain_id);
        let leaves = match direction {
            Direction::Forward => HeedPrefixCursor::Forward(index_leaves.prefix_iter(tx, &prefix)?),
            Direction::Reverse => {
                HeedPrefixCursor::Reverse(index_leaves.rev_prefix_iter(tx, &prefix)?)
            }
        };
        let edges = match direction {
            Direction::Forward => HeedPrefixCursor::Forward(index_edges.prefix_iter(tx, &prefix)?),
            Direction::Reverse => {
                HeedPrefixCursor::Reverse(index_edges.rev_prefix_iter(tx, &prefix)?)
            }
        };

        Ok(Self {
            value_prefix,
            leaves,
            edges,
            pending_leaf: None,
            pending_edge: None,
        })
    }

    fn next_action(&mut self, direction: Direction) -> crate::error::Result<IndexCursorAction> {
        if self.pending_leaf.is_none() {
            self.pending_leaf = self.next_leaf()?;
        }
        if self.pending_edge.is_none() {
            self.pending_edge = self.next_edge()?;
        }

        let Some(action) = self.choose_action(direction) else {
            return Ok(IndexCursorAction::Exhausted);
        };

        Ok(match action {
            PendingAction::Leaf => {
                let leaf = self
                    .pending_leaf
                    .take()
                    .expect("pending leaf must exist when selected");
                let mut value = self.value_prefix.clone();
                value.extend_from_slice(&leaf.segment);
                IndexCursorAction::Leaf(IndexCandidate {
                    value,
                    document_id: leaf.document_id,
                })
            }
            PendingAction::Edge => {
                let edge = self
                    .pending_edge
                    .take()
                    .expect("pending edge must exist when selected");
                let mut value_prefix = self.value_prefix.clone();
                value_prefix.extend_from_slice(&edge.segment);
                IndexCursorAction::Edge {
                    chain_id: edge.child_chain_id,
                    value_prefix,
                }
            }
        })
    }

    fn choose_action(&self, direction: Direction) -> Option<PendingAction> {
        match (&self.pending_leaf, &self.pending_edge) {
            (Some(_), None) => Some(PendingAction::Leaf),
            (None, Some(_)) => Some(PendingAction::Edge),
            (None, None) => None,
            (Some(leaf), Some(edge)) => match (leaf.segment.cmp(&edge.segment), direction) {
                (Ordering::Less | Ordering::Equal, Direction::Forward) => Some(PendingAction::Leaf),
                (Ordering::Greater, Direction::Forward) => Some(PendingAction::Edge),
                (Ordering::Greater, Direction::Reverse) => Some(PendingAction::Leaf),
                (Ordering::Less | Ordering::Equal, Direction::Reverse) => Some(PendingAction::Edge),
            },
        }
    }

    fn next_leaf(&mut self) -> crate::error::Result<Option<LeafCandidate>> {
        let Some((key, value)) = self.leaves.next_raw()? else {
            return Ok(None);
        };

        let (_, _, segment) = decode_index_node_key(key)?;
        Ok(Some(LeafCandidate {
            segment,
            document_id: decode_index_document_id(value)?,
        }))
    }

    fn next_edge(&mut self) -> crate::error::Result<Option<EdgeCandidate>> {
        let Some((key, value)) = self.edges.next_raw()? else {
            return Ok(None);
        };

        let (_, _, segment) = decode_index_node_key(key)?;
        Ok(Some(EdgeCandidate {
            segment,
            child_chain_id: decode_u64(value)?,
        }))
    }
}

enum IndexCursorAction {
    Leaf(IndexCandidate),
    Edge { chain_id: u64, value_prefix: Value },
    Exhausted,
}

struct IndexCandidate {
    value: Value,
    document_id: DocumentId,
}

enum PendingAction {
    Leaf,
    Edge,
}

struct LeafCandidate {
    segment: Value,
    document_id: DocumentId,
}

struct EdgeCandidate {
    segment: Value,
    child_chain_id: u64,
}
