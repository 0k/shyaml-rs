//! Command chain planning and execution mode analysis.
//!
//! Determines whether a command chain can use DocMode (Editor-based mutations)
//! or must fall back to ValueMode (full Value cloning).

use super::def::Actions;

/// Execution mode for a command chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Use Editor-based mutations directly on Document.
    /// Only modified nodes are allocated (practical COW).
    DocMode,
    /// Fall back to Value-based pipeline.
    /// Requires full document-to-Value conversion.
    ValueMode,
}

/// Classification of an action's behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionKind {
    /// Mutation: modifies the document in place (set-value, del)
    Mutation,
    /// ReadOnly: reads but doesn't modify (get-value, get-type, get-length)
    ReadOnly,
    /// Derived: produces a different structure than input (keys, values, get-values, key-values)
    /// These cannot use DocMode because subsequent commands operate on the derived result.
    Derived,
    /// Complex: requires full Value processing (apply)
    Complex,
}

impl ActionKind {
    fn from_action(action: &Actions) -> Self {
        match action {
            // Mutations: can use Editor directly
            Actions::SetValue { .. } | Actions::Del { .. } => ActionKind::Mutation,

            // Read-only: just read from document, can use zero-copy
            Actions::GetValue { .. } | Actions::GetType { .. } | Actions::GetLength { .. } => {
                ActionKind::ReadOnly
            }

            // Derived: produce a different structure (sequence of keys/values)
            // The result is a Value, not the original document
            Actions::Keys { .. }
            | Actions::Keys0 { .. }
            | Actions::Values { .. }
            | Actions::Values0 { .. }
            | Actions::KeyValues { .. }
            | Actions::KeyValues0 { .. }
            | Actions::GetValues { .. }
            | Actions::GetValues0 { .. } => ActionKind::Derived,

            // Complex: requires full Value-based processing
            Actions::Apply { .. } => ActionKind::Complex,
        }
    }
}

/// Analyze a sequence of actions to determine the best execution mode.
///
/// # Rules:
/// - Single read-only or mutation: DocMode
/// - Pure mutation chain: DocMode  
/// - Mutations followed by a single read-only at end: DocMode
/// - Any derived action: ValueMode (because next action operates on derived result)
/// - Any complex action (apply): ValueMode
/// - Mixed mutations with non-final read-only: check if read-only produces a value for next action
///
/// # Examples:
/// - `set-value a 1` -> DocMode
/// - `set-value a 1 ; set-value b 2` -> DocMode
/// - `set-value a 1 ; del c ; get-value a` -> DocMode (final read-only)
/// - `keys foo` -> DocMode (single derived uses zero-copy)
/// - `keys foo ; get-value 0` -> ValueMode (derived action not at end)
/// - `apply overlay.yaml` -> ValueMode (complex)
/// - `set-value a 1 ; keys foo` -> ValueMode (derived in chain needs Value output)
pub fn analyze_chain(actions: &[Option<Actions>]) -> ExecutionMode {
    if actions.is_empty() {
        return ExecutionMode::DocMode;
    }

    // Single action case
    if actions.len() == 1 {
        if let Some(action) = &actions[0] {
            let kind = ActionKind::from_action(action);
            return match kind {
                ActionKind::Mutation | ActionKind::ReadOnly | ActionKind::Derived => {
                    ExecutionMode::DocMode
                }
                ActionKind::Complex => ExecutionMode::ValueMode,
            };
        }
        return ExecutionMode::DocMode;
    }

    // Multi-action chain: check each action
    for (i, action_opt) in actions.iter().enumerate() {
        let Some(action) = action_opt else {
            continue;
        };

        let kind = ActionKind::from_action(action);
        let is_last = i == actions.len() - 1;

        match kind {
            ActionKind::Mutation => {
                // Mutations are fine in DocMode
                continue;
            }
            ActionKind::ReadOnly => {
                if !is_last {
                    // Read-only in middle of chain: the result becomes input for next action.
                    // get-value returns a Value which could be anything, so subsequent actions
                    // need to work on that Value.
                    // However, if the next action is also read-only or mutation on the same doc,
                    // it's ambiguous. Let's be conservative: non-final read-only forces ValueMode.
                    return ExecutionMode::ValueMode;
                }
                // Final read-only: can use zero-copy from Document after mutations
            }
            ActionKind::Derived => {
                // Derived actions produce a different structure (e.g., sequence of keys).
                // Even if last, the output needs Value-based serialization.
                // If not last, next action operates on the derived Value.
                return ExecutionMode::ValueMode;
            }
            ActionKind::Complex => {
                // Apply requires full Value processing
                return ExecutionMode::ValueMode;
            }
        }
    }

    ExecutionMode::DocMode
}

