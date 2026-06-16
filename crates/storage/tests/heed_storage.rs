use std::collections::HashMap;

use storage::{
    implementations::heed::datastore::HeedStorageEngine,
    traits::{Datastore, DatastoreCursor, DatastoreTransaction},
    types::{
        CollectionCatalogEntry, CollectionId, CursorStart, Direction, IndexEntry, IndexId,
        IndexPosition,
    },
    write_set::{CollectionWriteSet, WriteSet},
};
use tempfile::TempDir;

fn open_engine() -> (TempDir, HeedStorageEngine) {
    let dir = tempfile::tempdir().expect("temp dir");
    let engine = HeedStorageEngine::open(dir.path().to_str().expect("utf-8 temp path"))
        .expect("open heed engine");
    (dir, engine)
}

fn create_collection_and_index(engine: &HeedStorageEngine) -> (CollectionId, IndexId) {
    engine
        .put(WriteSet {
            collections: HashMap::new(),
            new_collections: vec![(-1, "docs".to_owned())],
            new_indexes: vec![(-1, -1, "by_value".to_owned(), Vec::new())],
            ts: 1,
        })
        .expect("create catalog entries");

    let tx = engine.transaction(1).expect("read transaction");
    let collection = tx
        .collection("docs")
        .expect("collection lookup")
        .expect("collection exists");
    let mut indexes = tx
        .get_indexes_catalog_cursor(collection, CursorStart::Unbounded, Direction::Forward)
        .expect("index catalog cursor");
    let index = indexes
        .next()
        .expect("next index")
        .expect("index exists")
        .id;
    (collection, index)
}

fn put_index_entries(
    engine: &HeedStorageEngine,
    ts: u64,
    collection: CollectionId,
    entries: Vec<(IndexId, Vec<u8>, u128)>,
) {
    put_index_changes(engine, ts, collection, entries, Vec::new());
}

fn delete_index_entries(
    engine: &HeedStorageEngine,
    ts: u64,
    collection: CollectionId,
    entries: Vec<(IndexId, Vec<u8>, u128)>,
) {
    put_index_changes(engine, ts, collection, Vec::new(), entries);
}

fn delete_documents(
    engine: &HeedStorageEngine,
    ts: u64,
    collection: CollectionId,
    deleted_keys: Vec<u128>,
) {
    let mut collections = HashMap::new();
    collections.insert(
        collection,
        CollectionWriteSet {
            documents: Vec::new(),
            deleted_keys,
            index_entries: Vec::new(),
            deleted_index_entries: Vec::new(),
            metadata: None,
        },
    );

    engine
        .put(WriteSet {
            collections,
            new_collections: Vec::new(),
            new_indexes: Vec::new(),
            ts,
        })
        .expect("delete documents");
}

fn put_index_changes(
    engine: &HeedStorageEngine,
    ts: u64,
    collection: CollectionId,
    entries: Vec<(IndexId, Vec<u8>, u128)>,
    deleted_entries: Vec<(IndexId, Vec<u8>, u128)>,
) {
    let documents = entries
        .iter()
        .map(|(_, _, document_id)| (*document_id, document_value(*document_id)))
        .collect();
    let mut collections = HashMap::new();
    collections.insert(
        collection,
        CollectionWriteSet {
            documents,
            deleted_keys: Vec::new(),
            index_entries: entries,
            deleted_index_entries: deleted_entries,
            metadata: None,
        },
    );

    engine
        .put(WriteSet {
            collections,
            new_collections: Vec::new(),
            new_indexes: Vec::new(),
            ts,
        })
        .expect("write index changes");
}

fn collect_index(cursor: impl DatastoreCursor<Item = IndexEntry>) -> Vec<IndexEntry> {
    let mut cursor = cursor;
    let mut entries = Vec::new();
    while let Some(entry) = cursor.next().expect("cursor next") {
        entries.push(entry);
    }
    entries
}

