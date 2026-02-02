//! Mutation operations for YAML values.
//!
//! Provides set-value, delete, and parse operations.

use super::error::Error;
use super::path::{resolve_index, split_path};
pub use fyaml::Value;

/// Set a value at a key path.
pub fn set_value(key: &str, new_value: Value, mut base: Value) -> Result<Value, Error> {
    if matches!(base, Value::Null) {
        base = Value::Mapping(Default::default());
    }
    set_value_at_path(&mut base, key, new_value)?;
    Ok(base)
}

/// Parse a string as either full YAML or with scalar type inference.
///
/// With `parse_as_yaml = true` (`-y` flag): the value is parsed as full YAML,
/// including structures like sequences and mappings.
///
/// With `parse_as_yaml = false` (default): scalar type inference is applied —
/// numbers, booleans, and null are recognized as their native YAML types,
/// but YAML structures (sequences, mappings) are kept as literal strings.
pub fn parse_value(value_str: &str, parse_as_yaml: bool) -> Result<Value, Error> {
    if parse_as_yaml {
        value_str
            .parse()
            .map_err(|e| Error::Base(format!("Failed to parse value as YAML: {}", e)))
    } else {
        // Scalar type inference: parse as YAML, accept only scalar results.
        // Sequences and mappings are kept as literal strings.
        match value_str.parse::<Value>() {
            Ok(v @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_))) => Ok(v),
            _ => Ok(Value::String(value_str.to_string())),
        }
    }
}

fn set_value_at_path(root: &mut Value, path: &str, value: Value) -> Result<(), Error> {
    let path_parts = split_path(path);

    if path_parts.is_empty() {
        return Err(Error::Path("Empty path".to_string()));
    }

    let mut current = root;

    for (i, part) in path_parts.iter().enumerate() {
        let is_last = i == path_parts.len() - 1;

        if is_last {
            match current {
                Value::Mapping(map) => {
                    map.insert(Value::String(part.clone()), value);
                    return Ok(());
                }
                Value::Sequence(seq) => {
                    let idx = resolve_index(part, seq.len(), path)?;
                    seq[idx] = value;
                    return Ok(());
                }
                _ => {
                    return Err(Error::Path(format!(
                        "invalid path '{}', cannot set value on scalar at '{}'.",
                        path, part
                    )));
                }
            }
        }

        match current {
            Value::Mapping(map) => {
                let key = Value::String(part.clone());
                if !map.contains_key(&key) {
                    map.insert(key.clone(), Value::Mapping(Default::default()));
                }
                current = map.get_mut(&key).unwrap();
            }
            Value::Sequence(seq) => {
                let idx = resolve_index(part, seq.len(), path)?;
                current = &mut seq[idx];
            }
            _ => {
                return Err(Error::Path(format!(
                    "invalid path '{}', cannot traverse scalar at '{}'.",
                    path, part
                )));
            }
        }
    }

    Ok(())
}

/// Delete a value at a key path.
pub fn del(key: &str, mut base: Value) -> Result<Value, Error> {
    if matches!(base, Value::Null) {
        return Err(Error::Path("Cannot delete from empty document".to_string()));
    }
    del_at_path(&mut base, key)?;
    Ok(base)
}

