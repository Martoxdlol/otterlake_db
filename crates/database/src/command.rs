use storage::types::{CollectionId, DocumentId};
use tokio::sync::{mpsc::error::SendError, oneshot};

use crate::{
    document::Document,
    error::Error,
    query::Query,
    transaction::TransactionMode,
};

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
        tx: oneshot::Sender<crate::Result<CommitOutput>>,
        // commit or end (for read and rw transactions)
    },
    RollbackTransaction {
        tx_id: u64,
        tx: oneshot::Sender<RollbackTransactionOutput>,
        // only for rw transactions
    },
    GetCollection {
        tx_id: u64,
        tx: oneshot::Sender<crate::Result<CollectionId>>,
        name: String,
    },
    Get {
        tx_id: u64,
        tx: oneshot::Sender<crate::Result<Option<Document>>>,
        collection_id: CollectionId,
        document_id: DocumentId,
    },
    Query {
        tx_id: u64,
        tx: oneshot::Sender<crate::Result<Vec<Document>>>,
        query: Query,
    },
    Write {
        tx_id: u64,
        tx: oneshot::Sender<crate::Result<()>>,
        // write query
    },
}

/// Failure to enqueue a command means the worker thread has gone away.
impl From<SendError<TransactionCommand>> for Error {
    fn from(_: SendError<TransactionCommand>) -> Self {
        Error::WorkerUnavailable
    }
}