fn collect_collections(
    cursor: impl DatastoreCursor<Item = CollectionCatalogEntry>,
) -> Vec<CollectionCatalogEntry> {
    let mut cursor = cursor;
    let mut entries = Vec::new();
    while let Some(entry) = cursor.next().expect("cursor next") {
        entries.push(entry);
    }
    entries
}

fn index_entries(
    engine: &HeedStorageEngine,
    ts: u64,
    collection: CollectionId,
    index: IndexId,
    start: CursorStart<IndexPosition>,
    direction: Direction,
) -> Vec<IndexEntry> {
    let tx = engine.transaction(ts).expect("read transaction");
    let cursor = tx
        .get_index_cursor(collection, index, start, direction)
        .expect("index cursor");
    collect_index(cursor)
}

fn document_value(document_id: u128) -> Vec<u8> {
    format!("doc-{document_id}").into_bytes()
}

fn entry(value: &[u8], document_id: u128) -> IndexEntry {
    IndexEntry {
        value: value.to_vec(),
        document_id,
        document_value: document_value(document_id),
    }
}

fn collection_entry(id: CollectionId, name: &str, metadata: &[u8]) -> CollectionCatalogEntry {
    CollectionCatalogEntry {
        id,
        name: name.to_owned(),
        metadata: metadata.to_vec(),
    }
}

#[test]
fn collection_catalog_is_timestamped() {
    let (_dir, engine) = open_engine();

    engine
        .put(WriteSet {
            collections: HashMap::new(),
            new_collections: vec![(-1, "docs".to_owned())],
            new_indexes: Vec::new(),
            ts: 1,
        })
        .expect("create collection");

    let tx = engine.transaction(0).expect("read transaction");
    assert_eq!(tx.collection("docs").expect("collection lookup"), None);
    let cursor = tx
        .get_collections_catalog_cursor(CursorStart::Unbounded, Direction::Forward)
        .expect("collections cursor");
    assert_eq!(collect_collections(cursor), Vec::new());

    let tx = engine.transaction(1).expect("read transaction");
    let collection = tx
        .collection("docs")
        .expect("collection lookup")
        .expect("collection exists");
    let cursor = tx
        .get_collections_catalog_cursor(CursorStart::Unbounded, Direction::Forward)
        .expect("collections cursor");
    assert_eq!(
        collect_collections(cursor),
        vec![collection_entry(collection, "docs", &[])]
    );

    let mut collections = HashMap::new();
    collections.insert(
        collection,
        CollectionWriteSet {
            documents: Vec::new(),
            deleted_keys: Vec::new(),
            index_entries: Vec::new(),
            deleted_index_entries: Vec::new(),
            metadata: Some(b"v2".to_vec()),
        },
    );
    engine
        .put(WriteSet {
            collections,
            new_collections: Vec::new(),
            new_indexes: Vec::new(),
            ts: 2,
        })
        .expect("update collection metadata");

    let tx = engine.transaction(1).expect("read transaction");
    let cursor = tx
        .get_collections_catalog_cursor(CursorStart::Unbounded, Direction::Forward)
        .expect("collections cursor");
    assert_eq!(
        collect_collections(cursor),
        vec![collection_entry(collection, "docs", &[])]
    );

    let tx = engine.transaction(2).expect("read transaction");
    let cursor = tx
        .get_collections_catalog_cursor(CursorStart::Unbounded, Direction::Forward)
        .expect("collections cursor");
    assert_eq!(
        collect_collections(cursor),
        vec![collection_entry(collection, "docs", b"v2")]
    );
}

