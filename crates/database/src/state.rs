use std::{
    collections::{BTreeSet, HashMap},
    sync::atomic::AtomicUsize,
};

use storage::types::TS;

pub struct DatabaseState {
    pub(crate) read_transactions: BTreeSet<(TS, u64)>, // (start_ts, tx_id) - includes also RW transactions
    pub(crate) write_transactions: BTreeSet<(TS, u64)>, // (start_ts, tx_id) - includes only RW transactions

    // Committed sets (ts, ... write set ...)
    // Documents commited between latest ts and min_rw_ts
    pub(crate) committed_sets: HashMap<u64, u64>,

    // Lowers ts of active readonly transactions
    pub(crate) min_read_ts: u64,

    // Lowers ts of active readwrite transactions
    pub(crate) min_write_ts: u64,

    // Transaction id allocator
    pub(crate) tx_id_allocator: AtomicUsize,
}
