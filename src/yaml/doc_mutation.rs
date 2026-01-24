//! Editor-based document mutations using fyaml's practical COW.
//!
//! This module provides mutation operations that work directly on `Document`
//! via `Editor`, avoiding full document cloning. Only modified nodes are
//! allocated, preserving comments and formatting.

use super::error::Error;
use super::path::{resolve_index, split_path};
use fyaml::Document;

/// Convert shyaml dot-notation path to fyaml slash-notation path.
///
/// shyaml paths: `a.b.c`, `a\.b` (escaped dot), `a\\b` (escaped backslash)
/// fyaml paths: `/a/b/c`, `/a.b`, `/a\b`
///
/// Note: Empty segments are preserved (valid YAML keys).
fn dot_path_to_slash_path(dot_path: &str) -> String {
    let parts = split_path(dot_path);
    if parts.is_empty() || (parts.len() == 1 && parts[0].is_empty()) {
        // Root path
        return String::new();
    }
    // Join with / and prepend /
    format!("/{}", parts.join("/"))
}

/// Split a path into parent path and final key.
///
/// Examples:
/// - `a.b.c` -> (`a.b`, `c`)
/// - `a` -> (``, `a`)
/// - `` -> error
fn split_parent_and_key(dot_path: &str) -> Result<(String, String), Error> {
    let parts = split_path(dot_path);
    if parts.is_empty() || (parts.len() == 1 && parts[0].is_empty()) {
        return Err(Error::Path("Empty path".to_string()));
    }
    if parts.len() == 1 {
        return Ok((String::new(), parts[0].clone()));
    }
    let key = parts.last().unwrap().clone();
    let parent_parts = &parts[..parts.len() - 1];
    let parent = parent_parts.join(".");
    Ok((parent, key))
}

/// Ensure all ancestor mappings exist for a given path.
///
/// Creates empty mappings for any missing intermediate keys.
/// Also replaces null values with empty mappings when encountered.
/// Returns error if a non-null scalar exists at an intermediate path.
fn ensure_ancestors(doc: &mut Document, dot_path: &str) -> Result<(), Error> {
    let parts = split_path(dot_path);
    if parts.len() <= 1 {
        // No ancestors to create (root level key)
        return Ok(());
    }

    // Check/create each ancestor level
    let mut current_path = String::new();
    for part in &parts[..parts.len() - 1] {
        if current_path.is_empty() {
            current_path = part.clone();
        } else {
            current_path = format!("{}.{}", current_path, part);
        }

        let slash_path = dot_path_to_slash_path(&current_path);

        // Check if this path exists and what type it is
        let node_at_path = doc.at_path(&slash_path);

        match node_at_path {
            Some(node) => {
                // Path exists - check if it's a null scalar that needs to be replaced
                if node.is_scalar() {
                    let is_null = node.scalar_bytes().map(|b| b.is_empty()).unwrap_or(false);
                    if is_null {
                        // It's null - replace with empty mapping
                        let mut ed = doc.edit();
                        ed.set_yaml_at(&slash_path, "__placeholder__: ~")
                            .map_err(|e| {
                                Error::Base(format!(
                                    "Failed to replace null at '{}': {}",
                                    current_path, e
                                ))
                            })?;
                        let placeholder_path = format!("{}/__placeholder__", slash_path);
                        ed.delete_at(&placeholder_path).map_err(|e| {
                            Error::Base(format!("Failed to clean placeholder: {}", e))
                        })?;
                    }
                    // else: non-null scalar - will error later when we try to set into it
                }
                // else: it's a mapping or sequence - that's fine, continue
            }
            None => {
                // Need to create this mapping
                // First ensure its parent exists (recursive base case: root always exists or is created)
                let (parent_dot, key) = split_parent_and_key(&current_path)?;
                let parent_slash = if parent_dot.is_empty() {
                    String::new()
                } else {
                    dot_path_to_slash_path(&parent_dot)
                };

                // Check if we need to create root first
                if parent_slash.is_empty() && doc.root().is_none() {
                    // Create empty root mapping using block style YAML
                    let mut ed = doc.edit();
                    let root = ed.build_from_yaml("__placeholder__: ~").map_err(|e| {
                        Error::Base(format!("Failed to create root mapping: {}", e))
                    })?;
                    ed.set_root(root)
                        .map_err(|e| Error::Base(format!("Failed to set root: {}", e)))?;
                    ed.delete_at("/__placeholder__")
                        .map_err(|e| Error::Base(format!("Failed to clean placeholder: {}", e)))?;
                }

                // Now create the mapping at this level using block-style YAML
                let target_path = if parent_slash.is_empty() {
                    format!("/{}", key)
                } else {
                    format!("{}/{}", parent_slash, key)
                };

                let mut ed = doc.edit();
                ed.set_yaml_at(&target_path, "__placeholder__: ~")
                    .map_err(|e| {
                        Error::Base(format!(
                            "Failed to create mapping at '{}': {}",
                            current_path, e
                        ))
                    })?;
                // Remove the placeholder to leave an empty block mapping
                let placeholder_path = format!("{}/__placeholder__", target_path);
                ed.delete_at(&placeholder_path)
                    .map_err(|e| Error::Base(format!("Failed to clean placeholder: {}", e)))?;
            }
        }
    }

    Ok(())
}

