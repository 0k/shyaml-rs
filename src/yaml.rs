use crate::tag::{parse_tag, MergeOp, TagError};
use fyaml::document::{FyParser, Parse};
use indexmap::IndexMap;

use std::io;

pub use fyaml::Value;

#[derive(Debug)]
pub enum Error {
    FyError(String),
    IoError(String),
    PathError(String),
    TypeError(String),
    BaseError(String),
}

impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::IoError(e.to_string())
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Error::BaseError(e)
    }
}

impl From<&str> for Error {
    fn from(e: &str) -> Self {
        Error::BaseError(e.to_string())
    }
}

impl From<TagError> for Error {
    fn from(e: TagError) -> Self {
        Error::BaseError(e.to_string())
    }
}

// Implementation of std::fmt::Display
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let e = match self {
            Error::FyError(e)
            | Error::IoError(e)
            | Error::PathError(e)
            | Error::TypeError(e)
            | Error::BaseError(e) => e,
        };
        write!(f, "{}", e)
    }
}

use crate::yaml::Error::*;
use std::collections::HashMap;

/// Merge policy for specific paths
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MergePolicy {
    /// Deep recursive merge (default for mappings)
    Merge,
    /// Replace entirely with overlay value
    Replace,
    /// Prepend overlay sequence to base sequence
    Prepend,
}

impl std::str::FromStr for MergePolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "merge" => Ok(MergePolicy::Merge),
            "replace" => Ok(MergePolicy::Replace),
            "prepend" => Ok(MergePolicy::Prepend),
            _ => Err(format!(
                "Invalid merge policy '{}': expected merge, replace, or prepend",
                s
            )),
        }
    }
}

/// Parse merge policy specifications from CLI arguments
/// Format: "path=policy" where policy is merge|replace|prepend
pub fn parse_merge_policies(
    args: Option<&Vec<String>>,
) -> Result<HashMap<String, MergePolicy>, String> {
    let mut policies = HashMap::new();

    if let Some(specs) = args {
        for spec in specs {
            let parts: Vec<&str> = spec.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(format!(
                    "Invalid merge policy '{}': expected format PATH=POLICY",
                    spec
                ));
            }
            let path = parts[0].trim();
            let policy: MergePolicy = parts[1].trim().parse()?;
            policies.insert(path.to_string(), policy);
        }
    }

    Ok(policies)
}

/// Split a dot-notation path into its components.
///
/// Handles escape sequences: `\.` for literal dots, `\\` for literal backslashes.
/// For example, `a.b\.c.d` becomes `["a", "b.c", "d"]`.
fn split_path(path: &str) -> Vec<String> {
    let mut elements = Vec::new();
    let mut escaped = false;
    let mut element = String::new();

    for c in path.chars() {
        if escaped {
            escaped = false;
            element.push(c);
            continue;
        }
        match c {
            '\\' => escaped = true,
            '.' => {
                elements.push(element.clone());
                element.clear();
            }
            _ => element.push(c),
        }
    }
    elements.push(element);
    elements
}

fn resolve_index(part: &str, len: usize, full_path: &str) -> Result<usize, Error> {
    let idx: i64 = part.parse().map_err(|_| {
        PathError(format!(
            "invalid path '{}', non-integer index '{}' provided on a sequence.",
            full_path, part
        ))
    })?;

    let resolved = if idx < 0 {
        let abs_idx = (-idx) as usize;
        if abs_idx > len {
            return Err(PathError(format!(
                "invalid path '{}', index {} is out of range ({} elements in sequence).",
                full_path, idx, len
            )));
        }
        len - abs_idx
    } else {
        idx as usize
    };

    if resolved >= len {
        return Err(PathError(format!(
            "invalid path '{}', index {} is out of range ({} elements in sequence).",
            full_path, idx, len
        )));
    }

    Ok(resolved)
}

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
    Err(PathError(format!(
        "invalid path '{}', missing key '{}' in struct.",
        path, part
    )))
}

fn get_at_path<'a>(value: &'a Value, path: Option<&str>) -> Result<&'a Value, Error> {
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
                        return Err(PathError(format!(
                            "invalid path '{}', cannot traverse scalar at '{}'.",
                            path, part
                        )));
                    }
                }
            }
            _ => {
                return Err(PathError(format!(
                    "invalid path '{}', cannot traverse scalar at '{}'.",
                    path, part
                )));
            }
        };
    }

    Ok(current)
}

