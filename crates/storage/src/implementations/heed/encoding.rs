use std::io;

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
