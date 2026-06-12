# Storage Engine

This crate handles low level document storage, indexing and catalog.

## Docstore

Primitives:
- store document to a btree/collection
- retrieve document from a btree/collection
- delete document from a btree/collection
- iterate/stream documents from a btree/collection starting from a given key (inclusive or exclusive) in one direction (forward or backward) and with a given batch size.

## Indexes

Create a index over a collection and a field.

Primitives:
- insert document to a index
- delete document from a index
- query index with a value and get the corresponding document ids
- iterate/stream index entries starting from a given key (inclusive or exclusive) in one direction (forward or backward) and with a given batch size.

## Catalog

Keep track of collections and indexes.

Primitives:
- create collection
- delete collection
- create index
- delete index
- get collection info (name, indexes, etc)
- get index info (name, collection, field, etc)
- list collections
- list indexes

# Low level storage structure

For LMDB databases.

## Catalog

Key structure `[entity_type: 1 byte][entity name]`

Entity types:
- `0x01` collection
- `0x02` index

Value: `[id: 64bits][metadata]`

Max collection name length: 255 bytes.

## Documents

Key structure `[collection_id: 64bits][document_id: 128bits uuid]`

Value: document data (jsonb)

## Indexes

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