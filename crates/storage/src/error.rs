use std::{io, result};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

pub type Result<T> = result::Result<T, Error>;

impl From<heed3::Error> for Error {
    fn from(e: heed3::Error) -> Self {
        Self::Other(Box::new(e))
    }
}
