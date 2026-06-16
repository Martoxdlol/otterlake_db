use std::io;

use crate::types::{CollectionId, DocumentId};

pub fn decode_i64(bytes: &[u8]) -> crate::error::Result<i64> {
    let bytes = bytes.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "stored i64 value is not 8 bytes",
        )
    })?;
    Ok(i64::from_be_bytes(bytes))
}

pub fn decode_u64(bytes: &[u8]) -> crate::error::Result<u64> {
    let bytes = bytes.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "stored u64 value is not 8 bytes",
        )
    })?;
    Ok(u64::from_be_bytes(bytes))
}

pub fn encode_document_key(
    collection_id: CollectionId,
    document_id: DocumentId,
    version: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(&collection_id.to_be_bytes());
    bytes.extend_from_slice(&document_id.to_be_bytes());
    bytes.extend_from_slice(&version.to_be_bytes());
    bytes
}

pub fn encode_vacuum_target_key(collection_id: CollectionId, document_id: DocumentId) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(24);
    bytes.extend_from_slice(&collection_id.to_be_bytes());
    bytes.extend_from_slice(&document_id.to_be_bytes());
    bytes
}