/// Set a value at a path in the document using Editor.
///
/// This is the Editor-based equivalent of `set_value()` that avoids
/// full document cloning. Only the modified path is allocated.
///
/// # Arguments
/// * `doc` - The document to modify (mutably borrowed)
/// * `dot_path` - Path in dot notation (e.g., `a.b.c`)
/// * `value_str` - The value to set
/// * `parse_as_yaml` - If true, parse value_str as YAML; if false, treat as literal string
pub fn set_value_doc(
    doc: &mut Document,
    dot_path: &str,
    value_str: &str,
    parse_as_yaml: bool,
) -> Result<(), Error> {
    let parts = split_path(dot_path);
    if parts.is_empty() || (parts.len() == 1 && parts[0].is_empty()) {
        return Err(Error::Path("Empty path".to_string()));
    }

    // Prepare the YAML value
    // Always normalize through Value for consistent block style output
    let yaml_value = if parse_as_yaml {
        // Parse as YAML, convert to Value, then re-emit for consistent style
        let v: fyaml::Value = value_str
            .parse()
            .map_err(|e| Error::Base(format!("Failed to parse YAML value: {}", e)))?;
        v.to_yaml_string()
            .map_err(|e| Error::Base(format!("Failed to serialize value: {}", e)))?
            .trim()
            .to_string()
    } else {
        // Treat as literal string - need to emit as valid YAML
        // Use fyaml's Value to properly quote the string
        let v = fyaml::Value::String(value_str.to_string());
        v.to_yaml_string()
            .map_err(|e| Error::Base(format!("Failed to serialize value: {}", e)))?
            .trim()
            .to_string()
    };

    // Handle empty document - create root mapping using block style
    if doc.root().is_none() {
        let mut ed = doc.edit();
        // Use placeholder to create block-style mapping, then remove placeholder
        let root = ed
            .build_from_yaml("__placeholder__: ~")
            .map_err(|e| Error::Base(format!("Failed to create root mapping: {}", e)))?;
        ed.set_root(root)
            .map_err(|e| Error::Base(format!("Failed to set root: {}", e)))?;
        ed.delete_at("/__placeholder__")
            .map_err(|e| Error::Base(format!("Failed to clean placeholder: {}", e)))?;
    }

    // Check if parent is a scalar (invalid target for set, unless it's null)
    let key = parts.last().unwrap();
    if parts.len() > 1 {
        let parent_parts = &parts[..parts.len() - 1];
        let parent_slash_path = format!("/{}", parent_parts.join("/"));

        if let Some(parent) = doc.at_path(&parent_slash_path) {
            if parent.is_scalar() {
                // Check if it's a null scalar (empty bytes) - if so, we can replace it with a mapping
                let is_null = parent.scalar_bytes().map(|b| b.is_empty()).unwrap_or(false);
                if is_null {
                    // It's null - replace with empty mapping
                    let parent_dot_path = parent_parts.join(".");
                    let mut ed = doc.edit();
                    ed.set_yaml_at(&parent_slash_path, "__placeholder__: ~")
                        .map_err(|e| {
                            Error::Base(format!(
                                "Failed to replace null at '{}': {}",
                                parent_dot_path, e
                            ))
                        })?;
                    let placeholder_path = format!("{}/__placeholder__", parent_slash_path);
                    ed.delete_at(&placeholder_path)
                        .map_err(|e| Error::Base(format!("Failed to clean placeholder: {}", e)))?;
                } else {
                    // Non-null scalar - can't set into it
                    return Err(Error::Path(format!(
                        "invalid path '{}', cannot set value on scalar at '{}'.",
                        dot_path, key
                    )));
                }
            }
        }
    }

    // Ensure ancestors exist for mapping paths (fyaml handles sequences directly)
    ensure_ancestors(doc, dot_path)?;

    let slash_path = dot_path_to_slash_path(dot_path);

    let mut ed = doc.edit();
    ed.set_yaml_at(&slash_path, &yaml_value)
        .map_err(|e| Error::Base(format!("Failed to set value at '{}': {}", dot_path, e)))?;

    Ok(())
}

