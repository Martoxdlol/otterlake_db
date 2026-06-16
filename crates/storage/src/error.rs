use std::{fmt, io, result};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),

    #[error(transparent)]
    Implementation(ImplementationError),
}

pub type Result<T> = result::Result<T, Error>;

impl From<heed3::Error> for Error {
    fn from(e: heed3::Error) -> Self {
        Self::implementation_with_source("heed storage implementation error", e)
    }
}

#[derive(Debug)]
pub struct ImplementationError {
    message: String,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl fmt::Display for ImplementationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ImplementationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

impl Error {
    pub fn implementation(message: impl Into<String>) -> Self {
        Self::Implementation(ImplementationError {
            message: message.into(),
            source: None,
        })
    }

    pub fn implementation_with_source(
        message: impl Into<String>,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::Implementation(ImplementationError {
            message: message.into(),
            source: Some(source.into()),
        })
    }
}
