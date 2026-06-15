# Storage Engine

This crate handles low level document storage, catalog and indexes. It does not handle document indexing/sync. It is not document shape aware, it is just a binary store with a few primitives. It is timestamp aware, allowing to read documents as of a specific timestamp for MVCC.

## Read transactions

Allows to read documents under a specific TS, scan forward and backward, and read the catalog. It is a snapshot of the database at a specific point in time, it is not affected by concurrent write transactions.

## Write transactions

It doesn't handle any time of interactive write transactions, it just allows for a single batch write. Depending on the implementation writes may be atomic, but it is not a guarantee. We can work around this by using versioned documents using the timestamp.

## Implementations

The storage module defines a specific interface, the default implementation is based on LMDB using the `heed` crate. 

## LMDB databases

### Indexes catalog

Catalog of existing indexes with config/metadata (opaque to this layer).

Format: `[collection id: u64][index name: string] -> [id: u64][index config (opaque)]`

### Collections catalog

Catalog of collections.

Format: `[collection name: string] -> [id: u64][collection config (opaque)]`

### Index entries

Here is where the fun start.

Index values may be any length, without limit. LMDB only allows up to 511 bytes which is pretty low. 
We are going to chain multiple entries like a tree, sharing the same initial parts of the key.

Lets say a segment limit of 4 bytes, we will have a structure like this:
```
[hi] -> doc id
[hell][chain id A]
[chain id A][o wo] -> [chain id B]
[chain id B][rld] -> doc id
```

Structure:
- Entry less than MAX_SEGMENT_SIZE: `[0x03][entry][doc id] -> (empty)`
- First entry of a chain: `[0x02][entry] -> chain id (unique monotonic counter u64)`
- Middle segment of a chain: `[0x00][chain id][entry] -> chain id (unique monotonic counter u64)`
- Last segment of a chain: `[0x01][chain id][entry][doc id] -> (empty)`

Initial and middle segments can be reused by multiple entries. Hot paths can be cached in memory.

### Documents

The documents themselves, stored as binary blobs.

Format: `[collection id: u64][doc id: u128][timestamp: u64] -> [document (opaque)]`