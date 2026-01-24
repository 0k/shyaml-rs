//! Query operations for YAML values.
//!
//! Provides both zero-copy (ValueRef) and owned (Value) query operations.

use super::error::Error;
use super::path::{resolve_index, split_path};
use super::InnerValue;
use fyaml::{Document, ValueRef};
pub use fyaml::{Number, Value};
use indexmap::IndexMap;

// =============================================================================
// Type Name Helpers
// =============================================================================

/// Get the type name for a ValueRef (zero-copy).
#[must_use]
pub fn value_ref_type_name(v: &ValueRef<'_>) -> &'static str {
    if v.is_null() {
        return "NoneType";
    }
    if v.as_bool().is_some() {
        return "bool";
    }
    // Check if it looks like a float (has decimal or exponent)
    if let Some(s) = v.as_str() {
        if (s.contains('.') || s.contains('e') || s.contains('E')) && v.as_f64().is_some() {
            return "float";
        }
        if v.as_i64().is_some() {
            return "int";
        }
    }
    if v.is_sequence() {
        return "sequence";
    }
    if v.is_mapping() {
        return "struct";
    }
    "str"
}

/// Get the type name for an owned Value.
#[must_use]
pub fn value_to_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if is_float_number(n) {
                "float"
            } else {
                "int"
            }
        }
        Value::String(_) => "str",
        Value::Sequence(_) => "sequence",
        Value::Mapping(_) => "struct",
        Value::Tagged(t) => match &t.value {
            Value::Sequence(_) => "sequence",
            Value::Mapping(_) => "struct",
            _ => "str",
        },
    }
}

fn is_float_number(n: &Number) -> bool {
    matches!(n, Number::Float(_))
}

// =============================================================================
// Error Helpers
// =============================================================================

/// Trait for getting a type name string from YAML value types.
/// Allows generic error creation for both `Value` and `ValueRef`.
trait TypeNamed {
    fn type_name(&self) -> &'static str;
}

impl TypeNamed for Value {
    fn type_name(&self) -> &'static str {
        value_to_type_name(self)
    }
}

impl TypeNamed for ValueRef<'_> {
    fn type_name(&self) -> &'static str {
        value_ref_type_name(self)
    }
}

/// Create a type error for operations that require a struct (mapping).
fn type_error_struct<T: TypeNamed>(op: &str, value: &T) -> Error {
    Error::Type(format!(
        "{} does not support '{}' type. Please provide or select a struct.",
        op,
        value.type_name()
    ))
}

/// Create a type error for operations that require a sequence or struct.
fn type_error_seq_or_struct<T: TypeNamed>(op: &str, value: &T) -> Error {
    Error::Type(format!(
        "{} does not support '{}' type. Please provide or select a sequence or struct.",
        op,
        value.type_name()
    ))
}

/// Create a path error for attempting to traverse a scalar value.
fn path_error_cannot_traverse(full_path: &str, part: &str) -> Error {
    Error::Path(format!(
        "invalid path '{}', cannot traverse scalar at '{}'.",
        full_path, part
    ))
}

// =============================================================================
// Value Extraction Helpers
// =============================================================================

/// Extract mapping from a Value, handling tagged values.
///
/// Returns the inner IndexMap if the value is a mapping (or tagged mapping),
/// or a type error if not.
fn as_mapping<'a>(value: &'a Value, op: &str) -> Result<&'a IndexMap<Value, Value>, Error> {
    match value {
        Value::Mapping(m) => Ok(m),
        Value::Tagged(t) => match &t.value {
            Value::Mapping(m) => Ok(m),
            _ => Err(type_error_struct(op, value)),
        },
        _ => Err(type_error_struct(op, value)),
    }
}

// Note: inner_value functionality is now provided by InnerValue trait
// Use value.inner() instead of inner_value(value)

// =============================================================================
// Zero-Copy Path Navigation
// =============================================================================

/// Zero-copy path navigation returning ValueRef.
///
/// This avoids allocating intermediate `Value` structures for read-only operations.
pub fn get_value_ref<'a>(path: Option<&str>, doc: &'a Document) -> Result<ValueRef<'a>, Error> {
    let root = doc
        .root_value()
        .ok_or_else(|| Error::Path("empty document".into()))?;

    match path {
        None => Ok(root),
        Some(p) => navigate_value_ref(root, p),
    }
}

