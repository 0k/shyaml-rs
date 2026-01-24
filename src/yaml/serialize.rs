//! Serialization utilities for YAML values.

use super::error::Error;
use fyaml::ValueRef;
pub use fyaml::{Number, Value};

// =============================================================================
// Zero-Copy Serialization (ValueRef)
// =============================================================================

/// Serialize ValueRef to YAML string.
pub fn serialize_ref(value: ValueRef<'_>) -> Result<String, Error> {
    value.as_node().emit().map_err(Error::from)
}

/// Raw output for ValueRef (returns owned String).
///
/// For scalars, returns the string representation without YAML formatting.
/// For complex types, emits as YAML.
pub fn serialize_raw_ref(value: ValueRef<'_>) -> String {
    if value.is_null() {
        return String::new();
    }
    if let Some(s) = value.as_str() {
        return s.to_string();
    }
    if let Some(b) = value.as_bool() {
        return b.to_string();
    }
    if let Some(n) = value.as_i64() {
        return n.to_string();
    }
    if let Some(n) = value.as_f64() {
        // Check if it's a special float value
        if n.is_nan() {
            return ".nan".to_string();
        }
        if n.is_infinite() {
            return if n.is_sign_positive() {
                ".inf".to_string()
            } else {
                "-.inf".to_string()
            };
        }
        return n.to_string();
    }
    // Complex types: emit as YAML
    value.as_node().emit().unwrap_or_default()
}

// =============================================================================
// Owned Value Serialization
// =============================================================================

/// Serialize Value to YAML string.
pub fn serialize(value: &Value) -> Result<String, Error> {
    value
        .to_yaml_string()
        .map_err(|e| Error::Base(format!("Failed to serialize YAML: {}", e)))
}

/// Serialize Value to raw string (without YAML formatting).
pub fn serialize_raw(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => match n {
            Number::Int(i) => i.to_string(),
            Number::UInt(u) => u.to_string(),
            Number::Float(f) => {
                if f.is_nan() {
                    ".nan".to_string()
                } else if f.is_infinite() {
                    if f.is_sign_positive() {
                        ".inf".to_string()
                    } else {
                        "-.inf".to_string()
                    }
                } else {
                    f.to_string()
                }
            }
        },
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        _ => serialize(value).unwrap_or_default(),
    }
}
