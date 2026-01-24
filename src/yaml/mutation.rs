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

/// Parse a string as either a literal string or YAML.
pub fn parse_value(value_str: &str, parse_as_yaml: bool) -> Result<Value, Error> {
    if parse_as_yaml {
        value_str
            .parse()
            .map_err(|e| Error::Base(format!("Failed to parse value as YAML: {}", e)))
    } else {
        Ok(Value::String(value_str.to_string()))
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
