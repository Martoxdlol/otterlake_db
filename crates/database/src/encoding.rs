//! Order-preserving key encoding.
//!
//! Converts entry field values into a byte-comparable representation for
//! B-tree index storage: comparing two encoded entries with `memcmp` yields
//! the same ordering as comparing the logical values field by field. The
//! encoding follows the type ordering and per-type rules from the docstore
//! D1 key-encoding design.
//!
//! The encoding is self-delimiting — each field carries a leading type tag
//! and (for variable-length fields) a terminator — so a decoder could walk
//! a compound entry field by field. Only the encode side lives here.
//!
//! ## Type tags (ascending order)
//!
//! | Tag    | Field                          |
//! |--------|--------------------------------|
//! | `0x00` | Undefined (an absent field)    |
//! | `0x01` | Null                           |
//! | `0x02` | Int64                          |
//! | `0x03` | Float64                        |
//! | `0x04` | Boolean                        |
//! | `0x05` | String                         |
//! | `0x06` | Bytes                          |
//!
//! [`Value::Array`] and [`Value::Document`] are composite and have no place in
//! an index key; encoding one is an [`EncodingError::NotAScalar`].

use crate::Value;

/// The sign bit of a 64-bit big-endian number.
const SIGN_BIT: u64 = 0x8000_0000_0000_0000;

/// Canonical bit pattern for a quiet NaN. All NaN payloads collapse to this so
/// every NaN encodes identically and sorts as a single value.
const CANONICAL_NAN_BITS: u64 = 0x7FF8_0000_0000_0000;

const TAG_UNDEFINED: u8 = 0x00;
const TAG_NULL: u8 = 0x01;
const TAG_INT64: u8 = 0x02;
const TAG_FLOAT64: u8 = 0x03;
const TAG_BOOL: u8 = 0x04;
const TAG_STRING: u8 = 0x05;
const TAG_BYTES: u8 = 0x06;

