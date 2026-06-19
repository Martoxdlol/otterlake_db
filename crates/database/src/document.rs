//! The document data model and its serde bridge.
//!
//! A stored document is, at its root, always a key/value map — never a bare
//! scalar or sequence. [`to_document`] enforces this at serialization time and
//! additionally rejects a root that carries an `_id` field, because `_id` lives
//! outside the encoded blob (it is the storage [`DocumentId`]). On the way back
//! out, [`from_document_with_id`] injects the id under the `_id` key before
//! deserializing, so a target struct can receive it.

use std::fmt::{self, Display};
use std::mem;

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde::ser::{self, Impossible};
use serde::{Deserialize, Serialize, forward_to_deserialize_any};
use storage::types::DocumentId;

/// The raw, encoded form of a document as it crosses the storage boundary:
/// the id bytes plus the opaque encoded blob.
pub struct RawDocument {
    pub _id: Vec<u8>,
    pub data: Vec<u8>,
}

/// A single value in the document data model.
///
/// This mirrors the shape of what the store can hold. `_id` values are carried
/// as [`Value::Bytes`] (the 16 big-endian bytes of a UUIDv7), which decodes
/// cleanly into `uuid::Uuid` or raw bytes on the read side.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Document(Document),
}

/// An ordered key/value map — the only shape allowed at the root of a document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    entries: Vec<(String, Value)>,
}

impl Document {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    /// Insert a key, replacing and returning any existing value for that key.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) -> Option<Value> {
        let key = key.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(mem::replace(&mut slot.1, value))
        } else {
            self.entries.push((key, value));
            None
        }
    }

    /// Insert a key at the front of the map, removing any prior occurrence.
    pub fn insert_front(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        self.remove(&key);
        self.entries.insert(0, (key, value));
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.entries
            .iter()
            .position(|(k, _)| k == key)
            .map(|pos| self.entries.remove(pos).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// Return this document with the storage id injected as the leading `_id`
    /// field (16 big-endian bytes of the UUIDv7).
    pub fn with_id(mut self, id: DocumentId) -> Self {
        self.insert_front("_id", Value::Bytes(id.to_be_bytes().to_vec()));
        self
    }
}

impl IntoIterator for Document {
    type Item = (String, Value);
    type IntoIter = std::vec::IntoIter<(String, Value)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

/// Errors produced while converting between application types and documents.
#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    /// The root of a document serialized to something other than a map.
    #[error("the root of a document must be a key/value map")]
    RootNotDocument,

    /// A document carried an `_id` field on the write path. `_id` is assigned
    /// by storage and must not be part of the document blob.
    #[error("documents must not contain an `_id` field on write")]
    IdNotAllowed,

    /// A map key serialized to something that is not a string.
    #[error("document map keys must be strings")]
    NonStringKey,

    /// A free-form message produced by serde.
    #[error("{0}")]
    Message(String),
}

impl ser::Error for DocumentError {
    fn custom<T: Display>(msg: T) -> Self {
        DocumentError::Message(msg.to_string())
    }
}

impl de::Error for DocumentError {
    fn custom<T: Display>(msg: T) -> Self {
        DocumentError::Message(msg.to_string())
    }
}

/// Serialize any value into the document data model.
pub fn to_value<T>(value: &T) -> Result<Value, DocumentError>
where
    T: ?Sized + Serialize,
{
    value.serialize(Serializer)
}

/// Serialize a value into a [`Document`], enforcing the document-store
/// invariants: the root must be a map, and it must not carry an `_id` field.
pub fn to_document<T>(value: &T) -> Result<Document, DocumentError>
where
    T: ?Sized + Serialize,
{
    match to_value(value)? {
        Value::Document(doc) => {
            if doc.contains_key("_id") {
                return Err(DocumentError::IdNotAllowed);
            }
            Ok(doc)
        }
        _ => Err(DocumentError::RootNotDocument),
    }
}

/// Deserialize a value out of a [`Document`].
pub fn from_document<T>(document: Document) -> Result<T, DocumentError>
where
    T: DeserializeOwned,
{
    T::deserialize(Value::Document(document))
}

/// Deserialize a value out of a [`Document`] after injecting the storage id as
/// the `_id` field. This is the read path: the blob never stores `_id`, so it
/// is grafted back on here.
pub fn from_document_with_id<T>(document: Document, id: DocumentId) -> Result<T, DocumentError>
where
    T: DeserializeOwned,
{
    from_document(document.with_id(id))
}

// ---------------------------------------------------------------------------
// Serialize / Deserialize for the model's own types (so they can nest and be
// targets of `to_document` / `from_document`).
// ---------------------------------------------------------------------------

impl Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::I64(n) => serializer.serialize_i64(*n),
            Value::F64(f) => serializer.serialize_f64(*f),
            Value::String(s) => serializer.serialize_str(s),
            Value::Bytes(b) => serializer.serialize_bytes(b),
            Value::Array(a) => a.serialize(serializer),
            Value::Document(d) => d.serialize(serializer),
        }
    }
}