#[test]
fn collection_catalog_cursor_scans_name_index_order() {
    let (_dir, engine) = open_engine();

    engine
        .put(WriteSet {
            collections: HashMap::new(),
            new_collections: vec![
                (-1, "beta".to_owned()),
                (-2, "alpha".to_owned()),
                (-3, "gamma".to_owned()),
            ],
            new_indexes: Vec::new(),
            ts: 1,
        })
        .expect("create collections");

    let tx = engine.transaction(1).expect("read transaction");
    let alpha = tx
        .collection("alpha")
        .expect("collection lookup")
        .expect("alpha exists");
    let beta = tx
        .collection("beta")
        .expect("collection lookup")
        .expect("beta exists");
    let gamma = tx
        .collection("gamma")
        .expect("collection lookup")
        .expect("gamma exists");

    let cursor = tx
        .get_collections_catalog_cursor(CursorStart::Unbounded, Direction::Forward)
        .expect("collections cursor");
    assert_eq!(
        collect_collections(cursor),
        vec![
            collection_entry(alpha, "alpha", &[]),
            collection_entry(beta, "beta", &[]),
            collection_entry(gamma, "gamma", &[]),
        ]
    );

    let cursor = tx
        .get_collections_catalog_cursor(
            CursorStart::Excluded("alpha".to_owned()),
            Direction::Forward,
        )
        .expect("collections cursor");
    assert_eq!(
        collect_collections(cursor),
        vec![
            collection_entry(beta, "beta", &[]),
            collection_entry(gamma, "gamma", &[]),
        ]
    );

    let cursor = tx
        .get_collections_catalog_cursor(
            CursorStart::Included("beta".to_owned()),
            Direction::Reverse,
        )
        .expect("collections cursor");
    assert_eq!(
        collect_collections(cursor),
        vec![
            collection_entry(beta, "beta", &[]),
            collection_entry(alpha, "alpha", &[]),
        ]
    );
}

#[test]
fn collection_metadata_update_uses_persistent_id_lookup() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().to_str().expect("utf-8 temp path").to_owned();
    let engine = HeedStorageEngine::open(&path).expect("open heed engine");

    engine
        .put(WriteSet {
            collections: HashMap::new(),
            new_collections: vec![(-1, "docs".to_owned())],
            new_indexes: Vec::new(),
            ts: 1,
        })
        .expect("create collection");

    let tx = engine.transaction(1).expect("read transaction");
    let collection = tx
        .collection("docs")
        .expect("collection lookup")
        .expect("collection exists");
    drop(tx);
    drop(engine);

    let engine = HeedStorageEngine::open(&path).expect("reopen heed engine");
    let mut collections = HashMap::new();
    collections.insert(
        collection,
        CollectionWriteSet {
            documents: Vec::new(),
            deleted_keys: Vec::new(),
            index_entries: Vec::new(),
            deleted_index_entries: Vec::new(),
            metadata: Some(b"after-reopen".to_vec()),
        },
    );
    engine
        .put(WriteSet {
            collections,
            new_collections: Vec::new(),
            new_indexes: Vec::new(),
            ts: 2,
        })
        .expect("update collection metadata");

    let tx = engine.transaction(2).expect("read transaction");
    let cursor = tx
        .get_collections_catalog_cursor(CursorStart::Unbounded, Direction::Forward)
        .expect("collections cursor");
    assert_eq!(
        collect_collections(cursor),
        vec![collection_entry(collection, "docs", b"after-reopen")]
    );
}

#[test]
fn index_metadata_is_stored_on_creation_and_not_updated() {
    let (_dir, engine) = open_engine();

    engine
        .put(WriteSet {
            collections: HashMap::new(),
            new_collections: vec![(-1, "docs".to_owned())],
            new_indexes: vec![(-1, -1, "by_value".to_owned(), b"v1".to_vec())],
            ts: 1,
        })
        .expect("create collection and index");

    let tx = engine.transaction(1).expect("read transaction");
    let collection = tx
        .collection("docs")
        .expect("collection lookup")
        .expect("collection exists");
    let mut indexes = tx
        .get_indexes_catalog_cursor(collection, CursorStart::Unbounded, Direction::Forward)
        .expect("index catalog cursor");
    let created = indexes.next().expect("next index").expect("index exists");
    assert_eq!(created.name, "by_value");
    assert_eq!(created.metadata, b"v1".to_vec());
    assert!(indexes.next().expect("next index").is_none());

    engine
        .put(WriteSet {
            collections: HashMap::new(),
            new_collections: Vec::new(),
            new_indexes: vec![(collection, -1, "by_value".to_owned(), b"v2".to_vec())],
            ts: 2,
        })
        .expect("recreate existing index");

    let tx = engine.transaction(2).expect("read transaction");
    let mut indexes = tx
        .get_indexes_catalog_cursor(collection, CursorStart::Unbounded, Direction::Forward)
        .expect("index catalog cursor");
    let current = indexes.next().expect("next index").expect("index exists");
    assert_eq!(current.id, created.id);
    assert_eq!(current.metadata, b"v1".to_vec());
    assert!(indexes.next().expect("next index").is_none());
}