/// Delete a value at a path in the document using Editor.
///
/// This is the Editor-based equivalent of `del()`.
///
/// # Arguments
/// * `doc` - The document to modify
/// * `dot_path` - Path in dot notation
pub fn del_doc(doc: &mut Document, dot_path: &str) -> Result<(), Error> {
    if dot_path.is_empty() {
        return Err(Error::Path("Empty path".to_string()));
    }

    let parts = split_path(dot_path);
    if parts.is_empty() || (parts.len() == 1 && parts[0].is_empty()) {
        return Err(Error::Path("Empty path".to_string()));
    }

    // Check if the parent is a sequence - if so, validate index before delete
    let key = parts.last().unwrap();
    if !parts.is_empty() {
        // Get parent path
        let parent_slash_path = if parts.len() == 1 {
            String::new() // Root level
        } else {
            let parent_parts = &parts[..parts.len() - 1];
            format!("/{}", parent_parts.join("/"))
        };

        // Get parent node to check if it's a sequence
        let parent_node = if parent_slash_path.is_empty() {
            doc.root()
        } else {
            doc.at_path(&parent_slash_path)
        };

        if let Some(parent) = parent_node {
            if parent.is_sequence() {
                // Validate index using resolve_index for proper error messages
                let seq_len = parent.seq_len().unwrap_or(0);
                let resolved_idx = resolve_index(key, seq_len, dot_path)?;

                // Build path with resolved index for deletion
                let slash_path = if parent_slash_path.is_empty() {
                    format!("/{}", resolved_idx)
                } else {
                    format!("{}/{}", parent_slash_path, resolved_idx)
                };

                let mut ed = doc.edit();
                ed.delete_at(&slash_path).map_err(|e| {
                    Error::Base(format!("Failed to delete at '{}': {}", dot_path, e))
                })?;

                return Ok(());
            }
        }
    }

    // Not a sequence or parent doesn't exist - use original path
    let slash_path = dot_path_to_slash_path(dot_path);

    let mut ed = doc.edit();
    let deleted = ed
        .delete_at(&slash_path)
        .map_err(|e| Error::Base(format!("Failed to delete at '{}': {}", dot_path, e)))?;

    if !deleted {
        return Err(Error::Path(format!(
            "invalid path '{}', missing key '{}' in struct.",
            dot_path, key
        )));
    }

    Ok(())
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dot_path_to_slash_path_simple() {
        assert_eq!(dot_path_to_slash_path("a"), "/a");
        assert_eq!(dot_path_to_slash_path("a.b"), "/a/b");
        assert_eq!(dot_path_to_slash_path("a.b.c"), "/a/b/c");
    }

    #[test]
    fn test_dot_path_to_slash_path_escaped_dot() {
        // `a\.b` in shyaml means key "a.b"
        assert_eq!(dot_path_to_slash_path(r"a\.b"), "/a.b");
        assert_eq!(dot_path_to_slash_path(r"a\.b.c"), "/a.b/c");
    }

    #[test]
    fn test_dot_path_to_slash_path_empty_segments() {
        // Empty string is a valid YAML key
        assert_eq!(dot_path_to_slash_path("a..b"), "/a//b");
        assert_eq!(dot_path_to_slash_path(".a"), "//a");
    }

    #[test]
    fn test_dot_path_to_slash_path_root() {
        assert_eq!(dot_path_to_slash_path(""), "");
    }

    #[test]
    fn test_split_parent_and_key() {
        let (parent, key) = split_parent_and_key("a.b.c").unwrap();
        assert_eq!(parent, "a.b");
        assert_eq!(key, "c");

        let (parent, key) = split_parent_and_key("a").unwrap();
        assert_eq!(parent, "");
        assert_eq!(key, "a");

        let (parent, key) = split_parent_and_key("a.b").unwrap();
        assert_eq!(parent, "a");
        assert_eq!(key, "b");
    }

    #[test]
    fn test_split_parent_and_key_empty_error() {
        assert!(split_parent_and_key("").is_err());
    }

    #[test]
    fn test_set_value_doc_simple() {
        let mut doc = Document::parse_str("name: old").unwrap();
        set_value_doc(&mut doc, "name", "new", false).unwrap();

        let root = doc.root().unwrap();
        let name = root.at_path("/name").unwrap();
        assert_eq!(name.scalar_str().unwrap(), "new");
    }

    #[test]
    fn test_set_value_doc_nested() {
        let mut doc = Document::parse_str("config: {}").unwrap();
        set_value_doc(&mut doc, "config.host", "localhost", false).unwrap();

        let root = doc.root().unwrap();
        let host = root.at_path("/config/host").unwrap();
        assert_eq!(host.scalar_str().unwrap(), "localhost");
    }

    #[test]
    fn test_set_value_doc_create_intermediate() {
        // Start with a document that has null root (empty YAML)
        let mut doc = Document::new().unwrap();
        set_value_doc(&mut doc, "a.b.c", "deep", false).unwrap();

        let root = doc.root().unwrap();
        let val = root.at_path("/a/b/c").unwrap();
        assert_eq!(val.scalar_str().unwrap(), "deep");
    }

    #[test]
    fn test_set_value_doc_yaml_mode() {
        let mut doc = Document::parse_str("data: {}").unwrap();
        set_value_doc(&mut doc, "data.items", "[1, 2, 3]", true).unwrap();

        let root = doc.root().unwrap();
        let items = root.at_path("/data/items").unwrap();
        assert!(items.is_sequence());
        assert_eq!(items.seq_len().unwrap(), 3);
    }

    #[test]
    fn test_del_doc_simple() {
        let mut doc = Document::parse_str("a: 1\nb: 2").unwrap();
        del_doc(&mut doc, "b").unwrap();

        let root = doc.root().unwrap();
        assert!(root.at_path("/a").is_some());
        assert!(root.at_path("/b").is_none());
    }

    #[test]
    fn test_del_doc_nested() {
        let mut doc = Document::parse_str("config:\n  host: localhost\n  port: 5432").unwrap();
        del_doc(&mut doc, "config.port").unwrap();

        let root = doc.root().unwrap();
        assert!(root.at_path("/config/host").is_some());
        assert!(root.at_path("/config/port").is_none());
    }

    #[test]
    fn test_del_doc_missing_key_error() {
        let mut doc = Document::parse_str("a: 1").unwrap();
        let result = del_doc(&mut doc, "nonexistent");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing key 'nonexistent'"));
    }

    #[test]
    fn test_del_doc_empty_path_error() {
        let mut doc = Document::parse_str("a: 1").unwrap();
        let result = del_doc(&mut doc, "");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Empty path"));
    }
}

#[test]
fn test_set_value_doc_null_to_mapping() {
    // Test setting into a null value should convert it to mapping
    let mut doc = Document::parse_str("config:").unwrap();

    // Verify config is initially null
    let config = doc.at_path("/config").unwrap();

    // scalar_bytes returns Ok(b"") for null, not Err
    // So we detect null by: is_scalar && scalar_bytes is empty
    assert!(
        config.is_scalar(),
        "config should be scalar (null is a scalar)"
    );
    let bytes = config.scalar_bytes().unwrap();
    assert!(
        bytes.is_empty(),
        "config should be null (empty scalar bytes)"
    );

    // Set a nested value - this should replace null with mapping
    set_value_doc(&mut doc, "config.host", "localhost", false).unwrap();

    // Verify config is now a mapping
    let config = doc.at_path("/config").unwrap();
    assert!(config.is_mapping(), "config should now be a mapping");

    // Verify the value was set
    let host = doc.at_path("/config/host").unwrap();
    assert_eq!(host.scalar_str().unwrap(), "localhost");
}