fn del_at_path(root: &mut Value, path: &str) -> Result<(), Error> {
    let path_parts = split_path(path);

    if path_parts.is_empty() || (path_parts.len() == 1 && path_parts[0].is_empty()) {
        return Err(Error::Path("Empty path".to_string()));
    }

    let mut current = root;

    for (i, part) in path_parts.iter().enumerate() {
        let is_last = i == path_parts.len() - 1;

        if is_last {
            return match current {
                Value::Mapping(map) => {
                    let key = Value::String(part.clone());
                    if map.shift_remove(&key).is_none() {
                        Err(Error::Path(format!(
                            "invalid path '{}', missing key '{}' in struct.",
                            path, part
                        )))
                    } else {
                        Ok(())
                    }
                }
                Value::Sequence(seq) => {
                    let idx = resolve_index(part, seq.len(), path)?;
                    seq.remove(idx);
                    Ok(())
                }
                _ => Err(Error::Path(format!(
                    "invalid path '{}', cannot delete from scalar.",
                    path
                ))),
            };
        }

        current = match current {
            Value::Mapping(map) => {
                let key = Value::String(part.clone());
                map.get_mut(&key).ok_or_else(|| {
                    Error::Path(format!(
                        "invalid path '{}', missing key '{}' in struct.",
                        path, part
                    ))
                })?
            }
            Value::Sequence(seq) => {
                let idx = resolve_index(part, seq.len(), path)?;
                &mut seq[idx]
            }
            _ => {
                return Err(Error::Path(format!(
                    "invalid path '{}', cannot traverse scalar at '{}'.",
                    path, part
                )));
            }
        };
    }

    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use fyaml::Number;
    use indexmap::indexmap;

    // -------------------------------------------------------------------------
    // parse_value Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_value_literal_string() {
        let result = parse_value("hello world", false).unwrap();
        assert_eq!(result, Value::String("hello world".to_string()));
    }

    #[test]
    fn test_parse_value_literal_preserves_yaml_structures() {
        // Without parse_as_yaml, YAML structures are kept as strings
        let result = parse_value("[1, 2, 3]", false).unwrap();
        assert_eq!(result, Value::String("[1, 2, 3]".to_string()));

        let result = parse_value("{a: 1, b: 2}", false).unwrap();
        assert_eq!(result, Value::String("{a: 1, b: 2}".to_string()));
    }

    #[test]
    fn test_parse_value_literal_infers_number() {
        let result = parse_value("42", false).unwrap();
        assert!(matches!(result, Value::Number(_)));
    }

    #[test]
    fn test_parse_value_literal_infers_bool() {
        let result = parse_value("true", false).unwrap();
        assert_eq!(result, Value::Bool(true));

        let result = parse_value("false", false).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn test_parse_value_literal_infers_null() {
        let result = parse_value("null", false).unwrap();
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn test_parse_value_as_yaml_sequence() {
        let result = parse_value("[1, 2, 3]", true).unwrap();
        if let Value::Sequence(seq) = result {
            assert_eq!(seq.len(), 3);
        } else {
            panic!("Expected sequence");
        }
    }

    #[test]
    fn test_parse_value_as_yaml_mapping() {
        let result = parse_value("{a: 1, b: 2}", true).unwrap();
        if let Value::Mapping(map) = result {
            assert_eq!(map.len(), 2);
        } else {
            panic!("Expected mapping");
        }
    }

    #[test]
    fn test_parse_value_as_yaml_number() {
        let result = parse_value("42", true).unwrap();
        assert!(matches!(result, Value::Number(_)));
    }

    #[test]
    fn test_parse_value_as_yaml_bool() {
        let result = parse_value("true", true).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_parse_value_as_yaml_null() {
        let result = parse_value("null", true).unwrap();
        assert_eq!(result, Value::Null);
    }

    // -------------------------------------------------------------------------
    // set_value Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_set_value_simple() {
        let base = Value::Mapping(indexmap! {
            Value::String("a".to_string()) => Value::Number(Number::Int(1)),
        });
        let result = set_value("b", Value::Number(Number::Int(2)), base).unwrap();
        if let Value::Mapping(map) = result {
            assert_eq!(map.len(), 2);
            assert_eq!(
                map.get(&Value::String("b".to_string())),
                Some(&Value::Number(Number::Int(2)))
            );
        } else {
            panic!("Expected mapping");
        }
    }

    #[test]
    fn test_set_value_overwrite() {
        let base = Value::Mapping(indexmap! {
            Value::String("key".to_string()) => Value::String("old".to_string()),
        });
        let result = set_value("key", Value::String("new".to_string()), base).unwrap();
        if let Value::Mapping(map) = result {
            assert_eq!(
                map.get(&Value::String("key".to_string())),
                Some(&Value::String("new".to_string()))
            );
        } else {
            panic!("Expected mapping");
        }
    }

    #[test]
    fn test_set_value_nested() {
        let base = Value::Mapping(indexmap! {
            Value::String("outer".to_string()) => Value::Mapping(indexmap! {
                Value::String("inner".to_string()) => Value::Number(Number::Int(1)),
            }),
        });
        let result = set_value("outer.inner", Value::Number(Number::Int(99)), base).unwrap();
        if let Value::Mapping(map) = &result {
            if let Some(Value::Mapping(inner)) = map.get(&Value::String("outer".to_string())) {
                assert_eq!(
                    inner.get(&Value::String("inner".to_string())),
                    Some(&Value::Number(Number::Int(99)))
                );
                return;
            }
        }
        panic!("Expected nested structure");
    }

    #[test]
    fn test_set_value_creates_intermediate_mappings() {
        let base = Value::Mapping(indexmap! {});
        let result = set_value("a.b.c", Value::String("deep".to_string()), base).unwrap();

        // Navigate to verify: a.b.c = "deep"
        if let Value::Mapping(a_map) = &result {
            if let Some(Value::Mapping(b_map)) = a_map.get(&Value::String("a".to_string())) {
                if let Some(Value::Mapping(c_map)) = b_map.get(&Value::String("b".to_string())) {
                    assert_eq!(
                        c_map.get(&Value::String("c".to_string())),
                        Some(&Value::String("deep".to_string()))
                    );
                    return;
                }
            }
        }
        panic!("Expected auto-created nested mappings");
    }

    #[test]
    fn test_set_value_on_null_creates_mapping() {
        let base = Value::Null;
        let result = set_value("key", Value::String("value".to_string()), base).unwrap();
        if let Value::Mapping(map) = result {
            assert_eq!(
                map.get(&Value::String("key".to_string())),
                Some(&Value::String("value".to_string()))
            );
        } else {
            panic!("Expected mapping created from null");
        }
    }

    #[test]
    fn test_set_value_sequence_index() {
        let base = Value::Mapping(indexmap! {
            Value::String("items".to_string()) => Value::Sequence(vec![
                Value::String("a".to_string()),
                Value::String("b".to_string()),
                Value::String("c".to_string()),
            ]),
        });
        let result = set_value("items.1", Value::String("changed".to_string()), base).unwrap();
        if let Value::Mapping(map) = &result {
            if let Some(Value::Sequence(seq)) = map.get(&Value::String("items".to_string())) {
                assert_eq!(seq[1], Value::String("changed".to_string()));
                return;
            }
        }
        panic!("Expected sequence modification");
    }

    #[test]
    fn test_set_value_negative_index() {
        let base = Value::Sequence(vec![
            Value::String("first".to_string()),
            Value::String("last".to_string()),
        ]);
        let result = set_value("-1", Value::String("modified".to_string()), base).unwrap();
        if let Value::Sequence(seq) = result {
            assert_eq!(seq[1], Value::String("modified".to_string()));
        } else {
            panic!("Expected sequence");
        }
    }

    #[test]
    fn test_set_value_empty_path_creates_empty_key() {
        // Empty path creates a key with empty string (valid in YAML)
        let base = Value::Mapping(indexmap! {});
        let result = set_value("", Value::String("value".to_string()), base).unwrap();
        if let Value::Mapping(map) = result {
            assert_eq!(
                map.get(&Value::String("".to_string())),
                Some(&Value::String("value".to_string()))
            );
        } else {
            panic!("Expected mapping");
        }
    }

    #[test]
    fn test_set_value_error_traverse_scalar() {
        let base = Value::String("scalar".to_string());
        let err = set_value("child", Value::Null, base).unwrap_err();
        assert!(matches!(err, Error::Path(_)));
        assert!(err.to_string().contains("cannot"));
    }

    #[test]
    fn test_set_value_error_index_out_of_range() {
        let base = Value::Sequence(vec![Value::String("only".to_string())]);
        let err = set_value("5", Value::Null, base).unwrap_err();
        assert!(matches!(err, Error::Path(_)));
        assert!(err.to_string().contains("out of range"));
    }

    // -------------------------------------------------------------------------
    // del Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_del_simple_key() {
        let base = Value::Mapping(indexmap! {
            Value::String("a".to_string()) => Value::Number(Number::Int(1)),
            Value::String("b".to_string()) => Value::Number(Number::Int(2)),
        });
        let result = del("a", base).unwrap();
        if let Value::Mapping(map) = result {
            assert_eq!(map.len(), 1);
            assert!(!map.contains_key(&Value::String("a".to_string())));
            assert!(map.contains_key(&Value::String("b".to_string())));
        } else {
            panic!("Expected mapping");
        }
    }

    #[test]
    fn test_del_nested_key() {
        let base = Value::Mapping(indexmap! {
            Value::String("outer".to_string()) => Value::Mapping(indexmap! {
                Value::String("keep".to_string()) => Value::Number(Number::Int(1)),
                Value::String("remove".to_string()) => Value::Number(Number::Int(2)),
            }),
        });
        let result = del("outer.remove", base).unwrap();
        if let Value::Mapping(map) = &result {
            if let Some(Value::Mapping(inner)) = map.get(&Value::String("outer".to_string())) {
                assert_eq!(inner.len(), 1);
                assert!(!inner.contains_key(&Value::String("remove".to_string())));
                return;
            }
        }
        panic!("Expected nested deletion");
    }

    #[test]
    fn test_del_sequence_index() {
        let base = Value::Sequence(vec![
            Value::String("a".to_string()),
            Value::String("b".to_string()),
            Value::String("c".to_string()),
        ]);
        let result = del("1", base).unwrap();
        if let Value::Sequence(seq) = result {
            assert_eq!(seq.len(), 2);
            assert_eq!(seq[0], Value::String("a".to_string()));
            assert_eq!(seq[1], Value::String("c".to_string()));
        } else {
            panic!("Expected sequence");
        }
    }

    #[test]
    fn test_del_negative_index() {
        let base = Value::Sequence(vec![
            Value::String("first".to_string()),
            Value::String("middle".to_string()),
            Value::String("last".to_string()),
        ]);
        let result = del("-1", base).unwrap();
        if let Value::Sequence(seq) = result {
            assert_eq!(seq.len(), 2);
            assert_eq!(seq[1], Value::String("middle".to_string()));
        } else {
            panic!("Expected sequence");
        }
    }

    #[test]
    fn test_del_error_empty_document() {
        let base = Value::Null;
        let err = del("key", base).unwrap_err();
        assert!(matches!(err, Error::Path(_)));
        assert!(err.to_string().contains("empty document"));
    }

    #[test]
    fn test_del_error_missing_key() {
        let base = Value::Mapping(indexmap! {
            Value::String("exists".to_string()) => Value::Number(Number::Int(1)),
        });
        let err = del("nonexistent", base).unwrap_err();
        assert!(matches!(err, Error::Path(_)));
        assert!(err.to_string().contains("missing key"));
    }

    #[test]
    fn test_del_error_index_out_of_range() {
        let base = Value::Sequence(vec![Value::String("only".to_string())]);
        let err = del("5", base).unwrap_err();
        assert!(matches!(err, Error::Path(_)));
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn test_del_error_empty_path() {
        let base = Value::Mapping(indexmap! {
            Value::String("a".to_string()) => Value::Number(Number::Int(1)),
        });
        let err = del("", base).unwrap_err();
        assert!(matches!(err, Error::Path(_)));
        assert!(err.to_string().contains("Empty path"));
    }

    #[test]
    fn test_del_error_from_scalar() {
        let base = Value::Mapping(indexmap! {
            Value::String("scalar".to_string()) => Value::String("value".to_string()),
        });
        let err = del("scalar.child", base).unwrap_err();
        assert!(matches!(err, Error::Path(_)));
        // Error is "cannot delete from scalar" when trying to delete a child of a scalar
        assert!(err.to_string().contains("cannot delete from scalar"));
    }
}