impl Serialize for Document {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap as _;
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (k, v) in &self.entries {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValueVisitor;

        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = Value;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any document value")
            }

            fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
                Ok(Value::Bool(v))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
                Ok(Value::I64(v))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
                Ok(Value::I64(v as i64))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
                Ok(Value::F64(v))
            }
            fn visit_str<E>(self, v: &str) -> Result<Value, E> {
                Ok(Value::String(v.to_owned()))
            }
            fn visit_string<E>(self, v: String) -> Result<Value, E> {
                Ok(Value::String(v))
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Value, E> {
                Ok(Value::Bytes(v.to_owned()))
            }
            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Value, E> {
                Ok(Value::Bytes(v))
            }
            fn visit_none<E>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_unit<E>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Value, D::Error> {
                Value::deserialize(deserializer)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
                let mut vec = Vec::new();
                while let Some(elem) = seq.next_element()? {
                    vec.push(elem);
                }
                Ok(Value::Array(vec))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
                let mut doc = Document::new();
                while let Some((k, v)) = map.next_entry::<String, Value>()? {
                    doc.insert(k, v);
                }
                Ok(Value::Document(doc))
            }
        }

        deserializer.deserialize_any(ValueVisitor)
    }
}

impl<'de> Deserialize<'de> for Document {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::Document(doc) => Ok(doc),
            _ => Err(de::Error::custom("expected a document")),
        }
    }
}

// ---------------------------------------------------------------------------
// Serializer: turns any `T: Serialize` into a `Value`.
// ---------------------------------------------------------------------------

/// A serde [`serde::Serializer`] that produces a [`Value`].
pub struct Serializer;

impl serde::Serializer for Serializer {
    type Ok = Value;
    type Error = DocumentError;

    type SerializeSeq = ArraySerializer;
    type SerializeTuple = ArraySerializer;
    type SerializeTupleStruct = ArraySerializer;
    type SerializeTupleVariant = TupleVariantSerializer;
    type SerializeMap = MapSerializer;
    type SerializeStruct = StructSerializer;
    type SerializeStructVariant = StructVariantSerializer;

    // A binary store: emit compact (non-human-readable) forms, so e.g. UUIDs
    // serialize to their 16 bytes rather than a hyphenated string.
    fn is_human_readable(&self) -> bool {
        false
    }

