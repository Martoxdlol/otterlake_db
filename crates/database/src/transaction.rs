use storage::types::{CollectionId, DocumentId};
use tokio::sync::{mpsc::UnboundedSender, oneshot};

use serde::de::DeserializeOwned;

use crate::{
    command::{CommitOutput, TransactionCommand},
    document::{Document, RawDocument, from_document_with_id},
    query::{Query, builder::QueryBuilder},
};

#[derive(Clone)]
pub enum TransactionMode {
    ReadOnly,
    ReadWrite,
}

pub struct Collection<'a> {
    transaction: &'a Transaction,
    pub(crate) collection_id: CollectionId,
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

    pub async fn collection(&self, name: String) -> crate::Result<Collection> {
        let (tx, rx) = oneshot::channel();

        self.tx.send(TransactionCommand::GetCollection {
            tx_id: self.tx_id,
            tx: tx,
            name: name,
        })?;

        let coll_id = (rx.await?)?;

        Ok(Collection {
            transaction: self,
            collection_id: coll_id,
        })
    }

    pub(crate) async fn get_document(
        &self,
        collection_id: CollectionId,
        document_id: DocumentId,
    ) -> crate::Result<Option<RawDocument>> {
        let (tx, rx) = oneshot::channel();

        self.tx.send(TransactionCommand::Get {
            tx_id: self.tx_id,
            tx: tx,
            collection_id,
            document_id,
        })?;

        rx.await?
    }

    pub fn query<S: Into<String>>(&'_ self, collection_name: S) -> QueryBuilder<'_> {
        QueryBuilder::new(self, collection_name.into())
    }

    pub(crate) async fn run_query(&self, query: Query) -> crate::Result<Vec<RawDocument>> {
        let (tx, rx) = oneshot::channel();

        self.tx.send(TransactionCommand::Query {
            tx_id: self.tx_id,
            tx: tx,
            query,
        })?;

        rx.await?
    }

    pub(crate) fn mock() -> Self {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            tx_id: 0,
            start_ts: 0,
            mode: TransactionMode::ReadOnly,
            tx,
        }
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
