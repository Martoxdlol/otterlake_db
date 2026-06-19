use storage::types::{CollectionId, DocumentId};
use tokio::sync::oneshot;

use crate::{
    query::{Document, Query},
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
        tx: oneshot::Sender<CommitOutput>,
        // commit or end (for read and rw transactions)
    },
    RollbackTransaction {
        tx_id: u64,
        tx: oneshot::Sender<RollbackTransactionOutput>,
        // only for rw transactions
    },
    GetCollection {
        tx_id: u64,
        tx: oneshot::Sender<Result<CollectionId, Box<dyn std::error::Error>>>,
        name: String,
    },
    Get {
        tx_id: u64,
        tx: oneshot::Sender<Result<Option<Document>, Box<dyn std::error::Error>>>,
        collection_id: CollectionId,
        document_id: DocumentId,
    },
    Query {
        tx_id: u64,
        tx: oneshot::Sender<Result<Vec<Document>, Box<dyn std::error::Error>>>,
        query: Query,
    },
    Write {
        tx_id: u64,
        tx: oneshot::Sender<Result<(), Box<dyn std::error::Error>>>,
        // write query
    },
}
