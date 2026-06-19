use std::{collections::HashMap, sync::Arc};

use storage::traits::{Datastore, DatastoreTransaction};
use tokio::sync::RwLock;

use crate::{
    TransactionMode, command::TransactionCommand, read_set::ReadSet, state::DatabaseState,
    write_set::WriteSet,
};

struct TransactionContext<'a, D: Datastore + 'a> {
    tx_id: u64,
    mode: TransactionMode,
    transaction: Option<D::Transaction<'a>>,
    read_set: Option<ReadSet>,
    write_set: Option<WriteSet>,
}

pub(crate) fn run_transaction_thread<D: Datastore>(
    ds: D,
    st: Arc<RwLock<DatabaseState>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<TransactionCommand>,
) {
    let mut transactions: HashMap<u64, TransactionContext<'_, D>> = HashMap::new();
    // iterate over channel reading commands (start transaction, end transaction, read/write within transaction)
    // execute against datastore
    // put result into response channel of the corresponding transaction
    while let Some(cmd) = rx.blocking_recv() {
        match cmd {
            TransactionCommand::StartTransaction { mode, tx_id } => {
                transactions.insert(
                    tx_id,
                    TransactionContext {
                        tx_id,
                        mode,
                        transaction: None,
                        read_set: Some(ReadSet::new()),
                        write_set: Some(WriteSet::new()),
                    },
                );
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
