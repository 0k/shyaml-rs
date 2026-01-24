//! YAML merge operations for the `apply` command.
//!
//! Provides merge policies, inline merge directives, and overlay application.

use super::error::Error;
use crate::tag::{parse_tag, MergeOp};
use fyaml::{TaggedValue, Value};
use std::collections::HashMap;

// =============================================================================
// Merge Policy
// =============================================================================

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

// =============================================================================
// Merge Directive Extraction
// =============================================================================

fn extract_merge_directive(value: Value) -> Result<(Option<MergeOp>, Value), Error> {
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

fn validate_merge_op_for_type(op: &MergeOp, value: &Value, path: &str) -> Result<(), Error> {
    if !matches!(op, MergeOp::Append | MergeOp::Prepend) {
        return Ok(());
    }

    let inner = match value {
        Value::Tagged(t) => &t.value,
        other => other,
    };

    if matches!(inner, Value::Sequence(_)) {
        return Ok(());
    }

    let location = if path.is_empty() {
        "at root".to_string()
    } else {
        format!("at '{}'", path)
    };

    Err(Error::Type(format!(
        "Invalid merge directive {}: !merge:{} can only be used on sequences, got {}",
        location,
        op,
        value_type_name(inner)
    )))
}

fn value_type_name(value: &Value) -> &'static str {
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

// =============================================================================
// Merge Operations
// =============================================================================

fn merge_values(
    base: Value,
    overlay: Value,
    path: &str,
    policies: &HashMap<String, MergePolicy>,
) -> Result<Value, Error> {
    let (inline_op, stripped_overlay) = extract_merge_directive(overlay)?;

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

    apply_default_merge(base, stripped_overlay, path, policies)
}

fn apply_policy(
    policy: MergePolicy,
    base: Value,
    overlay: Value,
    path: &str,
    policies: &HashMap<String, MergePolicy>,
) -> Result<Value, Error> {
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

fn apply_default_merge(
    base: Value,
    overlay: Value,
    path: &str,
    policies: &HashMap<String, MergePolicy>,
) -> Result<Value, Error> {
    let overlay_inner = match &overlay {
        Value::Tagged(t) => &t.value,
        other => other,
    };

    let base_inner = match &base {
        Value::Tagged(t) => &t.value,
        other => other,
    };

    match (base_inner, overlay_inner) {
        (Value::Null, Value::Null) => Ok(overlay),
        (_, Value::Null) => Ok(base),
        (Value::Null, _) => Ok(overlay),

        (Value::Mapping(_), Value::Mapping(_)) => {
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
                    let (_, stripped) = extract_merge_directive(overlay_value)?;
                    stripped
                };
                result.insert(key, merged_value);
            }
            Ok(Value::Mapping(result))
        }

        (Value::Sequence(_), Value::Sequence(_)) => {
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

        (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_))
        | (Value::Bool(_), Value::Number(_))
        | (Value::Bool(_), Value::String(_))
        | (Value::Number(_), Value::Bool(_))
        | (Value::Number(_), Value::String(_))
        | (Value::String(_), Value::Bool(_))
        | (Value::String(_), Value::Number(_)) => Ok(overlay),

        _ => {
            let base_type = value_type_name(base_inner);
            let overlay_type = value_type_name(overlay_inner);
            let location = if path.is_empty() {
                "at root".to_string()
            } else {
                format!("at '{}'", path)
            };
            Err(Error::Type(format!(
                "Type mismatch {}: cannot merge {} with {}",
                location, base_type, overlay_type
            )))
        }
    }
}

// =============================================================================
// Apply (Merge Overlays)
// =============================================================================

/// Apply overlay files to a base value.
pub fn apply(
    overlay_paths: &[String],
    policies: &HashMap<String, MergePolicy>,
    base: Value,
) -> Result<Value, Error> {
    let mut result = base;

    for overlay_path in overlay_paths {
        let overlay_str = std::fs::read_to_string(overlay_path)
            .map_err(|e| Error::Io(format!("Failed to read '{}': {}", overlay_path, e)))?;

        let overlay: Value = if overlay_str.trim().is_empty() {
            Value::Null
        } else {
            overlay_str
                .parse()
                .map_err(|e| Error::Base(format!("Failed to parse '{}': {}", overlay_path, e)))?
        };

        result = merge_values(result, overlay, "", policies)?;
    }

    Ok(result)
}
