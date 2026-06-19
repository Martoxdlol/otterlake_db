use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, atomic::AtomicUsize},
    thread,
};

use storage::traits::Datastore;
use tokio::sync::{RwLock, mpsc::UnboundedSender};

use crate::{
    command::TransactionCommand,
    config::Config,
    state::DatabaseState,
    transaction::{Transaction, TransactionMode},
    worker::run_transaction_thread,
};

pub struct Database<T: Datastore> {
    datastore: T,

    pool: Arc<Vec<(thread::JoinHandle<()>, UnboundedSender<TransactionCommand>)>>,
    pool_rr: Arc<AtomicUsize>,

    state: Arc<RwLock<DatabaseState>>,
}

impl<T: Datastore + 'static> Database<T> {
    pub async fn new(datastore: T, config: Config) -> crate::Result<Self> {
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

        for _ in 0..config.thread_pool_size {
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

    pub async fn transaction(&self, mode: TransactionMode) -> crate::Result<Transaction> {
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
