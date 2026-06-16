use std::io;

use crate::types::{CollectionId, DocumentId, IndexId, Value};

pub const I64_LEN: usize = 8;
pub const U64_LEN: usize = 8;
pub const U128_LEN: usize = 16;
pub const DOCUMENT_KEY_LEN: usize = I64_LEN + U128_LEN + U64_LEN;
pub const DOCUMENT_PREFIX_LEN: usize = I64_LEN + U128_LEN;
pub const DOCUMENT_TOMBSTONE: &[u8] = &[0x00];
pub const DOCUMENT_VALUE_PREFIX: u8 = 0x01;

pub const ROOT_INDEX_CHAIN_ID: u64 = 0;
pub const INDEX_NODE_KEY_PREFIX_LEN: usize = I64_LEN + U64_LEN;
pub const MAX_INDEX_SEGMENT_SIZE: usize = 511 - INDEX_NODE_KEY_PREFIX_LEN;

pub fn decode_i64(bytes: &[u8]) -> crate::error::Result<i64> {
    Ok(i64::from_be_bytes(fixed(bytes, "stored i64 value")?))
}

pub fn decode_u64(bytes: &[u8]) -> crate::error::Result<u64> {
    Ok(u64::from_be_bytes(fixed(bytes, "stored u64 value")?))
}

pub fn decode_u128(bytes: &[u8]) -> crate::error::Result<u128> {
    Ok(u128::from_be_bytes(fixed(bytes, "stored u128 value")?))
}

pub fn encode_collection_catalog_value(id: CollectionId, metadata: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(I64_LEN + metadata.len());
    bytes.extend_from_slice(&id.to_be_bytes());
    bytes.extend_from_slice(metadata);
    bytes
}

pub fn decode_collection_catalog_value(
    bytes: &[u8],
) -> crate::error::Result<(CollectionId, Value)> {
    if bytes.len() < I64_LEN {
        return Err(invalid_data(
            "collection catalog value is shorter than 8 bytes",
        ));
    }

    let id = decode_i64(&bytes[..I64_LEN])?;
    Ok((id, bytes[I64_LEN..].to_vec()))
}

pub fn encode_index_catalog_key(collection_id: CollectionId, name: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(I64_LEN + name.len());
    bytes.extend_from_slice(&collection_id.to_be_bytes());
    bytes.extend_from_slice(name.as_bytes());
    bytes
}

pub fn decode_index_catalog_key(bytes: &[u8]) -> crate::error::Result<(CollectionId, String)> {
    if bytes.len() < I64_LEN {
        return Err(invalid_data("index catalog key is shorter than 8 bytes"));
    }

    let collection_id = decode_i64(&bytes[..I64_LEN])?;
    let name = decode_utf8(&bytes[I64_LEN..])?;
    Ok((collection_id, name))
}

pub fn encode_index_catalog_value(id: IndexId, metadata: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(I64_LEN + metadata.len());
    bytes.extend_from_slice(&id.to_be_bytes());
    bytes.extend_from_slice(metadata);
    bytes
}

pub fn decode_index_catalog_value(bytes: &[u8]) -> crate::error::Result<(IndexId, Value)> {
    if bytes.len() < I64_LEN {
        return Err(invalid_data("index catalog value is shorter than 8 bytes"));
    }

    let id = decode_i64(&bytes[..I64_LEN])?;
    Ok((id, bytes[I64_LEN..].to_vec()))
}

pub fn encode_collection_prefix(collection_id: CollectionId) -> Vec<u8> {
    collection_id.to_be_bytes().to_vec()
}

pub fn encode_document_prefix(collection_id: CollectionId, document_id: DocumentId) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(DOCUMENT_PREFIX_LEN);
    bytes.extend_from_slice(&collection_id.to_be_bytes());
    bytes.extend_from_slice(&document_id.to_be_bytes());
    bytes
}

pub fn encode_document_key(
    collection_id: CollectionId,
    document_id: DocumentId,
    version: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(DOCUMENT_KEY_LEN);
    bytes.extend_from_slice(&collection_id.to_be_bytes());
    bytes.extend_from_slice(&document_id.to_be_bytes());
    bytes.extend_from_slice(&version.to_be_bytes());
    bytes
}

pub fn decode_document_key(bytes: &[u8]) -> crate::error::Result<(CollectionId, DocumentId, u64)> {
    if bytes.len() != DOCUMENT_KEY_LEN {
        return Err(invalid_data("document key is not 32 bytes"));
    }

    let collection_id = decode_i64(&bytes[..I64_LEN])?;
    let document_id = decode_u128(&bytes[I64_LEN..I64_LEN + U128_LEN])?;
    let version = decode_u64(&bytes[I64_LEN + U128_LEN..])?;
    Ok((collection_id, document_id, version))
}

