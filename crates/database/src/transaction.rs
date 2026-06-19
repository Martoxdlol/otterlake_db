use storage::types::{CollectionId, DocumentId};
use tokio::sync::{mpsc::UnboundedSender, oneshot};

use crate::{
    command::{CommitOutput, TransactionCommand},
    document::Document,
    query::Query,
};

#[derive(Clone)]
pub enum TransactionMode {
    ReadOnly,
    ReadWrite,
}

pub struct Collection {
    collection_id: CollectionId,
}

impl Collection {
    pub async fn get<T>(self, document_id: DocumentId) -> crate::Result<Option<T>> {
        todo!()
    }
}

pub struct Transaction {
    pub(crate) tx_id: u64,
    pub(crate) start_ts: u64,
    pub(crate) mode: TransactionMode,
    pub(crate) tx: UnboundedSender<TransactionCommand>,
}

impl Transaction {
    pub async fn commit(self) -> crate::Result<CommitOutput> {
        let (tx, rx) = oneshot::channel();

        self.tx.send(TransactionCommand::CommitTransaction {
            tx_id: self.tx_id,
            tx,
        })?;

        // Outer `?` maps a dropped responder to `WorkerUnavailable`; the inner
        // `Result` carries commit failures such as `Conflict`.
        rx.await?
    }

    pub async fn collection(&self) -> crate::Result<Collection> {
        Ok(Collection {
            collection_id: todo!(),
        })
    }

    pub(crate) async fn query(&self, query: Query) -> crate::Result<Vec<Document>> {
        let (tx, rx) = oneshot::channel();

        self.tx.send(TransactionCommand::Query {
            tx_id: self.tx_id,
            tx: tx,
            query,
        })?;

        rx.await?
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
