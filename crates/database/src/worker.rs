use std::{collections::HashMap, sync::Arc};

use storage::traits::Datastore;
use tokio::sync::RwLock;

use crate::{command::TransactionCommand, state::DatabaseState};

pub(crate) fn run_transaction_thread(
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
