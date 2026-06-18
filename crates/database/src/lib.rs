use std::{
    collections::{BTreeSet, BinaryHeap, HashMap},
    sync::{Arc, atomic::AtomicUsize},
    thread,
    time::Duration,
};

use storage::{traits::Datastore, types::TS};
use tokio::{
    sync::{RwLock, mpsc::UnboundedSender, oneshot},
    task::spawn_blocking,
};

pub struct Database<T: Datastore> {
    datastore: T,

    pool: Arc<Vec<(thread::JoinHandle<()>, UnboundedSender<TransactionCommand>)>>,
    pool_rr: Arc<AtomicUsize>,

    state: Arc<RwLock<DatabaseState>>,
}

#[derive(Clone)]
pub enum TransactionMode {
    ReadOnly,
    ReadWrite,
}

pub struct EndTransactionOutput {
    // commit or end (for read and rw transactions)
}

pub struct RollbackTransactionOutput {
    // only for rw transactions
}

pub struct CommitOutput {
    // only for rw transactions
}

enum TransactionCommand {
    StartTransaction {
        mode: TransactionMode,
        tx_id: u64,
    },
    EndTransaction {
        tx_id: u64,
        tx: oneshot::Sender<EndTransactionOutput>,
        // end (readonly transaction)
    },
    CommitTransaction {
        tx_id: u64,
        tx: oneshot::Sender<CommitOutput>,
        // commit or end (for read and rw transactions)
    },
    RollbackTransaction {
        tx_id: u64,
        tx: oneshot::Sender<RollbackTransactionOutput>,
        // only for rw transactions
    },
    Read {
        tx_id: u64,
        tx: oneshot::Sender<()>,
        // read query
    },
    Write {
        tx_id: u64,
        tx: oneshot::Sender<()>,
        // write query
    },
}

pub struct DatabaseState {
    read_transactions: BTreeSet<(TS, u64)>, // (start_ts, tx_id) - includes also RW transactions
    write_transactions: BTreeSet<(TS, u64)>, // (start_ts, tx_id) - includes only RW transactions

    // Committed sets (ts, ... write set ...)
    // Documents commited between latest ts and min_rw_ts
    committed_sets: HashMap<u64, u64>,

    // Lowers ts of active readonly transactions
    min_read_ts: u64,

    // Lowers ts of active readwrite transactions
    min_write_ts: u64,

    // Transaction id allocator
    tx_id_allocator: AtomicUsize,
}

pub struct Config {
    pub thread_pool_size: usize,
    pub max_transaction_duration: Duration,
}

fn run_transaction_thread(
    ds: impl Datastore,
    st: Arc<RwLock<DatabaseState>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<TransactionCommand>,
) {
    let mut transactions = HashMap::new();
    // iterate over channel reading commands (start transaction, end transaction, read/write within transaction)
    // execute against datastore
    // put result into response channel of the corresponding transaction
    while let Some(cmd) = rx.blocking_recv() {
        match cmd {
            TransactionCommand::StartTransaction { mode, tx_id } => {
                transactions.insert(tx_id, mode);
            }
            TransactionCommand::EndTransaction { tx_id, tx } => {
                // end transaction
            }
            TransactionCommand::CommitTransaction { tx_id, tx } => {
                // commit transaction
            }
            TransactionCommand::RollbackTransaction { tx_id, tx } => {
                // rollback transaction
            }
            TransactionCommand::Read { tx_id, tx } => {
                // read query
            }
            TransactionCommand::Write { tx_id, tx } => {
                // write query
            }
        }
    }
}

pub struct Transaction {
    tx_id: u64,
    start_ts: u64,
    mode: TransactionMode,
    tx: UnboundedSender<TransactionCommand>,
}

impl Transaction {
    pub async fn commit(self) -> Result<CommitOutput, Box<dyn std::error::Error>> {
        let (tx, rx) = oneshot::channel();

        self.tx.send(TransactionCommand::CommitTransaction {
            tx_id: self.tx_id,
            tx,
        })?;

        Ok(rx.await?)
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        // End transaction (commit)
        match self.mode {
            TransactionMode::ReadOnly => {
                let (tx, _rx) = oneshot::channel();
                let _ = self.tx.send(TransactionCommand::EndTransaction {
                    tx_id: self.tx_id,
                    tx,
                });
            }
            TransactionMode::ReadWrite => {
                let (tx, _rx) = oneshot::channel();
                let _ = self.tx.send(TransactionCommand::CommitTransaction {
                    tx_id: self.tx_id,
                    tx,
                });
            }
        }
    }
}

impl<T: Datastore + 'static> Database<T> {
    pub async fn new(datastore: T, config: Config) -> Result<Self, Box<dyn std::error::Error>> {
        let ts = datastore.get_ts()?;

        // todo: wal replay

        let state = Arc::new(RwLock::new(DatabaseState {
            read_transactions: BTreeSet::new(),
            write_transactions: BTreeSet::new(),
            committed_sets: HashMap::new(),
            min_read_ts: ts,
            min_write_ts: ts,
            tx_id_allocator: AtomicUsize::new(1),
        }));

        let mut pool = Vec::with_capacity(config.thread_pool_size);

        for thread in 0..config.thread_pool_size {
            let ds = datastore.clone();
            let st = state.clone();
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

            let handle = thread::spawn(move || {
                run_transaction_thread(ds, st, rx);
            });

            pool.push((handle, tx));
        }

        Ok(Self {
            datastore,
            state,
            pool: Arc::new(pool),
            pool_rr: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub async fn transaction(
        &self,
        mode: TransactionMode,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let tx_id = self
            .state
            .write()
            .await
            .tx_id_allocator
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst) as u64;

        // round robin
        let idx = self
            .pool_rr
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            % self.pool.len();
        let (_, thread_tx) = &self.pool[idx];
        thread_tx.send(TransactionCommand::StartTransaction {
            mode: mode.clone(),
            tx_id,
        })?;

        Ok(Transaction {
            tx_id,
            mode,
            start_ts: self.datastore.get_visible_ts()?,
            tx: thread_tx.clone(),
        })
    }
}
