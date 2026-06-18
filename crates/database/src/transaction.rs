use tokio::sync::{mpsc::UnboundedSender, oneshot};

use crate::command::{CommitOutput, TransactionCommand};

#[derive(Clone)]
pub enum TransactionMode {
    ReadOnly,
    ReadWrite,
}

pub struct Transaction {
    pub(crate) tx_id: u64,
    pub(crate) start_ts: u64,
    pub(crate) mode: TransactionMode,
    pub(crate) tx: UnboundedSender<TransactionCommand>,
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