pub fn encode_document_value(value: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + value.len());
    bytes.push(DOCUMENT_VALUE_PREFIX);
    bytes.extend_from_slice(value);
    bytes
}

pub fn decode_document_value(bytes: &[u8]) -> crate::error::Result<Option<Value>> {
    match bytes.split_first() {
        Some((&DOCUMENT_VALUE_PREFIX, value)) => Ok(Some(value.to_vec())),
        Some((&0x00, [])) => Ok(None),
        Some((&0x00, _)) => Err(invalid_data("document tombstone has trailing bytes")),
        Some(_) => Err(invalid_data("document value has an unknown prefix")),
        None => Err(invalid_data("document value is empty")),
    }
}

pub fn encode_vacuum_target_key(collection_id: CollectionId, document_id: DocumentId) -> Vec<u8> {
    encode_document_prefix(collection_id, document_id)
}

pub fn encode_index_node_key(index_id: IndexId, chain_id: u64, segment: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(INDEX_NODE_KEY_PREFIX_LEN + segment.len());
    bytes.extend_from_slice(&index_id.to_be_bytes());
    bytes.extend_from_slice(&chain_id.to_be_bytes());
    bytes.extend_from_slice(segment);
    bytes
}

pub fn decode_index_node_key(bytes: &[u8]) -> crate::error::Result<(IndexId, u64, Value)> {
    if bytes.len() < INDEX_NODE_KEY_PREFIX_LEN {
        return Err(invalid_data("index node key is shorter than 16 bytes"));
    }

    let index_id = decode_i64(&bytes[..I64_LEN])?;
    let chain_id = decode_u64(&bytes[I64_LEN..INDEX_NODE_KEY_PREFIX_LEN])?;
    Ok((
        index_id,
        chain_id,
        bytes[INDEX_NODE_KEY_PREFIX_LEN..].to_vec(),
    ))
}

pub fn index_node_prefix(index_id: IndexId, chain_id: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(INDEX_NODE_KEY_PREFIX_LEN);
    bytes.extend_from_slice(&index_id.to_be_bytes());
    bytes.extend_from_slice(&chain_id.to_be_bytes());
    bytes
}

pub fn encode_index_document_id(document_id: DocumentId) -> [u8; U128_LEN] {
    document_id.to_be_bytes()
}

pub fn decode_index_document_id(bytes: &[u8]) -> crate::error::Result<DocumentId> {
    decode_u128(bytes)
}

pub fn split_index_key(index_key: &[u8]) -> Vec<&[u8]> {
    if index_key.is_empty() {
        return vec![&[]];
    }

    index_key.chunks(MAX_INDEX_SEGMENT_SIZE).collect()
}

pub fn decode_utf8(bytes: &[u8]) -> crate::error::Result<String> {
    let value = std::str::from_utf8(bytes).map_err(|e| {
        crate::error::Error::implementation_with_source("stored string is not utf-8", e)
    })?;
    Ok(value.to_owned())
}

pub fn invalid_data(message: &'static str) -> crate::error::Error {
    io::Error::new(io::ErrorKind::InvalidData, message).into()
}

fn fixed<const N: usize>(bytes: &[u8], name: &'static str) -> crate::error::Result<[u8; N]> {
    bytes.try_into().map_err(|_| {
        invalid_data(match N {
            8 => "stored 8-byte integer has invalid length",
            16 => "stored 16-byte integer has invalid length",
            _ => name,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_segment_keys_fit_lmdb_limit() {
        let segment = vec![0; MAX_INDEX_SEGMENT_SIZE];
        assert_eq!(MAX_INDEX_SEGMENT_SIZE, 495);
        assert!(encode_index_node_key(1, ROOT_INDEX_CHAIN_ID, &segment).len() <= 511);
    }

    #[test]
    fn document_keys_round_trip() {
        let key = encode_document_key(7, 42, 99);
        assert_eq!(decode_document_key(&key).unwrap(), (7, 42, 99));
    }

    #[test]
    fn index_node_keys_round_trip_empty_segment() {
        let key = encode_index_node_key(2, ROOT_INDEX_CHAIN_ID, &[]);
        assert_eq!(
            decode_index_node_key(&key).unwrap(),
            (2, ROOT_INDEX_CHAIN_ID, Vec::new())
        );
    }

    #[test]
    fn split_index_key_keeps_empty_key_addressable() {
        assert_eq!(split_index_key(&[]), vec![&[] as &[u8]]);
    }
}
