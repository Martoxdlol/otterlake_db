/// Global auto incrementing id
pub type TS = u64;

/// UUID v7 (it is important that is v7)
pub type DocumentId = u128;

/// Collection id, negative numbers are temporary indicators used in write sets for new collections
pub type CollectionId = i64;

/// Index id, negative numbers are temporary indicators used in write sets for new indexes
pub type IndexId = i64;

// Arbitrary binary data
pub type Value = Vec<u8>;

/// Direction for cursors
pub enum Direction {
    Forward,
    Reverse,
}