/// Navigate ValueRef using dot-notation path.
fn navigate_value_ref<'a>(root: ValueRef<'a>, path: &str) -> Result<ValueRef<'a>, Error> {
    let parts = split_path(path);
    let mut current = root;

    for part in &parts {
        current = match navigate_one_step_ref(current, part, path)? {
            Some(v) => v,
            None => {
                return Err(Error::Path(format!(
                    "invalid path '{}', missing key '{}' in struct.",
                    path, part
                )))
            }
        };
    }
    Ok(current)
}

fn navigate_one_step_ref<'a>(
    current: ValueRef<'a>,
    part: &str,
    full_path: &str,
) -> Result<Option<ValueRef<'a>>, Error> {
    if current.is_mapping() {
        Ok(current.get(part))
    } else if current.is_sequence() {
        let len = current.seq_len().unwrap_or(0);
        let idx = resolve_index(part, len, full_path)?;
        Ok(current.index(idx as i32))
    } else {
        Err(path_error_cannot_traverse(full_path, part))
    }
}

// =============================================================================
// Zero-Copy Query Functions
// =============================================================================

/// Get type name using zero-copy.
pub fn get_type_ref(path: Option<&str>, doc: &Document) -> Result<String, Error> {
    let value = get_value_ref(path, doc)?;

    // Check for tag first
    if let Some(tag) = value.tag() {
        return Ok(tag.to_string());
    }

    Ok(value_ref_type_name(&value).to_string())
}

/// Get length using zero-copy.
pub fn get_length_ref(path: Option<&str>, doc: &Document) -> Result<usize, Error> {
    let value = get_value_ref(path, doc)?;

    if let Some(len) = value.seq_len() {
        return Ok(len);
    }
    if let Some(len) = value.map_len() {
        return Ok(len);
    }

    Err(type_error_seq_or_struct("get-length", &value))
}

/// Iterator for keys using zero-copy.
pub fn keys_ref<'a>(
    path: Option<&str>,
    doc: &'a Document,
) -> Result<impl Iterator<Item = ValueRef<'a>>, Error> {
    let value = get_value_ref(path, doc)?;

    if !value.is_mapping() {
        return Err(type_error_struct("keys", &value));
    }

    Ok(value.map_iter().map(|(k, _)| k))
}

/// Iterator for values using zero-copy.
pub fn values_ref<'a>(
    path: Option<&str>,
    doc: &'a Document,
) -> Result<impl Iterator<Item = ValueRef<'a>>, Error> {
    let value = get_value_ref(path, doc)?;

    if !value.is_mapping() {
        return Err(type_error_struct("values", &value));
    }

    Ok(value.map_iter().map(|(_, v)| v))
}

/// Iterator for key-values using zero-copy.
pub fn key_values_ref<'a>(
    path: Option<&str>,
    doc: &'a Document,
) -> Result<impl Iterator<Item = (ValueRef<'a>, ValueRef<'a>)>, Error> {
    let value = get_value_ref(path, doc)?;

    if !value.is_mapping() {
        return Err(type_error_struct("key-values", &value));
    }

    Ok(value.map_iter())
}

/// Enum for get-values iterator (handles both sequences and mappings).
pub enum GetValuesIter<'a> {
    Seq(Box<dyn Iterator<Item = ValueRef<'a>> + 'a>),
    Map(Box<dyn Iterator<Item = (ValueRef<'a>, ValueRef<'a>)> + 'a>),
}

/// Get values (sequence or mapping) using zero-copy.
pub fn get_values_ref<'a>(
    path: Option<&str>,
    doc: &'a Document,
) -> Result<GetValuesIter<'a>, Error> {
    let value = get_value_ref(path, doc)?;

    if value.is_sequence() {
        Ok(GetValuesIter::Seq(Box::new(value.seq_iter())))
    } else if value.is_mapping() {
        Ok(GetValuesIter::Map(Box::new(value.map_iter())))
    } else {
        Err(type_error_seq_or_struct("get-values", &value))
    }
}

