# OtterLake DB

_Note: I made up the name, I hope it is free to use and nothing is already called like that._

This project is a database with a specific set of characteristics. It is designed to be simple yet powerful and performant.
While I don't think it will reach production quality state any time soon, I intent of making this a usable reliable database.

I will avoid using AI to write big amounts of unsupervised code. It doesn't mean I won't use any but I want this project
to be real and not random slop.

I will use rust as the programming language and many different crates for the different parts, I do not intent to build every
single peace by hand (I'm not a super expert in low level disk optimized data structures).

This project is heavily inspired by [Convex](https://convex.dev/). I essentially want to build a similar product from scratch (at least the database layer)

## Features & Characteristics

### Simple document storage

The database will count with a few primitives:
- **Collections**: A collection is a group of documents. It is the main way to organize data in the database.
- **Documents**: A document is a JSON-like object that can contain any data.
- **Indexes**: Indexes over collections to speed up queries.
- **Transactions**: The main way to read/write to the database and to ensure consistency.

Each document will have a auto generated UUIDv7 as its primary key. However, the user can use any field as a secondary index and query by it.

### Queries & Transactions

The database will provide support for interactive serializable transactions with snapshot isolation. Will support readonly and read-write 
transactions.

Readonly transactions can be subscribed to and will notify when data changed, even on multi step interactive transactions.

### Realtime

The database will keep track of changes and will notify clients when data changes. This doesn't require any special consideration on the user side

