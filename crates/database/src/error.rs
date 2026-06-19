use std::result;

use tokio::sync::oneshot;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An error propagated from the underlying storage engine.
    #[error(transparent)]
    Storage(#[from] storage::error::Error),

    /// A command could not be delivered to (or a response received from) the
    /// transaction worker thread, meaning the worker is gone / shutting down.
    #[error("transaction worker is unavailable")]
    WorkerUnavailable,

    /// A transaction id was used after it was ended, or never existed.
    #[error("transaction {0} not found")]
    TransactionNotFound(u64),

    /// A referenced collection does not exist.
    #[error("collection not found: {0}")]
    CollectionNotFound(String),

    /// The transaction failed to commit because its read set conflicts with a
    /// concurrently committed transaction. The caller may retry.
    #[error("transaction conflict")]
    Conflict,

    /// Escape hatch for errors that don't (yet) have a dedicated variant.
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

pub type Result<T> = result::Result<T, Error>;

/// A pending response is dropped by the worker without a value: treat as the
/// worker being unavailable.
impl From<oneshot::error::RecvError> for Error {
    fn from(_: oneshot::error::RecvError) -> Self {
        Error::WorkerUnavailable
    }
}