// =============================================================================
// Value-Based Path Navigation (for mutations/chains)
// =============================================================================

fn lookup_in_map<'a>(
    map: &'a IndexMap<Value, Value>,
    part: &str,
    path: &str,
) -> Result<&'a Value, Error> {
    let string_key = Value::String(part.to_string());
    if let Some(v) = map.get(&string_key) {
        return Ok(v);
    }
    if part.is_empty() {
        if let Some(v) = map.get(&Value::Null) {
            return Ok(v);
        }
    }
    Err(Error::Path(format!(
        "invalid path '{}', missing key '{}' in struct.",
        path, part
    )))
}

/// Navigate to a value at a path (internal helper).
pub fn get_at_path<'a>(value: &'a Value, path: Option<&str>) -> Result<&'a Value, Error> {
    let path = match path {
        None => return Ok(value),
        Some(p) => p,
    };

    let parts = split_path(path);
    let mut current = value;

    for part in &parts {
        current = match current {
            Value::Mapping(map) => lookup_in_map(map, part, path)?,
            Value::Sequence(seq) => {
                let idx = resolve_index(part, seq.len(), path)?;
                &seq[idx]
            }
            Value::Tagged(tagged) => match &tagged.value {
                Value::Mapping(map) => lookup_in_map(map, part, path)?,
                Value::Sequence(seq) => {
                    let idx = resolve_index(part, seq.len(), path)?;
                    &seq[idx]
                }
                _ => return Err(path_error_cannot_traverse(path, part)),
            },
            _ => return Err(path_error_cannot_traverse(path, part)),
        };
    }

    Ok(current)
}

/// Get value at path (owned version for command chains).
pub fn get_value(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let result = get_at_path(value, path)?;
    Ok(result.clone())
}

// =============================================================================
// Type and Length Operations (Value-based)
// =============================================================================

pub fn get_type(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let target = get_at_path(value, path)?;

    if let Value::Tagged(t) = target {
        return Ok(Value::String(t.tag.clone()));
    }

    Ok(Value::String(value_to_type_name(target).to_string()))
}

pub fn get_length(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let target = get_at_path(value, path)?;

    let len = match target.inner() {
        Value::Sequence(seq) => seq.len(),
        Value::Mapping(map) => map.len(),
        _ => return Err(type_error_seq_or_struct("get-length", target)),
    };

    Ok(Value::Number(Number::UInt(len as u64)))
}

// =============================================================================
// Keys, Values, Key-Values (Value-based)
// =============================================================================

pub fn keys(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let target = get_at_path(value, path)?;
    let map = as_mapping(target, "keys")?;
    let keys: Vec<Value> = map.keys().cloned().collect();
    Ok(Value::Sequence(keys))
}

pub fn values(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let target = get_at_path(value, path)?;
    let map = as_mapping(target, "values")?;
    let vals: Vec<Value> = map.values().cloned().collect();
    Ok(Value::Sequence(vals))
}

pub fn get_values(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let target = get_at_path(value, path)?;

    match target.inner() {
        Value::Sequence(seq) => Ok(Value::Sequence(seq.clone())),
        Value::Mapping(map) => {
            let result: Vec<Value> = map
                .iter()
                .flat_map(|(k, v)| [k.clone(), v.clone()])
                .collect();
            Ok(Value::Sequence(result))
        }
        _ => Err(type_error_seq_or_struct("get-values", target)),
    }
}