    fn serialize_bool(self, v: bool) -> Result<Value, DocumentError> {
        Ok(Value::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<Value, DocumentError> {
        Ok(Value::I64(v as i64))
    }
    fn serialize_i16(self, v: i16) -> Result<Value, DocumentError> {
        Ok(Value::I64(v as i64))
    }
    fn serialize_i32(self, v: i32) -> Result<Value, DocumentError> {
        Ok(Value::I64(v as i64))
    }
    fn serialize_i64(self, v: i64) -> Result<Value, DocumentError> {
        Ok(Value::I64(v))
    }
    fn serialize_u8(self, v: u8) -> Result<Value, DocumentError> {
        Ok(Value::I64(v as i64))
    }
    fn serialize_u16(self, v: u16) -> Result<Value, DocumentError> {
        Ok(Value::I64(v as i64))
    }
    fn serialize_u32(self, v: u32) -> Result<Value, DocumentError> {
        Ok(Value::I64(v as i64))
    }
    fn serialize_u64(self, v: u64) -> Result<Value, DocumentError> {
        Ok(Value::I64(v as i64))
    }
    fn serialize_f32(self, v: f32) -> Result<Value, DocumentError> {
        Ok(Value::F64(v as f64))
    }
    fn serialize_f64(self, v: f64) -> Result<Value, DocumentError> {
        Ok(Value::F64(v))
    }
    fn serialize_char(self, v: char) -> Result<Value, DocumentError> {
        Ok(Value::String(v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<Value, DocumentError> {
        Ok(Value::String(v.to_owned()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Value, DocumentError> {
        Ok(Value::Bytes(v.to_owned()))
    }
    fn serialize_none(self) -> Result<Value, DocumentError> {
        Ok(Value::Null)
    }
    fn serialize_some<T>(self, value: &T) -> Result<Value, DocumentError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Value, DocumentError> {
        Ok(Value::Null)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, DocumentError> {
        Ok(Value::Null)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Value, DocumentError> {
        Ok(Value::String(variant.to_owned()))
    }
    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, DocumentError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, DocumentError>
    where
        T: ?Sized + Serialize,
    {
        let mut doc = Document::new();
        doc.insert(variant, to_value(value)?);
        Ok(Value::Document(doc))
    }
    fn serialize_seq(self, _len: Option<usize>) -> Result<ArraySerializer, DocumentError> {
        Ok(ArraySerializer { vec: Vec::new() })
    }
    fn serialize_tuple(self, len: usize) -> Result<ArraySerializer, DocumentError> {
        Ok(ArraySerializer {
            vec: Vec::with_capacity(len),
        })
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<ArraySerializer, DocumentError> {
        self.serialize_tuple(len)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<TupleVariantSerializer, DocumentError> {
        Ok(TupleVariantSerializer {
            variant,
            vec: Vec::with_capacity(len),
        })
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<MapSerializer, DocumentError> {
        Ok(MapSerializer {
            doc: Document::new(),
            next_key: None,
        })
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<StructSerializer, DocumentError> {
        Ok(StructSerializer {
            doc: Document::new(),
        })
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<StructVariantSerializer, DocumentError> {
        Ok(StructVariantSerializer {
            variant,
            doc: Document::new(),
        })
    }
}

pub struct ArraySerializer {
    vec: Vec<Value>,
}

impl ser::SerializeSeq for ArraySerializer {
    type Ok = Value;
    type Error = DocumentError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), DocumentError>
    where
        T: ?Sized + Serialize,
    {
        self.vec.push(to_value(value)?);
        Ok(())
    }
    fn end(self) -> Result<Value, DocumentError> {
        Ok(Value::Array(self.vec))
    }
}

impl ser::SerializeTuple for ArraySerializer {
    type Ok = Value;
    type Error = DocumentError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), DocumentError>
    where
        T: ?Sized + Serialize,
    {
        self.vec.push(to_value(value)?);
        Ok(())
    }
    fn end(self) -> Result<Value, DocumentError> {
        Ok(Value::Array(self.vec))
    }
}

impl ser::SerializeTupleStruct for ArraySerializer {
    type Ok = Value;
    type Error = DocumentError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), DocumentError>
    where
        T: ?Sized + Serialize,
    {
        self.vec.push(to_value(value)?);
        Ok(())
    }
    fn end(self) -> Result<Value, DocumentError> {
        Ok(Value::Array(self.vec))
    }
}

pub struct TupleVariantSerializer {
    variant: &'static str,
    vec: Vec<Value>,
}

impl ser::SerializeTupleVariant for TupleVariantSerializer {
    type Ok = Value;
    type Error = DocumentError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), DocumentError>
    where
        T: ?Sized + Serialize,
    {
        self.vec.push(to_value(value)?);
        Ok(())
    }
    fn end(self) -> Result<Value, DocumentError> {
        let mut doc = Document::new();
        doc.insert(self.variant, Value::Array(self.vec));
        Ok(Value::Document(doc))
    }
}

pub struct MapSerializer {
    doc: Document,
    next_key: Option<String>,
}

impl ser::SerializeMap for MapSerializer {
    type Ok = Value;
    type Error = DocumentError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), DocumentError>
    where
        T: ?Sized + Serialize,
    {
        self.next_key = Some(key.serialize(MapKeySerializer)?);
        Ok(())
    }
    fn serialize_value<T>(&mut self, value: &T) -> Result<(), DocumentError>
    where
        T: ?Sized + Serialize,
    {
        let key = self
            .next_key
            .take()
            .ok_or_else(|| <DocumentError as ser::Error>::custom("value serialized before key"))?;
        self.doc.insert(key, to_value(value)?);
        Ok(())
    }
    fn end(self) -> Result<Value, DocumentError> {
        Ok(Value::Document(self.doc))
    }
}

pub struct StructSerializer {
    doc: Document,
}

impl ser::SerializeStruct for StructSerializer {
    type Ok = Value;
    type Error = DocumentError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), DocumentError>
    where
        T: ?Sized + Serialize,
    {
        self.doc.insert(key, to_value(value)?);
        Ok(())
    }
    fn end(self) -> Result<Value, DocumentError> {
        Ok(Value::Document(self.doc))
    }
}

pub struct StructVariantSerializer {
    variant: &'static str,
    doc: Document,
}

impl ser::SerializeStructVariant for StructVariantSerializer {
    type Ok = Value;
    type Error = DocumentError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), DocumentError>
    where
        T: ?Sized + Serialize,
    {
        self.doc.insert(key, to_value(value)?);
        Ok(())
    }
    fn end(self) -> Result<Value, DocumentError> {
        let mut outer = Document::new();
        outer.insert(self.variant, Value::Document(self.doc));
        Ok(Value::Document(outer))
    }
}

/// Serializes a map key, accepting only string-like scalars.
struct MapKeySerializer;

impl serde::Serializer for MapKeySerializer {
    type Ok = String;
    type Error = DocumentError;

    type SerializeSeq = Impossible<String, DocumentError>;
    type SerializeTuple = Impossible<String, DocumentError>;
    type SerializeTupleStruct = Impossible<String, DocumentError>;
    type SerializeTupleVariant = Impossible<String, DocumentError>;
    type SerializeMap = Impossible<String, DocumentError>;
    type SerializeStruct = Impossible<String, DocumentError>;
    type SerializeStructVariant = Impossible<String, DocumentError>;

    fn serialize_str(self, v: &str) -> Result<String, DocumentError> {
        Ok(v.to_owned())
    }
    fn serialize_char(self, v: char) -> Result<String, DocumentError> {
        Ok(v.to_string())
    }
    fn serialize_bool(self, v: bool) -> Result<String, DocumentError> {
        Ok(v.to_string())
    }
    fn serialize_i8(self, v: i8) -> Result<String, DocumentError> {
        Ok(v.to_string())
    }
    fn serialize_i16(self, v: i16) -> Result<String, DocumentError> {
        Ok(v.to_string())
    }
    fn serialize_i32(self, v: i32) -> Result<String, DocumentError> {
        Ok(v.to_string())
    }
    fn serialize_i64(self, v: i64) -> Result<String, DocumentError> {
        Ok(v.to_string())
    }
    fn serialize_u8(self, v: u8) -> Result<String, DocumentError> {
        Ok(v.to_string())
    }
    fn serialize_u16(self, v: u16) -> Result<String, DocumentError> {
        Ok(v.to_string())
    }
    fn serialize_u32(self, v: u32) -> Result<String, DocumentError> {
        Ok(v.to_string())
    }
    fn serialize_u64(self, v: u64) -> Result<String, DocumentError> {
        Ok(v.to_string())
    }
    fn serialize_f32(self, _v: f32) -> Result<String, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_f64(self, _v: f64) -> Result<String, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_bytes(self, _v: &[u8]) -> Result<String, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_none(self) -> Result<String, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_some<T>(self, _value: &T) -> Result<String, DocumentError>
    where
        T: ?Sized + Serialize,
    {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_unit(self) -> Result<String, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<String, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<String, DocumentError> {
        Ok(variant.to_owned())
    }
    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<String, DocumentError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<String, DocumentError>
    where
        T: ?Sized + Serialize,
    {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, DocumentError> {
        Err(DocumentError::NonStringKey)
    }
}

// ---------------------------------------------------------------------------
// Deserializer: drives a `T: Deserialize` from a `Value`.
// ---------------------------------------------------------------------------

impl<'de> serde::Deserializer<'de> for Value {
    type Error = DocumentError;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DocumentError> {
        match self {
            Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(b),
            Value::I64(n) => visitor.visit_i64(n),
            Value::F64(f) => visitor.visit_f64(f),
            Value::String(s) => visitor.visit_string(s),
            Value::Bytes(b) => visitor.visit_byte_buf(b),
            Value::Array(arr) => visitor.visit_seq(SeqDeserializer {
                iter: arr.into_iter(),
            }),
            Value::Document(doc) => visitor.visit_map(MapDeserializer {
                iter: doc.into_iter(),
                value: None,
            }),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DocumentError> {
        match self {
            Value::Null => visitor.visit_none(),
            other => visitor.visit_some(other),
        }
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DocumentError> {
        match self {
            Value::Bytes(b) => visitor.visit_byte_buf(b),
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DocumentError> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, DocumentError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DocumentError> {
        let (variant, value) = match self {
            Value::String(s) => (s, None),
            Value::Document(doc) => {
                let mut iter = doc.into_iter();
                let (k, v) = iter
                    .next()
                    .ok_or_else(|| de::Error::custom("expected an externally tagged enum"))?;
                if iter.next().is_some() {
                    return Err(de::Error::custom(
                        "expected a single-entry map for an enum variant",
                    ));
                }
                (k, Some(v))
            }
            _ => return Err(de::Error::custom("expected an enum")),
        };
        visitor.visit_enum(EnumDeserializer { variant, value })
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

struct SeqDeserializer {
    iter: std::vec::IntoIter<Value>,
}

impl<'de> SeqAccess<'de> for SeqDeserializer {
    type Error = DocumentError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, DocumentError> {
        match self.iter.next() {
            Some(value) => seed.deserialize(value).map(Some),
            None => Ok(None),
        }
    }
}

struct MapDeserializer {
    iter: std::vec::IntoIter<(String, Value)>,
    value: Option<Value>,
}

impl<'de> MapAccess<'de> for MapDeserializer {
    type Error = DocumentError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, DocumentError> {
        match self.iter.next() {
            Some((key, value)) => {
                self.value = Some(value);
                seed.deserialize(Value::String(key)).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, DocumentError> {
        let value = self
            .value
            .take()
            .ok_or_else(|| de::Error::custom("value requested before key"))?;
        seed.deserialize(value)
    }
}

struct EnumDeserializer {
    variant: String,
    value: Option<Value>,
}

impl<'de> EnumAccess<'de> for EnumDeserializer {
    type Error = DocumentError;
    type Variant = VariantDeserializer;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), DocumentError> {
        let variant = seed.deserialize(Value::String(self.variant))?;
        Ok((variant, VariantDeserializer { value: self.value }))
    }
}

struct VariantDeserializer {
    value: Option<Value>,
}

impl<'de> VariantAccess<'de> for VariantDeserializer {
    type Error = DocumentError;

    fn unit_variant(self) -> Result<(), DocumentError> {
        match self.value {
            None => Ok(()),
            Some(_) => Err(de::Error::custom("expected a unit variant")),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, DocumentError> {
        match self.value {
            Some(value) => seed.deserialize(value),
            None => Err(de::Error::custom("expected a newtype variant")),
        }
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, DocumentError> {
        match self.value {
            Some(Value::Array(arr)) => visitor.visit_seq(SeqDeserializer {
                iter: arr.into_iter(),
            }),
            _ => Err(de::Error::custom("expected a tuple variant")),
        }
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DocumentError> {
        match self.value {
            Some(Value::Document(doc)) => visitor.visit_map(MapDeserializer {
                iter: doc.into_iter(),
                value: None,
            }),
            _ => Err(de::Error::custom("expected a struct variant")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct User {
        name: String,
        age: u32,
        tags: Vec<String>,
    }

    #[test]
    fn struct_round_trips_through_a_document() {
        let user = User {
            name: "ada".into(),
            age: 36,
            tags: vec!["math".into(), "cs".into()],
        };

        let doc = to_document(&user).unwrap();
        assert_eq!(doc.get("name"), Some(&Value::String("ada".into())));
        assert_eq!(doc.get("age"), Some(&Value::I64(36)));

        let back: User = from_document(doc).unwrap();
        assert_eq!(back, user);
    }

    #[test]
    fn scalar_root_is_rejected() {
        assert!(matches!(
            to_document(&42u32),
            Err(DocumentError::RootNotDocument)
        ));
        assert!(matches!(
            to_document(&vec![1, 2, 3]),
            Err(DocumentError::RootNotDocument)
        ));
    }

    #[derive(Serialize)]
    struct WithId {
        _id: String,
        name: String,
    }

    #[test]
    fn id_in_a_write_is_rejected() {
        let value = WithId {
            _id: "nope".into(),
            name: "ada".into(),
        };
        assert!(matches!(
            to_document(&value),
            Err(DocumentError::IdNotAllowed)
        ));
    }

    #[test]
    fn id_is_injected_on_read() {
        // Build a stored document (no `_id`), then read it back with the id.
        let mut stored = Document::new();
        stored.insert("name", Value::String("ada".into()));

        let id: DocumentId = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;
        let injected = stored.with_id(id);
        assert_eq!(
            injected.get("_id"),
            Some(&Value::Bytes(id.to_be_bytes().to_vec()))
        );
    }

    #[test]
    fn enums_round_trip() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Holder {
            kind: Kind,
        }
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        enum Kind {
            Unit,
            Newtype(u32),
            Tuple(u32, u32),
            Struct { a: u32 },
        }

        for holder in [
            Holder { kind: Kind::Unit },
            Holder {
                kind: Kind::Newtype(7),
            },
            Holder {
                kind: Kind::Tuple(1, 2),
            },
            Holder {
                kind: Kind::Struct { a: 3 },
            },
        ] {
            let doc = to_document(&holder).unwrap();
            let back: Holder = from_document(doc).unwrap();
            assert_eq!(back, holder);
        }
    }
}