/// Convert a path from the form "foo.bar.0.baz" to "foo/bar/0/baz"
///
/// This is necessary because libfyaml uses '/' as a path separator, but
/// shyaml historically uses '.' as a path separator.
///
/// In shyaml, `\.` is used to escape a literal '.' in a key name, but this
/// does not need to be escaped in libfyaml.
///
pub fn get_value(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let result = get_at_path(value, path)?;
    Ok(result.clone())
}

pub fn streaming_documents_from_stdin(
    line_buffered: bool,
) -> Result<impl Iterator<Item = Result<Value, Error>>, Error> {
    let parser = FyParser::from_stdin_with_line_buffer(line_buffered)
        .map_err(|e| BaseError(format!("Failed to create parser: {}", e)))?;

    Ok(parser.doc_iter().map(|doc| {
        let root_node = match doc.root_node() {
            Some(node) => node,
            None => return Ok(Value::Null),
        };
        Value::from_node(&root_node)
            .map_err(|e| BaseError(format!("Failed to convert document: {}", e)))
    }))
}

pub fn serialize(value: &Value) -> Result<String, Error> {
    value
        .to_yaml_string()
        .map_err(|e| BaseError(format!("Failed to serialize YAML: {}", e)))
}

pub fn serialize_raw(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => match n {
            fyaml::Number::Int(i) => i.to_string(),
            fyaml::Number::UInt(u) => u.to_string(),
            fyaml::Number::Float(f) => f.to_string(),
        },
        Value::Bool(b) => b.to_string(),
        Value::Null => "".to_string(),
        _ => serialize(value).unwrap_or_default(),
    }
}

fn is_float_number(n: &fyaml::Number) -> bool {
    match n {
        fyaml::Number::Float(_) => true,
        _ => false,
    }
}

fn value_to_type_name(value: &Value) -> &'static str {
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
                return Err(TypeError(format!(
                    "get-length does not support '{}' type. Please provide or select a sequence or struct.",
                    value_to_type_name(target)
                )))
            }
        },
        _ => {
            return Err(TypeError(format!(
                "get-length does not support '{}' type. Please provide or select a sequence or struct.",
                value_to_type_name(target)
            )))
        }
    };

    Ok(Value::Number(fyaml::Number::UInt(len as u64)))
}

///
/// use fy_library_version() to get the version of the libfyaml library
pub fn get_version() -> Result<String, String> {
    fyaml::get_c_version()
}