pub fn key_values(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let target = get_at_path(value, path)?;
    let map = as_mapping(target, "key-values")?;
    let result: Vec<Value> = map
        .iter()
        .flat_map(|(k, v)| [k.clone(), v.clone()])
        .collect();
    Ok(Value::Sequence(result))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use fyaml::TaggedValue;
    use indexmap::indexmap;

    // -------------------------------------------------------------------------
    // Type Name Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_value_to_type_name_null() {
        assert_eq!(value_to_type_name(&Value::Null), "NoneType");
    }

    #[test]
    fn test_value_to_type_name_bool() {
        assert_eq!(value_to_type_name(&Value::Bool(true)), "bool");
        assert_eq!(value_to_type_name(&Value::Bool(false)), "bool");
    }

    #[test]
    fn test_value_to_type_name_int() {
        assert_eq!(value_to_type_name(&Value::Number(Number::Int(42))), "int");
        assert_eq!(value_to_type_name(&Value::Number(Number::UInt(42))), "int");
    }

    #[test]
    fn test_value_to_type_name_float() {
        assert_eq!(
            value_to_type_name(&Value::Number(Number::Float(1.5))),
            "float"
        );
    }

    #[test]
    fn test_value_to_type_name_string() {
        assert_eq!(
            value_to_type_name(&Value::String("hello".to_string())),
            "str"
        );
    }

    #[test]
    fn test_value_to_type_name_sequence() {
        assert_eq!(value_to_type_name(&Value::Sequence(vec![])), "sequence");
    }

    #[test]
    fn test_value_to_type_name_mapping() {
        assert_eq!(
            value_to_type_name(&Value::Mapping(IndexMap::new())),
            "struct"
        );
    }

    #[test]
    fn test_value_to_type_name_tagged_sequence() {
        let tagged = Value::Tagged(Box::new(TaggedValue {
            tag: "!custom".to_string(),
            value: Value::Sequence(vec![]),
        }));
        assert_eq!(value_to_type_name(&tagged), "sequence");
    }

    #[test]
    fn test_value_to_type_name_tagged_mapping() {
        let tagged = Value::Tagged(Box::new(TaggedValue {
            tag: "!custom".to_string(),
            value: Value::Mapping(IndexMap::new()),
        }));
        assert_eq!(value_to_type_name(&tagged), "struct");
    }

    #[test]
    fn test_value_to_type_name_tagged_scalar() {
        let tagged = Value::Tagged(Box::new(TaggedValue {
            tag: "!custom".to_string(),
            value: Value::String("value".to_string()),
        }));
        // Tagged scalars return "str" (simplified)
        assert_eq!(value_to_type_name(&tagged), "str");
    }

    // -------------------------------------------------------------------------
    // get_at_path Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_get_at_path_none() {
        let value = Value::String("hello".to_string());
        let result = get_at_path(&value, None).unwrap();
        assert_eq!(result, &value);
    }

    #[test]
    fn test_get_at_path_simple_key() {
        let value = Value::Mapping(indexmap! {
            Value::String("name".to_string()) => Value::String("alice".to_string()),
        });
        let result = get_at_path(&value, Some("name")).unwrap();
        assert_eq!(result, &Value::String("alice".to_string()));
    }

    #[test]
    fn test_get_at_path_nested() {
        let value = Value::Mapping(indexmap! {
            Value::String("a".to_string()) => Value::Mapping(indexmap! {
                Value::String("b".to_string()) => Value::Number(Number::Int(42)),
            }),
        });
        let result = get_at_path(&value, Some("a.b")).unwrap();
        assert_eq!(result, &Value::Number(Number::Int(42)));
    }

    #[test]
    fn test_get_at_path_sequence_index() {
        let value = Value::Mapping(indexmap! {
            Value::String("items".to_string()) => Value::Sequence(vec![
                Value::String("a".to_string()),
                Value::String("b".to_string()),
                Value::String("c".to_string()),
            ]),
        });
        let result = get_at_path(&value, Some("items.1")).unwrap();
        assert_eq!(result, &Value::String("b".to_string()));
    }

    #[test]
    fn test_get_at_path_negative_index() {
        let value = Value::Sequence(vec![
            Value::String("first".to_string()),
            Value::String("last".to_string()),
        ]);
        let result = get_at_path(&value, Some("-1")).unwrap();
        assert_eq!(result, &Value::String("last".to_string()));
    }

    #[test]
    fn test_get_at_path_missing_key() {
        let value = Value::Mapping(indexmap! {
            Value::String("a".to_string()) => Value::Number(Number::Int(1)),
        });
        let err = get_at_path(&value, Some("b")).unwrap_err();
        assert!(matches!(err, Error::Path(_)));
        assert!(err.to_string().contains("missing key 'b'"));
    }

    #[test]
    fn test_get_at_path_cannot_traverse_scalar() {
        let value = Value::String("hello".to_string());
        let err = get_at_path(&value, Some("child")).unwrap_err();
        assert!(matches!(err, Error::Path(_)));
        assert!(err.to_string().contains("cannot traverse scalar"));
    }

    #[test]
    fn test_get_at_path_index_out_of_range() {
        let value = Value::Sequence(vec![Value::String("a".to_string())]);
        let err = get_at_path(&value, Some("5")).unwrap_err();
        assert!(matches!(err, Error::Path(_)));
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn test_get_at_path_through_tagged_mapping() {
        let value = Value::Tagged(Box::new(TaggedValue {
            tag: "!custom".to_string(),
            value: Value::Mapping(indexmap! {
                Value::String("key".to_string()) => Value::String("value".to_string()),
            }),
        }));
        let result = get_at_path(&value, Some("key")).unwrap();
        assert_eq!(result, &Value::String("value".to_string()));
    }

    #[test]
    fn test_get_at_path_through_tagged_sequence() {
        let value = Value::Tagged(Box::new(TaggedValue {
            tag: "!list".to_string(),
            value: Value::Sequence(vec![
                Value::Number(Number::Int(10)),
                Value::Number(Number::Int(20)),
            ]),
        }));
        let result = get_at_path(&value, Some("0")).unwrap();
        assert_eq!(result, &Value::Number(Number::Int(10)));
    }

    // -------------------------------------------------------------------------
    // get_type Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_get_type_basic() {
        let value = Value::String("hello".to_string());
        let result = get_type(None, &value).unwrap();
        assert_eq!(result, Value::String("str".to_string()));
    }

    #[test]
    fn test_get_type_nested() {
        let value = Value::Mapping(indexmap! {
            Value::String("count".to_string()) => Value::Number(Number::Int(42)),
        });
        let result = get_type(Some("count"), &value).unwrap();
        assert_eq!(result, Value::String("int".to_string()));
    }

    #[test]
    fn test_get_type_tagged_returns_tag() {
        let value = Value::Tagged(Box::new(TaggedValue {
            tag: "!custom-type".to_string(),
            value: Value::String("data".to_string()),
        }));
        let result = get_type(None, &value).unwrap();
        assert_eq!(result, Value::String("!custom-type".to_string()));
    }

    // -------------------------------------------------------------------------
    // get_length Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_get_length_sequence() {
        let value = Value::Sequence(vec![
            Value::Number(Number::Int(1)),
            Value::Number(Number::Int(2)),
            Value::Number(Number::Int(3)),
        ]);
        let result = get_length(None, &value).unwrap();
        assert_eq!(result, Value::Number(Number::UInt(3)));
    }

    #[test]
    fn test_get_length_mapping() {
        let value = Value::Mapping(indexmap! {
            Value::String("a".to_string()) => Value::Number(Number::Int(1)),
            Value::String("b".to_string()) => Value::Number(Number::Int(2)),
        });
        let result = get_length(None, &value).unwrap();
        assert_eq!(result, Value::Number(Number::UInt(2)));
    }

    #[test]
    fn test_get_length_nested() {
        let value = Value::Mapping(indexmap! {
            Value::String("items".to_string()) => Value::Sequence(vec![
                Value::String("x".to_string()),
                Value::String("y".to_string()),
            ]),
        });
        let result = get_length(Some("items"), &value).unwrap();
        assert_eq!(result, Value::Number(Number::UInt(2)));
    }

    #[test]
    fn test_get_length_scalar_error() {
        let value = Value::String("hello".to_string());
        let err = get_length(None, &value).unwrap_err();
        assert!(matches!(err, Error::Type(_)));
        assert!(err.to_string().contains("get-length"));
    }

    #[test]
    fn test_get_length_tagged_sequence() {
        let value = Value::Tagged(Box::new(TaggedValue {
            tag: "!list".to_string(),
            value: Value::Sequence(vec![Value::Null, Value::Null]),
        }));
        let result = get_length(None, &value).unwrap();
        assert_eq!(result, Value::Number(Number::UInt(2)));
    }

    // -------------------------------------------------------------------------
    // keys Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_keys_basic() {
        let value = Value::Mapping(indexmap! {
            Value::String("a".to_string()) => Value::Number(Number::Int(1)),
            Value::String("b".to_string()) => Value::Number(Number::Int(2)),
        });
        let result = keys(None, &value).unwrap();
        if let Value::Sequence(seq) = result {
            assert_eq!(seq.len(), 2);
            assert_eq!(seq[0], Value::String("a".to_string()));
            assert_eq!(seq[1], Value::String("b".to_string()));
        } else {
            panic!("Expected sequence");
        }
    }

    #[test]
    fn test_keys_non_mapping_error() {
        let value = Value::Sequence(vec![]);
        let err = keys(None, &value).unwrap_err();
        assert!(matches!(err, Error::Type(_)));
        assert!(err.to_string().contains("keys"));
    }

    // -------------------------------------------------------------------------
    // values Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_values_basic() {
        let value = Value::Mapping(indexmap! {
            Value::String("x".to_string()) => Value::Number(Number::Int(10)),
            Value::String("y".to_string()) => Value::Number(Number::Int(20)),
        });
        let result = values(None, &value).unwrap();
        if let Value::Sequence(seq) = result {
            assert_eq!(seq.len(), 2);
            assert_eq!(seq[0], Value::Number(Number::Int(10)));
            assert_eq!(seq[1], Value::Number(Number::Int(20)));
        } else {
            panic!("Expected sequence");
        }
    }

    // -------------------------------------------------------------------------
    // get_values Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_get_values_sequence() {
        let value = Value::Sequence(vec![
            Value::String("a".to_string()),
            Value::String("b".to_string()),
        ]);
        let result = get_values(None, &value).unwrap();
        if let Value::Sequence(seq) = result {
            assert_eq!(seq.len(), 2);
            assert_eq!(seq[0], Value::String("a".to_string()));
        } else {
            panic!("Expected sequence");
        }
    }

    #[test]
    fn test_get_values_mapping() {
        let value = Value::Mapping(indexmap! {
            Value::String("k".to_string()) => Value::String("v".to_string()),
        });
        let result = get_values(None, &value).unwrap();
        if let Value::Sequence(seq) = result {
            // Should be flattened key-value pairs
            assert_eq!(seq.len(), 2);
            assert_eq!(seq[0], Value::String("k".to_string()));
            assert_eq!(seq[1], Value::String("v".to_string()));
        } else {
            panic!("Expected sequence");
        }
    }

    #[test]
    fn test_get_values_scalar_error() {
        let value = Value::String("scalar".to_string());
        let err = get_values(None, &value).unwrap_err();
        assert!(matches!(err, Error::Type(_)));
    }

    // -------------------------------------------------------------------------
    // key_values Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_key_values_basic() {
        let value = Value::Mapping(indexmap! {
            Value::String("a".to_string()) => Value::Number(Number::Int(1)),
            Value::String("b".to_string()) => Value::Number(Number::Int(2)),
        });
        let result = key_values(None, &value).unwrap();
        if let Value::Sequence(seq) = result {
            assert_eq!(seq.len(), 4); // 2 key-value pairs flattened
            assert_eq!(seq[0], Value::String("a".to_string()));
            assert_eq!(seq[1], Value::Number(Number::Int(1)));
            assert_eq!(seq[2], Value::String("b".to_string()));
            assert_eq!(seq[3], Value::Number(Number::Int(2)));
        } else {
            panic!("Expected sequence");
        }
    }

    // -------------------------------------------------------------------------
    // Path Escaping Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_get_at_path_escaped_dot() {
        // Key with literal dot: "a.b" -> accessed as "a\.b"
        let value = Value::Mapping(indexmap! {
            Value::String("a.b".to_string()) => Value::Number(Number::Int(42)),
        });
        let result = get_at_path(&value, Some(r"a\.b")).unwrap();
        assert_eq!(result, &Value::Number(Number::Int(42)));
    }

    #[test]
    fn test_get_at_path_empty_key() {
        // Empty string key
        let value = Value::Mapping(indexmap! {
            Value::String("".to_string()) => Value::String("empty-key-value".to_string()),
        });
        let result = get_at_path(&value, Some("")).unwrap();
        assert_eq!(result, &Value::String("empty-key-value".to_string()));
    }
}