/// An error produced while encoding a value into an order-preserving key.
#[derive(Debug, thiserror::Error)]
pub enum EncodingError {
    /// An entry field held a composite value (an array or a document). Index
    /// keys are built from scalar fields only.
    #[error("cannot encode a {kind} value into an index key")]
    NotAScalar { kind: &'static str },
}

/// Encode a compound entry into an order-preserving byte key.
///
/// Each element is one field of the entry, encoded in order and concatenated.
/// A `None` field is encoded as Undefined (`0x00`), which sorts before every
/// present value — this is how an optional/missing index field is represented.
///
/// Returns [`EncodingError::NotAScalar`] if any field is a [`Value::Array`] or
/// [`Value::Document`].
pub fn encode_entry(entry: &[Option<&Value>]) -> Result<Vec<u8>, EncodingError> {
    let mut buf = Vec::new();
    for field in entry {
        match field {
            None => buf.push(TAG_UNDEFINED),
            Some(value) => encode_value(&mut buf, value)?,
        }
    }
    Ok(buf)
}

/// Append the order-preserving encoding of a single scalar value to `buf`.
fn encode_value(buf: &mut Vec<u8>, value: &Value) -> Result<(), EncodingError> {
    match value {
        Value::Null => buf.push(TAG_NULL),
        Value::I64(v) => {
            buf.push(TAG_INT64);
            // Flip the sign bit so two's-complement ints sort correctly as
            // unsigned big-endian: i64::MIN -> 0x00.., 0 -> 0x80.., i64::MAX -> 0xFF..
            let bits = (*v as u64) ^ SIGN_BIT;
            buf.extend_from_slice(&bits.to_be_bytes());
        }
        Value::F64(v) => {
            buf.push(TAG_FLOAT64);
            // Collapse -0.0 to 0.0 and every NaN to one pattern, so equal
            // values encode identically.
            let f = if v.is_nan() {
                f64::from_bits(CANONICAL_NAN_BITS)
            } else if *v == 0.0 {
                0.0
            } else {
                *v
            };
            let bits = f.to_bits();
            // Positive: flip just the sign bit. Negative: flip all bits. This
            // orders -inf < .. < -0.0 < 0.0 < .. < +inf < NaN.
            let ordered = if bits & SIGN_BIT == 0 {
                bits ^ SIGN_BIT
            } else {
                !bits
            };
            buf.extend_from_slice(&ordered.to_be_bytes());
        }
        Value::Bool(v) => buf.extend_from_slice(&[TAG_BOOL, *v as u8]),
        Value::String(s) => {
            buf.push(TAG_STRING);
            encode_escaped(buf, s.as_bytes());
        }
        Value::Bytes(b) => {
            buf.push(TAG_BYTES);
            encode_escaped(buf, b);
        }
        Value::Array(_) | Value::Document(_) => {
            return Err(EncodingError::NotAScalar { kind: value.kind() });
        }
    }
    Ok(())
}

/// Append `bytes` with `0x00` escaped as `0x00 0xFF`, followed by the `0x00
/// 0x00` terminator. Escaping keeps a literal zero byte from being mistaken
/// for the terminator while preserving lexicographic order.
fn encode_escaped(buf: &mut Vec<u8>, bytes: &[u8]) {
    for &byte in bytes {
        if byte == 0x00 {
            buf.extend_from_slice(&[0x00, 0xFF]);
        } else {
            buf.push(byte);
        }
    }
    buf.extend_from_slice(&[0x00, 0x00]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(value: Value) -> Vec<u8> {
        encode_entry(&[Some(&value)]).unwrap()
    }

    #[test]
    fn undefined_sorts_before_null() {
        let undefined = encode_entry(&[None]).unwrap();
        let null = enc(Value::Null);
        assert_eq!(undefined, vec![TAG_UNDEFINED]);
        assert!(undefined < null);
    }

    #[test]
    fn int64_ordering_matches_numeric() {
        let values = [i64::MIN, -2, -1, 0, 1, 2, i64::MAX];
        let mut prev: Option<Vec<u8>> = None;
        for v in values {
            let cur = enc(Value::I64(v));
            if let Some(prev) = &prev {
                assert!(prev < &cur, "{v} did not sort after its predecessor");
            }
            prev = Some(cur);
        }
    }

    #[test]
    fn float64_ordering() {
        let values = [
            f64::NEG_INFINITY,
            -1.0,
            -0.0,
            0.0,
            1.0,
            f64::INFINITY,
            f64::NAN,
        ];
        let mut prev: Option<Vec<u8>> = None;
        for v in values {
            let cur = enc(Value::F64(v));
            if let Some(prev) = &prev {
                assert!(prev <= &cur, "{v} broke float ordering");
            }
            prev = Some(cur);
        }
    }

    #[test]
    fn negative_zero_encodes_like_positive_zero() {
        assert_eq!(enc(Value::F64(-0.0)), enc(Value::F64(0.0)));
    }

    #[test]
    fn all_nans_canonicalize_to_one_encoding() {
        let quiet = f64::from_bits(0x7FF8_0000_0000_0001);
        let signaling = f64::from_bits(0x7FF0_0000_0000_0001);
        let negative = f64::from_bits(0xFFF8_0000_0000_0001);
        let expected = enc(Value::F64(f64::NAN));
        assert_eq!(enc(Value::F64(quiet)), expected);
        assert_eq!(enc(Value::F64(signaling)), expected);
        assert_eq!(enc(Value::F64(negative)), expected);
    }

    #[test]
    fn bool_false_sorts_before_true() {
        assert!(enc(Value::Bool(false)) < enc(Value::Bool(true)));
    }

    #[test]
    fn string_ordering_is_lexicographic() {
        let values = ["", "a", "aa", "ab", "b"];
        let mut prev: Option<Vec<u8>> = None;
        for v in values {
            let cur = enc(Value::String(v.to_string()));
            if let Some(prev) = &prev {
                assert!(prev < &cur, "{v:?} broke string ordering");
            }
            prev = Some(cur);
        }
    }

    #[test]
    fn string_terminator_and_escaping() {
        // "a\0b" -> tag, 'a', escaped null, 'b', terminator
        let encoded = enc(Value::String("a\u{0}b".to_string()));
        assert_eq!(encoded, vec![TAG_STRING, b'a', 0x00, 0xFF, b'b', 0x00, 0x00]);
    }

    #[test]
    fn cross_type_ordering_follows_tags() {
        let ordered = [
            encode_entry(&[None]).unwrap(),
            enc(Value::Null),
            enc(Value::I64(0)),
            enc(Value::F64(0.0)),
            enc(Value::Bool(false)),
            enc(Value::String("".to_string())),
            enc(Value::Bytes(vec![])),
        ];
        for pair in ordered.windows(2) {
            assert!(pair[0] < pair[1], "cross-type ordering violated");
        }
    }

    #[test]
    fn compound_entry_is_the_concatenation_of_fields() {
        let a = Value::I64(1);
        let b = Value::String("hello".to_string());
        let compound = encode_entry(&[Some(&a), Some(&b)]).unwrap();

        let mut expected = enc(Value::I64(1));
        expected.extend(enc(Value::String("hello".to_string())));
        assert_eq!(compound, expected);
    }

    #[test]
    fn array_and_document_are_rejected() {
        assert!(matches!(
            encode_entry(&[Some(&Value::Array(vec![]))]),
            Err(EncodingError::NotAScalar { kind: "array" })
        ));
        assert!(matches!(
            encode_entry(&[Some(&Value::Document(Default::default()))]),
            Err(EncodingError::NotAScalar { kind: "document" })
        ));
    }
}
