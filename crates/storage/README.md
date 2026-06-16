# Storage Engine

This crate handles low level document storage, catalog and indexes. It does not handle document indexing/sync. It is not document shape aware, it is just a binary store with a few primitives. It is timestamp aware, allowing to read documents as of a specific timestamp for MVCC.

Index entries are physical entries, not MVCC-versioned entries. Higher layers add every index entry that may still be needed by an active reader and remove entries only after no active read transaction can need it. Storage index cursors validate that the referenced document is visible and live at the read timestamp, and return the document value with the index hit. This crate is still document-shape opaque, so predicate-level validation belongs to the layer that understands the index definition.

## Read transactions

Allows to read documents under a specific TS, scan forward and backward from an unbounded, included, or excluded start key, scan visible index entries with document values, and read the catalog. Cursor limits are controlled by callers consuming entries from the cursor. It is a snapshot of the database at a specific point in time, it is not affected by concurrent write transactions.

## Write transactions

It doesn't handle any time of interactive write transactions, it just allows for a single batch write. Depending on the implementation writes may be atomic, but it is not a guarantee. We can work around this by using versioned documents using the timestamp.

## Implementations

The storage module defines a specific interface, the default implementation is based on LMDB using the `heed` crate. 

## LMDB databases

### Indexes catalog

Catalog of existing indexes with config/metadata (opaque to this layer).

Format: `[collection id: i64][index name: string] -> [index id: i64][index config (opaque)]`

### Collections catalog

Catalog of collections.

Format: `[collection name: string] -> [collection id: i64][collection config (opaque)]`

### Index entries

Here is where the fun starts.

Index values may be any length, without limit. LMDB only allows keys up to 511 bytes, which is too low for arbitrary serialized index values. We chain entries like a segmented tree, sharing initial path segments where possible.

The heed implementation stores the trie in two LMDB databases:

- `index_edges`: child links between trie nodes.
- `index_leaves`: document ids at terminal trie nodes. This database uses `DUP_SORT`, so multiple document ids for the same index value do not repeat the full key.

Root is represented by `chain id = 0`.

Formats:
- Edge: `[index id: i64][parent chain id: u64][segment bytes] -> [child chain id: u64]`
- Leaf: `[index id: i64][parent chain id: u64][segment bytes] -> [doc id: u128]`

With `[index id][chain id]` using 16 bytes, each segment may use up to `495` bytes and still stay under LMDB's 511-byte key limit.

Lets say a segment limit of 4 bytes, we will have a structure like this for an index value `hello world`:
```
[index id][0][hell] -> chain id A        (edge)
[index id][A][o wo] -> chain id B        (edge)
[index id][B][rld] -> doc id             (leaf duplicate value)
```

Short values use the same structure and become a single root leaf:
```
[index id][0][entry] -> doc id           (leaf duplicate value)
```

Logical cursor order is produced by a trie cursor. At each trie node, the cursor merges:
- leaf segments for terminal values at that node
- edge segments for child nodes

When a leaf segment and edge segment have the same bytes, the leaf is considered first because shorter byte strings sort before longer byte strings. Each leaf candidate is then checked against the timestamped document table for the cursor's collection. Tombstones and future document versions are skipped; live documents are returned with the index value, document id, and document bytes. Initial and middle segments can be reused by multiple entries. Hot paths can be cached in memory.

### Documents

The documents themselves, stored as binary blobs.

Format: `[collection id: i64][doc id: u128][timestamp: u64] -> [document marker][document (opaque)]`

Document marker:
- `[0x00]`: tombstone
- `[0x01][document]`: live value

### Vacuum targets

When we create a new version of a document (an insert or delete), we add an entry to a vacuum list so we can later
clean up old versions without full table scans.
