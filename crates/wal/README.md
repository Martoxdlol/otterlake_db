## WAL Entry Types

### Transaction Commit

This is the main entry type and the only one that modifies data.

It contains the write set of a transaction, which allows to do a replay of the transaction on crash.

### Visible TS

This entry type is used to track the visible timestamp of transactions. 
The visible ts only changes when a transaction commits and gets replicated successfully to all replicas or quorum (depending on the implementation and config).

### Index backfilling

Not yet defined, but we need a entry for starting a index backfill, a entry for some checkpoint (the collection can be long) and a entry for finishing the index backfill.