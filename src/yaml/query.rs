//! Query operations for YAML values.
//!
//! Provides both zero-copy (ValueRef) and owned (Value) query operations.

use super::error::Error;
use super::path::{resolve_index, split_path};
use fyaml::{Document, ValueRef};
pub use fyaml::{Number, Value};
use indexmap::IndexMap;

// =============================================================================
// Type Name Helpers
// =============================================================================

/// Get the type name for a ValueRef (zero-copy).
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
        Err(Error::Path(format!(
            "invalid path '{}', cannot traverse scalar at '{}'.",
            full_path, part
        )))
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

    Err(Error::Type(format!(
        "get-length does not support '{}' type. Please provide or select a sequence or struct.",
        value_ref_type_name(&value)
    )))
}

/// Iterator for keys using zero-copy.
pub fn keys_ref<'a>(
    path: Option<&str>,
    doc: &'a Document,
) -> Result<impl Iterator<Item = ValueRef<'a>>, Error> {
    let value = get_value_ref(path, doc)?;

    if !value.is_mapping() {
        return Err(Error::Type(format!(
            "keys does not support '{}' type. Please provide or select a struct.",
            value_ref_type_name(&value)
        )));
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
        return Err(Error::Type(format!(
            "values does not support '{}' type. Please provide or select a struct.",
            value_ref_type_name(&value)
        )));
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
        return Err(Error::Type(format!(
            "key-values does not support '{}' type. Please provide or select a struct.",
            value_ref_type_name(&value)
        )));
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
        Err(Error::Type(format!(
            "get-values does not support '{}' type. Please provide or select a sequence or struct.",
            value_ref_type_name(&value)
        )))
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
            Value::Tagged(tagged) => {
                let inner = &tagged.value;
                match inner {
                    Value::Mapping(map) => lookup_in_map(map, part, path)?,
                    Value::Sequence(seq) => {
                        let idx = resolve_index(part, seq.len(), path)?;
                        &seq[idx]
                    }
                    _ => {
                        return Err(Error::Path(format!(
                            "invalid path '{}', cannot traverse scalar at '{}'.",
                            path, part
                        )));
                    }
                }
            }
            _ => {
                return Err(Error::Path(format!(
                    "invalid path '{}', cannot traverse scalar at '{}'.",
                    path, part
                )));
            }
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

    let len = match target {
        Value::Sequence(seq) => seq.len(),
        Value::Mapping(map) => map.len(),
        Value::Tagged(t) => match &t.value {
            Value::Sequence(seq) => seq.len(),
            Value::Mapping(map) => map.len(),
            _ => {
                return Err(Error::Type(format!(
                    "get-length does not support '{}' type. Please provide or select a sequence or struct.",
                    value_to_type_name(target)
                )))
            }
        },
        _ => {
            return Err(Error::Type(format!(
                "get-length does not support '{}' type. Please provide or select a sequence or struct.",
                value_to_type_name(target)
            )))
        }
    };

    Ok(Value::Number(Number::UInt(len as u64)))
}

// =============================================================================
// Keys, Values, Key-Values (Value-based)
// =============================================================================

pub fn keys(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let target = get_at_path(value, path)?;

    let map = match target {
        Value::Mapping(m) => m,
        Value::Tagged(t) => match &t.value {
            Value::Mapping(m) => m,
            _ => {
                return Err(Error::Type(format!(
                    "keys does not support '{}' type. Please provide or select a struct.",
                    value_to_type_name(target)
                )))
            }
        },
        _ => {
            return Err(Error::Type(format!(
                "keys does not support '{}' type. Please provide or select a struct.",
                value_to_type_name(target)
            )))
        }
    };

    let keys: Vec<Value> = map.keys().cloned().collect();
    Ok(Value::Sequence(keys))
}

pub fn values(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let target = get_at_path(value, path)?;

    let map = match target {
        Value::Mapping(m) => m,
        Value::Tagged(t) => match &t.value {
            Value::Mapping(m) => m,
            _ => {
                return Err(Error::Type(format!(
                    "values does not support '{}' type. Please provide or select a struct.",
                    value_to_type_name(target)
                )))
            }
        },
        _ => {
            return Err(Error::Type(format!(
                "values does not support '{}' type. Please provide or select a struct.",
                value_to_type_name(target)
            )))
        }
    };

    let vals: Vec<Value> = map.values().cloned().collect();
    Ok(Value::Sequence(vals))
}

pub fn get_values(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let target = get_at_path(value, path)?;

    match target {
        Value::Sequence(seq) => Ok(Value::Sequence(seq.clone())),
        Value::Mapping(map) => {
            let mut result = Vec::new();
            for (k, v) in map {
                result.push(k.clone());
                result.push(v.clone());
            }
            Ok(Value::Sequence(result))
        }
        Value::Tagged(t) => match &t.value {
            Value::Sequence(seq) => Ok(Value::Sequence(seq.clone())),
            Value::Mapping(map) => {
                let mut result = Vec::new();
                for (k, v) in map {
                    result.push(k.clone());
                    result.push(v.clone());
                }
                Ok(Value::Sequence(result))
            }
            _ => Err(Error::Type(format!(
                "get-values does not support '{}' type. Please provide or select a sequence or struct.",
                value_to_type_name(target)
            ))),
        },
        _ => Err(Error::Type(format!(
            "get-values does not support '{}' type. Please provide or select a sequence or struct.",
            value_to_type_name(target)
        ))),
    }
}

pub fn key_values(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let target = get_at_path(value, path)?;

    let map = match target {
        Value::Mapping(m) => m,
        Value::Tagged(t) => match &t.value {
            Value::Mapping(m) => m,
            _ => {
                return Err(Error::Type(format!(
                    "key-values does not support '{}' type. Please provide or select a struct.",
                    value_to_type_name(target)
                )))
            }
        },
        _ => {
            return Err(Error::Type(format!(
                "key-values does not support '{}' type. Please provide or select a struct.",
                value_to_type_name(target)
            )))
        }
    };

    let mut result = Vec::new();
    for (k, v) in map {
        result.push(k.clone());
        result.push(v.clone());
    }
    Ok(Value::Sequence(result))
}