#[test]
fn index_cursor_returns_entries_in_forward_and_reverse_order() {
    let (_dir, engine) = open_engine();
    let (collection, index) = create_collection_and_index(&engine);

    put_index_entries(
        &engine,
        2,
        collection,
        vec![
            (index, b"b".to_vec(), 30),
            (index, b"a".to_vec(), 20),
            (index, b"a".to_vec(), 10),
            (index, Vec::new(), 5),
        ],
    );

    assert_eq!(
        index_entries(
            &engine,
            2,
            collection,
            index,
            CursorStart::Unbounded,
            Direction::Forward
        ),
        vec![
            entry(b"", 5),
            entry(b"a", 10),
            entry(b"a", 20),
            entry(b"b", 30)
        ]
    );

    assert_eq!(
        index_entries(
            &engine,
            2,
            collection,
            index,
            CursorStart::Unbounded,
            Direction::Reverse
        ),
        vec![
            entry(b"b", 30),
            entry(b"a", 20),
            entry(b"a", 10),
            entry(b"", 5)
        ]
    );
}

#[test]
fn index_cursor_honors_included_and_excluded_starts() {
    let (_dir, engine) = open_engine();
    let (collection, index) = create_collection_and_index(&engine);

    put_index_entries(
        &engine,
        2,
        collection,
        vec![
            (index, Vec::new(), 5),
            (index, b"a".to_vec(), 10),
            (index, b"a".to_vec(), 20),
            (index, b"b".to_vec(), 30),
        ],
    );

    let at_a10 = IndexPosition {
        value: b"a".to_vec(),
        document_id: 10,
    };
    let at_a20 = IndexPosition {
        value: b"a".to_vec(),
        document_id: 20,
    };

    assert_eq!(
        index_entries(
            &engine,
            2,
            collection,
            index,
            CursorStart::Included(at_a10.clone()),
            Direction::Forward
        ),
        vec![entry(b"a", 10), entry(b"a", 20), entry(b"b", 30)]
    );
    assert_eq!(
        index_entries(
            &engine,
            2,
            collection,
            index,
            CursorStart::Excluded(at_a10),
            Direction::Forward
        ),
        vec![entry(b"a", 20), entry(b"b", 30)]
    );
    assert_eq!(
        index_entries(
            &engine,
            2,
            collection,
            index,
            CursorStart::Included(at_a20.clone()),
            Direction::Reverse
        ),
        vec![entry(b"a", 20), entry(b"a", 10), entry(b"", 5)]
    );
    assert_eq!(
        index_entries(
            &engine,
            2,
            collection,
            index,
            CursorStart::Excluded(at_a20),
            Direction::Reverse
        ),
        vec![entry(b"a", 10), entry(b"", 5)]
    );
}

