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