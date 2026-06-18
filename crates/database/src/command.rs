use tokio::sync::oneshot;

use crate::transaction::TransactionMode;

pub struct EndTransactionOutput {
    // commit or end (for read and rw transactions)
}

pub struct RollbackTransactionOutput {
    // only for rw transactions
}

pub struct CommitOutput {
    // only for rw transactions
}

pub(crate) enum TransactionCommand {
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