pub fn keys(path: Option<&str>, value: &Value) -> Result<Value, Error> {
    let target = get_at_path(value, path)?;

    let map = match target {
        Value::Mapping(m) => m,
        Value::Tagged(t) => match &t.value {
            Value::Mapping(m) => m,
            _ => {
                return Err(TypeError(format!(
                    "keys does not support '{}' type. Please provide or select a struct.",
                    value_to_type_name(target)
                )))
            }
        },
        _ => {
            return Err(TypeError(format!(
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
                return Err(TypeError(format!(
                    "values does not support '{}' type. Please provide or select a struct.",
                    value_to_type_name(target)
                )))
            }
        },
        _ => {
            return Err(TypeError(format!(
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
            _ => Err(TypeError(format!(
                "get-values does not support '{}' type. Please provide or select a sequence or struct.",
                value_to_type_name(target)
            ))),
        },
        _ => Err(TypeError(format!(
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
                return Err(TypeError(format!(
                    "key-values does not support '{}' type. Please provide or select a struct.",
                    value_to_type_name(target)
                )))
            }
        },
        _ => {
            return Err(TypeError(format!(
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

/// Merge two YAML values according to merge rules:
/// - Scalars: child replaces parent
/// - Mappings: deep recursive merge
/// - Sequences: append child to parent with deduplication
///
/// Merge policies can override default behavior for specific paths:
/// - Replace: overlay completely replaces base
/// - Prepend: overlay sequence is prepended to base sequence
/// - Merge: default deep merge behavior

fn extract_merge_directive(value: fyaml::Value) -> Result<(Option<MergeOp>, fyaml::Value), Error> {
    use fyaml::value::TaggedValue;
    use fyaml::Value;

    match value {
        Value::Tagged(tagged) => {
            let parsed = parse_tag(&tagged.tag)?;
            let stripped_value = match parsed.remaining {
                Some(remaining_tag) => Value::Tagged(Box::new(TaggedValue {
                    tag: remaining_tag,
                    value: tagged.value,
                })),
                None => tagged.value,
            };
            Ok((parsed.merge_op, stripped_value))
        }
        other => Ok((None, other)),
    }
}

fn validate_merge_op_for_type(op: &MergeOp, value: &fyaml::Value, path: &str) -> Result<(), Error> {
    if !matches!(op, MergeOp::Append | MergeOp::Prepend) {
        return Ok(());
    }

    let inner = match value {
        fyaml::Value::Tagged(t) => &t.value,
        other => other,
    };

    if matches!(inner, fyaml::Value::Sequence(_)) {
        return Ok(());
    }

    let location = if path.is_empty() {
        "at root".to_string()
    } else {
        format!("at '{}'", path)
    };

    Err(TypeError(format!(
        "Invalid merge directive {}: !merge:{} can only be used on sequences, got {}",
        location,
        op,
        value_type_name(inner)
    )))
}

/// Merge two YAML values according to merge rules:
/// - Scalars: child replaces parent
/// - Mappings: deep recursive merge
/// - Sequences: append child to parent with deduplication
///
/// Merge policies can be specified via:
/// 1. CLI arguments (--merge-policy PATH=POLICY) - highest priority
/// 2. Inline tags in overlay (!merge:replace, !merge:append, !merge:prepend)
/// 3. Default behavior (merge for mappings, append for sequences, replace for scalars)
///
/// Inline merge tags are always stripped from the output.
fn merge_values(
    base: fyaml::Value,
    overlay: fyaml::Value,
    path: &str,
    policies: &HashMap<String, MergePolicy>,
) -> Result<fyaml::Value, Error> {
    // Extract merge directive from overlay (if any) and strip the merge tag
    let (inline_op, stripped_overlay) = extract_merge_directive(overlay)?;

    // Determine effective policy: CLI > inline tag > default
    let cli_policy = policies.get(path);

    if let Some(policy) = cli_policy {
        return apply_policy(*policy, base, stripped_overlay, path, policies);
    }

    if let Some(op) = inline_op {
        validate_merge_op_for_type(&op, &stripped_overlay, path)?;

        let policy = match op {
            MergeOp::Replace => MergePolicy::Replace,
            MergeOp::Append => MergePolicy::Merge,
            MergeOp::Prepend => MergePolicy::Prepend,
        };
        return apply_policy(policy, base, stripped_overlay, path, policies);
    }

    // Default behavior with stripped overlay
    apply_default_merge(base, stripped_overlay, path, policies)
}

/// Apply a specific merge policy
fn apply_policy(
    policy: MergePolicy,
    base: fyaml::Value,
    overlay: fyaml::Value,
    path: &str,
    policies: &HashMap<String, MergePolicy>,
) -> Result<fyaml::Value, Error> {
    use fyaml::value::TaggedValue;
    use fyaml::Value;

    match policy {
        MergePolicy::Replace => Ok(overlay),
        MergePolicy::Prepend => {
            let (overlay_tag, overlay_inner) = match &overlay {
                Value::Tagged(t) => (Some(&t.tag), &t.value),
                other => (None, other),
            };
            let base_inner = match &base {
                Value::Tagged(t) => &t.value,
                other => other,
            };

            if let (Value::Sequence(base_seq), Value::Sequence(overlay_seq)) =
                (base_inner, overlay_inner)
            {
                let mut result = overlay_seq.clone();
                for elt in base_seq {
                    if !result.contains(elt) {
                        result.push(elt.clone());
                    }
                }
                let result_value = Value::Sequence(result);
                return Ok(match overlay_tag {
                    Some(tag) => Value::Tagged(Box::new(TaggedValue {
                        tag: tag.clone(),
                        value: result_value,
                    })),
                    None => result_value,
                });
            }
            Ok(overlay)
        }
        MergePolicy::Merge => apply_default_merge(base, overlay, path, policies),
    }
}

/// Apply default merge behavior based on types
fn apply_default_merge(
    base: fyaml::Value,
    overlay: fyaml::Value,
    path: &str,
    policies: &HashMap<String, MergePolicy>,
) -> Result<fyaml::Value, Error> {
    use fyaml::Value;

    // Get the inner value if overlay is tagged (for type comparison)
    // Note: merge tags already stripped, but other tags may remain
    let overlay_inner = match &overlay {
        Value::Tagged(t) => &t.value,
        other => other,
    };

    // Get the inner value if base is tagged (for type comparison)
    let base_inner = match &base {
        Value::Tagged(t) => &t.value,
        other => other,
    };

    match (base_inner, overlay_inner) {
        // Both null - return overlay (which might be tagged null)
        (Value::Null, Value::Null) => Ok(overlay),

        // Overlay is null - return base unchanged
        // Note: null-deletes-key is handled in mapping merge logic
        (_, Value::Null) => Ok(base),

        // Base is null - return overlay
        (Value::Null, _) => Ok(overlay),

        // Both are mappings - deep merge
        (Value::Mapping(_), Value::Mapping(_)) => {
            // Extract the actual mappings (handling tags)
            let base_map = match base {
                Value::Mapping(m) => m,
                Value::Tagged(t) => match t.value {
                    Value::Mapping(m) => m,
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            };
            let overlay_map = match overlay {
                Value::Mapping(m) => m,
                Value::Tagged(t) => match t.value {
                    Value::Mapping(m) => m,
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            };

            let mut result = base_map;
            for (key, overlay_value) in overlay_map {
                // Legacy behavior: explicit null value deletes the key
                let is_null = match &overlay_value {
                    Value::Null => true,
                    Value::Tagged(t) => matches!(t.value, Value::Null),
                    _ => false,
                };
                if is_null {
                    result.shift_remove(&key);
                    continue;
                }

                let key_str = match &key {
                    Value::String(s) => s.clone(),
                    _ => format!("{:?}", key),
                };
                let new_path = if path.is_empty() {
                    key_str
                } else {
                    format!("{}.{}", path, key_str)
                };

                let merged_value = if let Some(base_value) = result.get(&key) {
                    merge_values(base_value.clone(), overlay_value, &new_path, policies)?
                } else {
                    // New key - still need to strip any merge tags
                    let (_, stripped) = extract_merge_directive(overlay_value)?;
                    stripped
                };
                result.insert(key, merged_value);
            }
            Ok(Value::Mapping(result))
        }

        // Both are sequences - append with deduplication (legacy behavior)
        (Value::Sequence(_), Value::Sequence(_)) => {
            // Extract the actual sequences (handling tags)
            let base_seq = match base {
                Value::Sequence(s) => s,
                Value::Tagged(t) => match t.value {
                    Value::Sequence(s) => s,
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            };
            let overlay_seq = match overlay {
                Value::Sequence(s) => s,
                Value::Tagged(t) => match t.value {
                    Value::Sequence(s) => s,
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            };

            let mut result = base_seq;
            for elt in overlay_seq {
                if let Some(pos) = result.iter().position(|x| x == &elt) {
                    result.remove(pos);
                }
                result.push(elt);
            }
            Ok(Value::Sequence(result))
        }

        // Scalars (same or different types) - replace
        (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_))
        | (Value::Bool(_), Value::Number(_))
        | (Value::Bool(_), Value::String(_))
        | (Value::Number(_), Value::Bool(_))
        | (Value::Number(_), Value::String(_))
        | (Value::String(_), Value::Bool(_))
        | (Value::String(_), Value::Number(_)) => Ok(overlay),

        // Type mismatch (mapping vs sequence, scalar vs collection, etc.)
        _ => {
            let base_type = value_type_name(base_inner);
            let overlay_type = value_type_name(overlay_inner);
            let location = if path.is_empty() {
                "at root".to_string()
            } else {
                format!("at '{}'", path)
            };
            Err(TypeError(format!(
                "Type mismatch {}: cannot merge {} with {}",
                location, base_type, overlay_type
            )))
        }
    }
}

fn value_type_name(value: &fyaml::Value) -> &'static str {
    use fyaml::Value;
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Sequence(_) => "sequence",
        Value::Mapping(_) => "mapping",
        Value::Tagged(_) => "tagged",
    }
}

pub fn set_value(key: &str, new_value: Value, mut base: Value) -> Result<Value, Error> {
    if matches!(base, Value::Null) {
        base = Value::Mapping(Default::default());
    }
    set_value_at_path(&mut base, key, new_value)?;
    Ok(base)
}

pub fn parse_value(value_str: &str, parse_as_yaml: bool) -> Result<Value, Error> {
    if parse_as_yaml {
        value_str
            .parse()
            .map_err(|e| BaseError(format!("Failed to parse value as YAML: {}", e)))
    } else {
        Ok(Value::String(value_str.to_string()))
    }
}

fn resolve_sequence_index(path: &str, part: &str, seq_len: usize) -> Result<usize, Error> {
    let idx: i64 = part.parse().map_err(|_| {
        PathError(format!(
            "invalid path '{}', non-integer index '{}' provided on a sequence.",
            path, part
        ))
    })?;

    let resolved = if idx < 0 {
        let abs_idx = (-idx) as usize;
        if abs_idx > seq_len {
            return Err(PathError(format!(
                "invalid path '{}', index {} is out of range ({} elements in sequence).",
                path, idx, seq_len
            )));
        }
        seq_len - abs_idx
    } else {
        idx as usize
    };

    if resolved >= seq_len {
        return Err(PathError(format!(
            "invalid path '{}', index {} is out of range ({} elements in sequence).",
            path, idx, seq_len
        )));
    }

    Ok(resolved)
}

fn set_value_at_path(
    root: &mut fyaml::Value,
    path: &str,
    value: fyaml::Value,
) -> Result<(), Error> {
    use fyaml::Value;

    let path_parts = split_path(path);

    if path_parts.is_empty() {
        return Err(PathError("Empty path".to_string()));
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
                    let idx = resolve_sequence_index(path, part, seq.len())?;
                    seq[idx] = value;
                    return Ok(());
                }
                _ => {
                    return Err(PathError(format!(
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
                let idx = resolve_sequence_index(path, part, seq.len())?;
                current = &mut seq[idx];
            }
            _ => {
                return Err(PathError(format!(
                    "invalid path '{}', cannot traverse scalar at '{}'.",
                    path, part
                )));
            }
        }
    }

    Ok(())
}

pub fn del(key: &str, mut base: Value) -> Result<Value, Error> {
    if matches!(base, Value::Null) {
        return Err(PathError("Cannot delete from empty document".to_string()));
    }
    del_at_path(&mut base, key)?;
    Ok(base)
}

fn del_at_path(root: &mut fyaml::Value, path: &str) -> Result<(), Error> {
    use fyaml::Value;

    let path_parts = split_path(path);

    if path_parts.is_empty() || (path_parts.len() == 1 && path_parts[0].is_empty()) {
        return Err(PathError("Empty path".to_string()));
    }

    let mut current = root;

    for (i, part) in path_parts.iter().enumerate() {
        let is_last = i == path_parts.len() - 1;

        if is_last {
            return match current {
                Value::Mapping(map) => {
                    let key = Value::String(part.clone());
                    if map.shift_remove(&key).is_none() {
                        Err(PathError(format!(
                            "invalid path '{}', missing key '{}' in struct.",
                            path, part
                        )))
                    } else {
                        Ok(())
                    }
                }
                Value::Sequence(seq) => {
                    let idx = parse_seq_index(part, seq.len(), path)?;
                    seq.remove(idx);
                    Ok(())
                }
                _ => Err(PathError(format!(
                    "invalid path '{}', cannot delete from scalar.",
                    path
                ))),
            };
        }

        current = match current {
            Value::Mapping(map) => {
                let key = Value::String(part.clone());
                map.get_mut(&key).ok_or_else(|| {
                    PathError(format!(
                        "invalid path '{}', missing key '{}' in struct.",
                        path, part
                    ))
                })?
            }
            Value::Sequence(seq) => {
                let idx = parse_seq_index(part, seq.len(), path)?;
                &mut seq[idx]
            }
            _ => {
                return Err(PathError(format!(
                    "invalid path '{}', cannot traverse scalar at '{}'.",
                    path, part
                )));
            }
        };
    }

    Ok(())
}

fn parse_seq_index(part: &str, len: usize, path: &str) -> Result<usize, Error> {
    let idx: i64 = part.parse().map_err(|_| {
        PathError(format!(
            "invalid path '{}', non-integer index '{}' provided on a sequence.",
            path, part
        ))
    })?;
    let resolved = if idx < 0 { len as i64 + idx } else { idx };
    if resolved < 0 || resolved >= len as i64 {
        return Err(PathError(format!(
            "invalid path '{}', index {} is out of range ({} elements in sequence).",
            path, idx, len
        )));
    }
    Ok(resolved as usize)
}

pub fn apply(
    overlay_paths: &[String],
    policies: &HashMap<String, MergePolicy>,
    base: Value,
) -> Result<Value, Error> {
    let mut result = base;

    for overlay_path in overlay_paths {
        let overlay_str = std::fs::read_to_string(overlay_path)
            .map_err(|e| IoError(format!("Failed to read '{}': {}", overlay_path, e)))?;

        let overlay: Value = if overlay_str.trim().is_empty() {
            Value::Null
        } else {
            overlay_str
                .parse()
                .map_err(|e| BaseError(format!("Failed to parse '{}': {}", overlay_path, e)))?
        };

        result = merge_values(result, overlay, "", policies)?;
    }

    Ok(result)
}
