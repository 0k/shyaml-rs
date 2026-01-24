//! YAML processing module with zero-copy support.
//!
//! This module provides both zero-copy read operations via `ValueRef` and
//! owned `Value` operations for mutations and command chains.
//!
//! # Module Organization
//!
//! - [`error`]: Error types for YAML operations
//! - [`path`]: Path parsing and index resolution
//! - [`query`]: Query operations (get, type, length, keys, values)
//! - [`mutation`]: Mutation operations (set, delete)
//! - [`merge`]: Merge operations for the `apply` command
//! - [`serialize`]: Serialization utilities

mod error;
pub mod merge;
mod mutation;
mod path;
mod query;
mod serialize;

// Re-export fyaml types
pub use fyaml::{Document, FyParser, Value};
// Note: Number is re-exported for public API even if not used internally
#[allow(unused_imports)]
pub use fyaml::Number;

// Re-export error type
pub use error::Error;

// Re-export merge types
pub use merge::{apply, parse_merge_policies};

// Re-export mutation functions
pub use mutation::{del, parse_value, set_value};

// Re-export query functions (zero-copy)
pub use query::{
    get_length_ref, get_type_ref, get_value_ref, get_values_ref, key_values_ref, keys_ref,
    values_ref, GetValuesIter,
};

// Re-export query functions (owned)
pub use query::{get_length, get_type, get_value, get_values, key_values, keys, values};

// Re-export serialization functions
pub use serialize::{serialize, serialize_raw, serialize_raw_ref, serialize_ref};

// =============================================================================
// Streaming
// =============================================================================

/// Stream documents from stdin (zero-copy friendly).
///
/// Returns an iterator of `Document` objects that can be used with zero-copy
/// operations via `ValueRef` or converted to owned `Value` for mutations.
pub fn streaming_documents_from_stdin(
    line_buffered: bool,
) -> Result<impl Iterator<Item = Result<Document, Error>>, Error> {
    let parser = FyParser::from_stdin_with_line_buffer(line_buffered)?;
    Ok(parser.doc_iter().map(|r| r.map_err(Error::from)))
}

/// Convert a Document to an owned Value.
///
/// Use this when you need to mutate the document or pass it through
/// a command chain. For read-only operations, prefer zero-copy functions.
pub fn document_to_value(doc: &Document) -> Result<Value, Error> {
    match doc.root() {
        Some(node) => Value::from_node_ref(node).map_err(Error::from),
        None => Ok(Value::Null),
    }
}

// =============================================================================
// Version
// =============================================================================

/// Get the fyaml C library version.
pub fn get_version() -> Result<String, String> {
    fyaml::get_c_version().map_err(|e| e.to_string())
}