#[test]
fn index_cursor_orders_segmented_long_keys() {
    let (_dir, engine) = open_engine();
    let (collection, index) = create_collection_and_index(&engine);

    let values = vec![
        vec![b'a'; 494],
        vec![b'a'; 495],
        vec![b'a'; 496],
        vec![b'a'; 990],
        vec![b'a'; 991],
        vec![b'b'; 700],
    ];
    put_index_entries(
        &engine,
        2,
        collection,
        values
            .iter()
            .enumerate()
            .map(|(i, value)| (index, value.clone(), i as u128 + 1))
            .collect(),
    );

    let mut expected: Vec<_> = values
        .into_iter()
        .enumerate()
        .map(|(i, value)| IndexEntry {
            value,
            document_id: i as u128 + 1,
            document_value: document_value(i as u128 + 1),
        })
        .collect();
    expected.sort_by(|a, b| {
        IndexPosition {
            value: a.value.clone(),
            document_id: a.document_id,
        }
        .cmp(&IndexPosition {
            value: b.value.clone(),
            document_id: b.document_id,
        })
    });

    assert_eq!(
        index_entries(
            &engine,
            2,
            collection,
            index,
            CursorStart::Unbounded,
            Direction::Forward
        ),
        expected
    );

    let mut reverse_expected = expected;
    reverse_expected.reverse();
    assert_eq!(
        index_entries(
            &engine,
            2,
            collection,
            index,
            CursorStart::Unbounded,
            Direction::Reverse
        ),
        reverse_expected
    );
}

#[test]
fn index_delete_removes_single_leaf_duplicate() {
    let (_dir, engine) = open_engine();
    let (collection, index) = create_collection_and_index(&engine);

    put_index_entries(
        &engine,
        2,
        collection,
        vec![
            (index, b"a".to_vec(), 10),
            (index, b"a".to_vec(), 20),
            (index, vec![b'z'; 800], 30),
        ],
    );
    delete_index_entries(
        &engine,
        3,
        collection,
        vec![(index, b"a".to_vec(), 10), (index, vec![b'z'; 800], 30)],
    );

    assert_eq!(
        index_entries(
            &engine,
            3,
            collection,
            index,
            CursorStart::Unbounded,
            Direction::Forward
        ),
        vec![entry(b"a", 20)]
    );
}

#[test]
fn index_cursor_filters_documents_not_visible_at_read_timestamp() {
    let (_dir, engine) = open_engine();
    let (collection, index) = create_collection_and_index(&engine);

    put_index_entries(
        &engine,
        2,
        collection,
        vec![(index, b"a".to_vec(), 10), (index, b"b".to_vec(), 20)],
    );
    delete_documents(&engine, 3, collection, vec![10]);

    assert_eq!(
        index_entries(
            &engine,
            2,
            collection,
            index,
            CursorStart::Unbounded,
            Direction::Forward
        ),
        vec![entry(b"a", 10), entry(b"b", 20)]
    );

    assert_eq!(
        index_entries(
            &engine,
            3,
            collection,
            index,
            CursorStart::Unbounded,
            Direction::Forward
        ),
        vec![entry(b"b", 20)]
    );
}

#[test]
fn set_ts_rejects_values_lower_than_current() {
    let (_dir, engine) = open_engine();

    engine.set_ts(10).expect("set timestamp");
    assert_eq!(engine.get_ts().expect("get timestamp"), 10);

    assert!(engine.set_ts(9).is_err());
    assert_eq!(engine.get_ts().expect("get timestamp"), 10);

    engine
        .set_ts(10)
        .expect("setting same timestamp is allowed");
    engine.set_ts(11).expect("advancing timestamp is allowed");
    assert_eq!(engine.get_ts().expect("get timestamp"), 11);
}

#[test]
fn put_rejects_batches_with_lower_timestamp() {
    let (_dir, engine) = open_engine();
    let (collection, index) = create_collection_and_index(&engine);
    engine.set_ts(10).expect("set timestamp");

    let mut collections = HashMap::new();
    collections.insert(
        collection,
        CollectionWriteSet {
            documents: Vec::new(),
            deleted_keys: Vec::new(),
            index_entries: vec![(index, b"stale".to_vec(), 1)],
            deleted_index_entries: Vec::new(),
            metadata: None,
        },
    );

    assert!(
        engine
            .put(WriteSet {
                collections,
                new_collections: Vec::new(),
                new_indexes: Vec::new(),
                ts: 9,
            })
            .is_err()
    );
    assert_eq!(engine.get_ts().expect("get timestamp"), 10);
    assert_eq!(
        index_entries(
            &engine,
            10,
            collection,
            index,
            CursorStart::Unbounded,
            Direction::Forward
        ),
        Vec::new()
    );
}