/// Check if an action is read-only (can use zero-copy from Document).
pub fn is_readonly(action: &Actions) -> bool {
    matches!(ActionKind::from_action(action), ActionKind::ReadOnly)
}

/// Check if an action is derived (iteration: keys, values, etc.).
pub fn is_derived(action: &Actions) -> bool {
    matches!(ActionKind::from_action(action), ActionKind::Derived)
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn set_value() -> Option<Actions> {
        Some(Actions::SetValue {
            key: "a".to_string(),
            value: "1".to_string(),
            yaml: false,
        })
    }

    fn del() -> Option<Actions> {
        Some(Actions::Del {
            key: "a".to_string(),
        })
    }

    fn get_value() -> Option<Actions> {
        Some(Actions::GetValue {
            path: Some("a".to_string()),
            default: None,
            yaml: false,
            line_buffer: false,
        })
    }

    fn get_type() -> Option<Actions> {
        Some(Actions::GetType {
            path: Some("a".to_string()),
        })
    }

    fn keys() -> Option<Actions> {
        Some(Actions::Keys {
            path: Some("a".to_string()),
            yaml: false,
        })
    }

    fn apply() -> Option<Actions> {
        Some(Actions::Apply {
            merge_policy: None,
            overlays: vec!["overlay.yaml".to_string()],
        })
    }

    #[test]
    fn test_single_mutation_is_doc_mode() {
        assert_eq!(analyze_chain(&[set_value()]), ExecutionMode::DocMode);
        assert_eq!(analyze_chain(&[del()]), ExecutionMode::DocMode);
    }

    #[test]
    fn test_single_readonly_is_doc_mode() {
        assert_eq!(analyze_chain(&[get_value()]), ExecutionMode::DocMode);
        assert_eq!(analyze_chain(&[get_type()]), ExecutionMode::DocMode);
    }

    #[test]
    fn test_single_derived_is_doc_mode() {
        assert_eq!(analyze_chain(&[keys()]), ExecutionMode::DocMode);
    }

    #[test]
    fn test_single_complex_is_value_mode() {
        assert_eq!(analyze_chain(&[apply()]), ExecutionMode::ValueMode);
    }

    #[test]
    fn test_pure_mutation_chain_is_doc_mode() {
        assert_eq!(
            analyze_chain(&[set_value(), set_value(), del()]),
            ExecutionMode::DocMode
        );
    }

    #[test]
    fn test_mutations_with_final_readonly_is_doc_mode() {
        assert_eq!(
            analyze_chain(&[set_value(), del(), get_value()]),
            ExecutionMode::DocMode
        );
    }

    #[test]
    fn test_readonly_not_at_end_is_value_mode() {
        // get-value in middle means next action operates on that value
        assert_eq!(
            analyze_chain(&[get_value(), set_value()]),
            ExecutionMode::ValueMode
        );
    }

    #[test]
    fn test_derived_at_end_is_value_mode() {
        // keys produces a sequence, which needs Value-based output
        assert_eq!(
            analyze_chain(&[set_value(), keys()]),
            ExecutionMode::ValueMode
        );
    }

    #[test]
    fn test_complex_anywhere_is_value_mode() {
        assert_eq!(
            analyze_chain(&[set_value(), apply()]),
            ExecutionMode::ValueMode
        );
    }

    #[test]
    fn test_empty_chain_is_doc_mode() {
        assert_eq!(analyze_chain(&[]), ExecutionMode::DocMode);
    }

    #[test]
    fn test_is_readonly() {
        assert!(is_readonly(&Actions::GetValue {
            path: None,
            default: None,
            yaml: false,
            line_buffer: false,
        }));
        assert!(is_readonly(&Actions::GetType { path: None }));
        assert!(is_readonly(&Actions::GetLength { path: None }));
        assert!(!is_readonly(&Actions::SetValue {
            key: "a".to_string(),
            value: "1".to_string(),
            yaml: false,
        }));
    }
}
