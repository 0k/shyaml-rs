//! YAML merge operations for the `apply` command.
//!
//! Provides merge policies, inline merge directives, and overlay application.

use super::error::Error;
use super::InnerValue;
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

    if value.is_inner_sequence() {
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
        value_type_name(value.inner())
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
            let overlay_tag = match &overlay {
                Value::Tagged(t) => Some(&t.tag),
                _ => None,
            };

            if let (Value::Sequence(base_seq), Value::Sequence(overlay_seq)) =
                (base.inner(), overlay.inner())
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
    let overlay_inner = overlay.inner();
    let base_inner = base.inner();

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
                if overlay_value.is_inner_null() {
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

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use fyaml::Number;
    use indexmap::indexmap;

    // -------------------------------------------------------------------------
    // MergePolicy Parsing Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_merge_policy_from_str_merge() {
        assert_eq!("merge".parse::<MergePolicy>().unwrap(), MergePolicy::Merge);
        assert_eq!("MERGE".parse::<MergePolicy>().unwrap(), MergePolicy::Merge);
    }

    #[test]
    fn test_merge_policy_from_str_replace() {
        assert_eq!(
            "replace".parse::<MergePolicy>().unwrap(),
            MergePolicy::Replace
        );
    }

    #[test]
    fn test_merge_policy_from_str_prepend() {
        assert_eq!(
            "prepend".parse::<MergePolicy>().unwrap(),
            MergePolicy::Prepend
        );
    }

    #[test]
    fn test_merge_policy_from_str_invalid() {
        let err = "invalid".parse::<MergePolicy>().unwrap_err();
        assert!(err.contains("Invalid merge policy"));
    }

    // -------------------------------------------------------------------------
    // parse_merge_policies Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_merge_policies_empty() {
        let policies = parse_merge_policies(None).unwrap();
        assert!(policies.is_empty());
    }

    #[test]
    fn test_parse_merge_policies_single() {
        let args = vec!["config=replace".to_string()];
        let policies = parse_merge_policies(Some(&args)).unwrap();
        assert_eq!(policies.get("config"), Some(&MergePolicy::Replace));
    }

    #[test]
    fn test_parse_merge_policies_multiple() {
        let args = vec![
            "config=replace".to_string(),
            "items=prepend".to_string(),
            "nested.path=merge".to_string(),
        ];
        let policies = parse_merge_policies(Some(&args)).unwrap();
        assert_eq!(policies.get("config"), Some(&MergePolicy::Replace));
        assert_eq!(policies.get("items"), Some(&MergePolicy::Prepend));
        assert_eq!(policies.get("nested.path"), Some(&MergePolicy::Merge));
    }

    #[test]
    fn test_parse_merge_policies_invalid_format() {
        let args = vec!["invalid-no-equals".to_string()];
        let err = parse_merge_policies(Some(&args)).unwrap_err();
        assert!(err.contains("expected format PATH=POLICY"));
    }

    #[test]
    fn test_parse_merge_policies_invalid_policy() {
        let args = vec!["config=unknown".to_string()];
        let err = parse_merge_policies(Some(&args)).unwrap_err();
        assert!(err.contains("Invalid merge policy"));
    }

    // -------------------------------------------------------------------------
    // value_type_name Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_value_type_name_all_types() {
        assert_eq!(value_type_name(&Value::Null), "null");
        assert_eq!(value_type_name(&Value::Bool(true)), "bool");
        assert_eq!(value_type_name(&Value::Number(Number::Int(1))), "number");
        assert_eq!(value_type_name(&Value::String("s".into())), "string");
        assert_eq!(value_type_name(&Value::Sequence(vec![])), "sequence");
        assert_eq!(value_type_name(&Value::Mapping(indexmap! {})), "mapping");
    }

    // -------------------------------------------------------------------------
    // extract_merge_directive Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_merge_directive_no_tag() {
        let value = Value::String("hello".to_string());
        let (op, stripped) = extract_merge_directive(value).unwrap();
        assert!(op.is_none());
        assert_eq!(stripped, Value::String("hello".to_string()));
    }

    #[test]
    fn test_extract_merge_directive_replace() {
        let value = Value::Tagged(Box::new(TaggedValue {
            tag: "!merge:replace".to_string(),
            value: Value::String("new".to_string()),
        }));
        let (op, stripped) = extract_merge_directive(value).unwrap();
        assert_eq!(op, Some(MergeOp::Replace));
        assert_eq!(stripped, Value::String("new".to_string()));
    }

    #[test]
    fn test_extract_merge_directive_compound_tag() {
        let value = Value::Tagged(Box::new(TaggedValue {
            tag: "!custom;merge:prepend".to_string(),
            value: Value::Sequence(vec![]),
        }));
        let (op, stripped) = extract_merge_directive(value).unwrap();
        assert_eq!(op, Some(MergeOp::Prepend));
        // Remaining tag should be preserved
        if let Value::Tagged(t) = stripped {
            assert_eq!(t.tag, "!custom");
        } else {
            panic!("Expected tagged value with remaining tag");
        }
    }

    // -------------------------------------------------------------------------
    // merge_values Tests - Scalar Replacement
    // -------------------------------------------------------------------------

    #[test]
    fn test_merge_scalars_overlay_wins() {
        let base = Value::String("old".to_string());
        let overlay = Value::String("new".to_string());
        let policies = HashMap::new();

        let result = merge_values(base, overlay, "", &policies).unwrap();
        assert_eq!(result, Value::String("new".to_string()));
    }

    #[test]
    fn test_merge_null_overlay_preserves_base() {
        let base = Value::String("keep".to_string());
        let overlay = Value::Null;
        let policies = HashMap::new();

        let result = merge_values(base, overlay, "", &policies).unwrap();
        assert_eq!(result, Value::String("keep".to_string()));
    }

    #[test]
    fn test_merge_null_base_uses_overlay() {
        let base = Value::Null;
        let overlay = Value::String("new".to_string());
        let policies = HashMap::new();

        let result = merge_values(base, overlay, "", &policies).unwrap();
        assert_eq!(result, Value::String("new".to_string()));
    }

    // -------------------------------------------------------------------------
    // merge_values Tests - Mapping Merge
    // -------------------------------------------------------------------------

    #[test]
    fn test_merge_mappings_deep() {
        let base = Value::Mapping(indexmap! {
            Value::String("a".to_string()) => Value::Number(Number::Int(1)),
            Value::String("b".to_string()) => Value::Number(Number::Int(2)),
        });
        let overlay = Value::Mapping(indexmap! {
            Value::String("b".to_string()) => Value::Number(Number::Int(20)),
            Value::String("c".to_string()) => Value::Number(Number::Int(3)),
        });
        let policies = HashMap::new();

        let result = merge_values(base, overlay, "", &policies).unwrap();
        if let Value::Mapping(map) = result {
            assert_eq!(map.len(), 3);
            assert_eq!(
                map.get(&Value::String("a".to_string())),
                Some(&Value::Number(Number::Int(1)))
            );
            assert_eq!(
                map.get(&Value::String("b".to_string())),
                Some(&Value::Number(Number::Int(20)))
            );
            assert_eq!(
                map.get(&Value::String("c".to_string())),
                Some(&Value::Number(Number::Int(3)))
            );
        } else {
            panic!("Expected mapping");
        }
    }

    #[test]
    fn test_merge_mapping_null_deletes_key() {
        let base = Value::Mapping(indexmap! {
            Value::String("keep".to_string()) => Value::Number(Number::Int(1)),
            Value::String("remove".to_string()) => Value::Number(Number::Int(2)),
        });
        let overlay = Value::Mapping(indexmap! {
            Value::String("remove".to_string()) => Value::Null,
        });
        let policies = HashMap::new();

        let result = merge_values(base, overlay, "", &policies).unwrap();
        if let Value::Mapping(map) = result {
            assert_eq!(map.len(), 1);
            assert!(map.contains_key(&Value::String("keep".to_string())));
            assert!(!map.contains_key(&Value::String("remove".to_string())));
        } else {
            panic!("Expected mapping");
        }
    }

    // -------------------------------------------------------------------------
    // merge_values Tests - Sequence Merge
    // -------------------------------------------------------------------------

    #[test]
    fn test_merge_sequences_append_dedup() {
        let base = Value::Sequence(vec![
            Value::String("a".to_string()),
            Value::String("b".to_string()),
            Value::String("c".to_string()),
        ]);
        let overlay = Value::Sequence(vec![
            Value::String("b".to_string()), // duplicate
            Value::String("d".to_string()),
        ]);
        let policies = HashMap::new();

        let result = merge_values(base, overlay, "", &policies).unwrap();
        if let Value::Sequence(seq) = result {
            // [a, c, b, d] - b moved to where overlay placed it
            assert_eq!(seq.len(), 4);
            assert_eq!(seq[0], Value::String("a".to_string()));
            assert_eq!(seq[1], Value::String("c".to_string()));
            assert_eq!(seq[2], Value::String("b".to_string()));
            assert_eq!(seq[3], Value::String("d".to_string()));
        } else {
            panic!("Expected sequence");
        }
    }

    // -------------------------------------------------------------------------
    // merge_values Tests - Policy Override
    // -------------------------------------------------------------------------

    #[test]
    fn test_merge_with_replace_policy() {
        let base = Value::Mapping(indexmap! {
            Value::String("a".to_string()) => Value::Number(Number::Int(1)),
            Value::String("b".to_string()) => Value::Number(Number::Int(2)),
        });
        let overlay = Value::Mapping(indexmap! {
            Value::String("c".to_string()) => Value::Number(Number::Int(3)),
        });
        let mut policies = HashMap::new();
        policies.insert("".to_string(), MergePolicy::Replace);

        let result = merge_values(base, overlay, "", &policies).unwrap();
        if let Value::Mapping(map) = result {
            assert_eq!(map.len(), 1);
            assert!(map.contains_key(&Value::String("c".to_string())));
        } else {
            panic!("Expected mapping");
        }
    }

    #[test]
    fn test_merge_sequence_with_prepend_policy() {
        let base = Value::Sequence(vec![
            Value::String("a".to_string()),
            Value::String("b".to_string()),
        ]);
        let overlay = Value::Sequence(vec![
            Value::String("x".to_string()),
            Value::String("y".to_string()),
        ]);
        let mut policies = HashMap::new();
        policies.insert("".to_string(), MergePolicy::Prepend);

        let result = merge_values(base, overlay, "", &policies).unwrap();
        if let Value::Sequence(seq) = result {
            // [x, y, a, b] - overlay comes first
            assert_eq!(seq.len(), 4);
            assert_eq!(seq[0], Value::String("x".to_string()));
            assert_eq!(seq[1], Value::String("y".to_string()));
            assert_eq!(seq[2], Value::String("a".to_string()));
            assert_eq!(seq[3], Value::String("b".to_string()));
        } else {
            panic!("Expected sequence");
        }
    }

    // -------------------------------------------------------------------------
    // merge_values Tests - Type Mismatch Errors
    // -------------------------------------------------------------------------

    #[test]
    fn test_merge_type_mismatch_mapping_sequence() {
        let base = Value::Mapping(indexmap! {});
        let overlay = Value::Sequence(vec![]);
        let policies = HashMap::new();

        let err = merge_values(base, overlay, "test.path", &policies).unwrap_err();
        assert!(matches!(err, Error::Type(_)));
        assert!(err.to_string().contains("cannot merge"));
        assert!(err.to_string().contains("at 'test.path'"));
    }

    #[test]
    fn test_merge_type_mismatch_at_root() {
        let base = Value::Mapping(indexmap! {});
        let overlay = Value::Sequence(vec![]);
        let policies = HashMap::new();

        let err = merge_values(base, overlay, "", &policies).unwrap_err();
        assert!(err.to_string().contains("at root"));
    }

    // -------------------------------------------------------------------------
    // Inline Merge Directive Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_inline_merge_replace() {
        let base = Value::Mapping(indexmap! {
            Value::String("a".to_string()) => Value::Number(Number::Int(1)),
        });
        let overlay = Value::Tagged(Box::new(TaggedValue {
            tag: "!merge:replace".to_string(),
            value: Value::Mapping(indexmap! {
                Value::String("b".to_string()) => Value::Number(Number::Int(2)),
            }),
        }));
        let policies = HashMap::new();

        let result = merge_values(base, overlay, "", &policies).unwrap();
        if let Value::Mapping(map) = result {
            assert_eq!(map.len(), 1);
            assert!(map.contains_key(&Value::String("b".to_string())));
        } else {
            panic!("Expected mapping");
        }
    }

    #[test]
    fn test_inline_append_on_non_sequence_error() {
        let base = Value::Mapping(indexmap! {});
        let overlay = Value::Tagged(Box::new(TaggedValue {
            tag: "!merge:append".to_string(),
            value: Value::Mapping(indexmap! {}),
        }));
        let policies = HashMap::new();

        let err = merge_values(base, overlay, "config", &policies).unwrap_err();
        assert!(matches!(err, Error::Type(_)));
        assert!(err.to_string().contains("!merge:append"));
        assert!(err.to_string().contains("sequences"));
    }

    #[test]
    fn test_cli_policy_overrides_inline_directive() {
        let base = Value::Sequence(vec![Value::String("a".to_string())]);
        // Inline says prepend
        let overlay = Value::Tagged(Box::new(TaggedValue {
            tag: "!merge:prepend".to_string(),
            value: Value::Sequence(vec![Value::String("x".to_string())]),
        }));
        // CLI says replace
        let mut policies = HashMap::new();
        policies.insert("".to_string(), MergePolicy::Replace);

        let result = merge_values(base, overlay, "", &policies).unwrap();
        // Replace wins - only overlay content
        if let Value::Sequence(seq) = result {
            assert_eq!(seq.len(), 1);
            assert_eq!(seq[0], Value::String("x".to_string()));
        } else {
            panic!("Expected sequence");
        }
    }
}
