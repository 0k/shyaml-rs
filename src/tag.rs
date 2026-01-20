//! Tag parsing module for YAML merge directives.
//!
//! This module implements parsing of extended local tags as specified in
//! `tag-syntax.md`, with specific support for the `merge:` namespace used
//! in merge directives.
//!
//! # Tag Syntax
//!
//! Tags follow this grammar:
//! - Simple: `!tagname`
//! - Namespaced: `!namespace:operation`
//! - Compound: `!tag1;tag2` (semicolon concatenation)
//! - Parameterized: `!tag(args)` (for future use)
//!
//! # Merge Directives
//!
//! The `merge:` namespace supports these operations:
//! - `!merge:replace` - Replace parent value entirely
//! - `!merge:append` - Append to sequence (default for sequences)
//! - `!merge:prepend` - Prepend to sequence

use std::fmt;

/// Merge operation extracted from a tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOp {
    /// Replace parent value entirely
    Replace,
    /// Append child items after parent items (sequences only)
    Append,
    /// Prepend child items before parent items (sequences only)
    Prepend,
}

impl fmt::Display for MergeOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MergeOp::Replace => write!(f, "replace"),
            MergeOp::Append => write!(f, "append"),
            MergeOp::Prepend => write!(f, "prepend"),
        }
    }
}

/// Result of parsing a tag string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTag {
    /// Remaining tag after merge directive is removed.
    /// - `!literal;merge:replace` → `Some("!literal")`
    /// - `!a;b;merge:append` → `Some("!a;b")`
    /// - `!merge:replace` → `None`
    /// - `!custom` → `Some("!custom")`
    pub remaining: Option<String>,

    /// Extracted merge operation, if any.
    pub merge_op: Option<MergeOp>,
}

/// Errors that can occur during tag parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagError {
    /// Unknown merge operation (e.g., `!merge:foo`)
    UnknownOperation(String),
    /// Multiple merge directives in compound tag (e.g., `!merge:replace;merge:append`)
    MultipleMergeDirectives,
    /// Unexpected arguments on operation (e.g., `!merge:replace(x)`)
    UnexpectedArguments(String),
    /// Empty tag
    EmptyTag,
}

impl fmt::Display for TagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TagError::UnknownOperation(op) => {
                write!(
                    f,
                    "unknown merge operation '{}': expected replace, append, or prepend",
                    op
                )
            }
            TagError::MultipleMergeDirectives => {
                write!(
                    f,
                    "multiple merge directives in tag: only one merge directive allowed per node"
                )
            }
            TagError::UnexpectedArguments(op) => {
                write!(f, "unexpected arguments for merge operation '{}': current operations do not accept arguments", op)
            }
            TagError::EmptyTag => {
                write!(f, "empty tag")
            }
        }
    }
}

impl std::error::Error for TagError {}

/// Parse a tag string and extract any merge directive.
///
/// # Arguments
///
/// * `tag` - The tag string (with or without leading `!`)
///
/// # Returns
///
/// A `ParsedTag` containing:
/// - `remaining`: Other tags after merge directive removed (if any)
/// - `merge_op`: The merge operation (if a merge directive was found)
///
/// # Examples
///
/// ```
/// use shyaml_rs::tag::{parse_tag, MergeOp};
///
/// // Simple merge directive
/// let result = parse_tag("!merge:replace").unwrap();
/// assert_eq!(result.merge_op, Some(MergeOp::Replace));
/// assert_eq!(result.remaining, None);
///
/// // Compound tag with merge directive
/// let result = parse_tag("!literal;merge:append").unwrap();
/// assert_eq!(result.merge_op, Some(MergeOp::Append));
/// assert_eq!(result.remaining, Some("!literal".to_string()));
///
/// // No merge directive
/// let result = parse_tag("!custom").unwrap();
/// assert_eq!(result.merge_op, None);
/// assert_eq!(result.remaining, Some("!custom".to_string()));
/// ```
pub fn parse_tag(tag: &str) -> Result<ParsedTag, TagError> {
    let tag = tag.trim();
    if tag.is_empty() {
        return Err(TagError::EmptyTag);
    }

    // Remove leading '!' if present for parsing
    let tag_content = tag.strip_prefix('!').unwrap_or(tag);
    if tag_content.is_empty() {
        return Err(TagError::EmptyTag);
    }

    // Split on ';' outside parentheses to get tag parts
    let parts = split_tag_parts(tag_content);

    let mut merge_op: Option<MergeOp> = None;
    let mut remaining_parts: Vec<&str> = Vec::new();

    for part in &parts {
        if let Some(op) = parse_merge_part(part)? {
            if merge_op.is_some() {
                return Err(TagError::MultipleMergeDirectives);
            }
            merge_op = Some(op);
        } else {
            remaining_parts.push(part);
        }
    }

    // Reconstruct remaining tag
    let remaining = if remaining_parts.is_empty() {
        None
    } else {
        Some(format!("!{}", remaining_parts.join(";")))
    };

    Ok(ParsedTag {
        remaining,
        merge_op,
    })
}

/// Split tag content on ';' but respect parentheses.
fn split_tag_parts(content: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut paren_depth: u32 = 0;

    for (i, c) in content.char_indices() {
        match c {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            ';' if paren_depth == 0 => {
                if start < i {
                    parts.push(&content[start..i]);
                }
                start = i + 1;
            }
            _ => {}
        }
    }

    // Don't forget the last part
    if start < content.len() {
        parts.push(&content[start..]);
    }

    parts
}

/// Parse a single tag part to check if it's a merge directive.
/// Returns `Ok(Some(MergeOp))` if it's a merge directive,
/// `Ok(None)` if it's not, or `Err` if it's an invalid merge directive.
fn parse_merge_part(part: &str) -> Result<Option<MergeOp>, TagError> {
    // Check if this part starts with "merge:"
    let merge_content = match part.strip_prefix("merge:") {
        Some(content) => content,
        None => return Ok(None), // Not a merge directive
    };

    // Check for arguments (parentheses)
    if let Some(paren_pos) = merge_content.find('(') {
        let op_name = &merge_content[..paren_pos];
        return Err(TagError::UnexpectedArguments(op_name.to_string()));
    }

    // Parse the operation
    match merge_content {
        "replace" => Ok(Some(MergeOp::Replace)),
        "append" => Ok(Some(MergeOp::Append)),
        "prepend" => Ok(Some(MergeOp::Prepend)),
        other => Err(TagError::UnknownOperation(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==========================================================================
    // Simple merge directives
    // ==========================================================================

    #[test]
    fn test_parse_merge_replace() {
        let result = parse_tag("!merge:replace").unwrap();
        assert_eq!(result.merge_op, Some(MergeOp::Replace));
        assert_eq!(result.remaining, None);
    }

    #[test]
    fn test_parse_merge_append() {
        let result = parse_tag("!merge:append").unwrap();
        assert_eq!(result.merge_op, Some(MergeOp::Append));
        assert_eq!(result.remaining, None);
    }

    #[test]
    fn test_parse_merge_prepend() {
        let result = parse_tag("!merge:prepend").unwrap();
        assert_eq!(result.merge_op, Some(MergeOp::Prepend));
        assert_eq!(result.remaining, None);
    }

    // ==========================================================================
    // Tags without merge directive
    // ==========================================================================

    #[test]
    fn test_parse_simple_tag() {
        let result = parse_tag("!custom").unwrap();
        assert_eq!(result.merge_op, None);
        assert_eq!(result.remaining, Some("!custom".to_string()));
    }

    #[test]
    fn test_parse_namespaced_tag() {
        let result = parse_tag("!type:string").unwrap();
        assert_eq!(result.merge_op, None);
        assert_eq!(result.remaining, Some("!type:string".to_string()));
    }

    // ==========================================================================
    // Compound tags with merge directive
    // ==========================================================================

    #[test]
    fn test_parse_compound_merge_last() {
        let result = parse_tag("!literal;merge:replace").unwrap();
        assert_eq!(result.merge_op, Some(MergeOp::Replace));
        assert_eq!(result.remaining, Some("!literal".to_string()));
    }

    #[test]
    fn test_parse_compound_merge_first() {
        // Convention is merge last, but we support it anywhere
        let result = parse_tag("!merge:append;literal").unwrap();
        assert_eq!(result.merge_op, Some(MergeOp::Append));
        assert_eq!(result.remaining, Some("!literal".to_string()));
    }

    #[test]
    fn test_parse_compound_merge_middle() {
        let result = parse_tag("!a;merge:prepend;b").unwrap();
        assert_eq!(result.merge_op, Some(MergeOp::Prepend));
        assert_eq!(result.remaining, Some("!a;b".to_string()));
    }

    #[test]
    fn test_parse_compound_multiple_tags_no_merge() {
        let result = parse_tag("!a;b;c").unwrap();
        assert_eq!(result.merge_op, None);
        assert_eq!(result.remaining, Some("!a;b;c".to_string()));
    }

    #[test]
    fn test_parse_compound_with_namespaced() {
        let result = parse_tag("!custom;type:int;merge:replace").unwrap();
        assert_eq!(result.merge_op, Some(MergeOp::Replace));
        assert_eq!(result.remaining, Some("!custom;type:int".to_string()));
    }

    // ==========================================================================
    // Tags with arguments (parentheses)
    // ==========================================================================

    #[test]
    fn test_parse_tag_with_args_no_merge() {
        let result = parse_tag("!custom(arg1;arg2)").unwrap();
        assert_eq!(result.merge_op, None);
        assert_eq!(result.remaining, Some("!custom(arg1;arg2)".to_string()));
    }

    #[test]
    fn test_parse_compound_with_args_and_merge() {
        let result = parse_tag("!custom(arg);merge:replace").unwrap();
        assert_eq!(result.merge_op, Some(MergeOp::Replace));
        assert_eq!(result.remaining, Some("!custom(arg)".to_string()));
    }

    #[test]
    fn test_parse_semicolon_in_args_not_split() {
        // Semicolon inside parentheses should not split tags
        let result = parse_tag("!custom(a;b;c);merge:append").unwrap();
        assert_eq!(result.merge_op, Some(MergeOp::Append));
        assert_eq!(result.remaining, Some("!custom(a;b;c)".to_string()));
    }

    // ==========================================================================
    // Error cases
    // ==========================================================================

    #[test]
    fn test_error_unknown_operation() {
        let result = parse_tag("!merge:unknown");
        assert!(matches!(result, Err(TagError::UnknownOperation(op)) if op == "unknown"));
    }

    #[test]
    fn test_error_multiple_merge_directives() {
        let result = parse_tag("!merge:replace;merge:append");
        assert!(matches!(result, Err(TagError::MultipleMergeDirectives)));
    }

    #[test]
    fn test_error_unexpected_arguments() {
        let result = parse_tag("!merge:replace(foo)");
        assert!(matches!(result, Err(TagError::UnexpectedArguments(op)) if op == "replace"));
    }

    #[test]
    fn test_error_empty_tag() {
        let result = parse_tag("");
        assert!(matches!(result, Err(TagError::EmptyTag)));

        let result = parse_tag("!");
        assert!(matches!(result, Err(TagError::EmptyTag)));
    }

    // ==========================================================================
    // Edge cases
    // ==========================================================================

    #[test]
    fn test_tag_without_leading_bang() {
        // Should work without leading '!'
        let result = parse_tag("merge:replace").unwrap();
        assert_eq!(result.merge_op, Some(MergeOp::Replace));
        assert_eq!(result.remaining, None);
    }

    #[test]
    fn test_whitespace_trimmed() {
        let result = parse_tag("  !merge:replace  ").unwrap();
        assert_eq!(result.merge_op, Some(MergeOp::Replace));
        assert_eq!(result.remaining, None);
    }

    #[test]
    fn test_merge_op_display() {
        assert_eq!(format!("{}", MergeOp::Replace), "replace");
        assert_eq!(format!("{}", MergeOp::Append), "append");
        assert_eq!(format!("{}", MergeOp::Prepend), "prepend");
    }

    #[test]
    fn test_tag_error_display() {
        let err = TagError::UnknownOperation("foo".to_string());
        assert!(err.to_string().contains("foo"));
        assert!(err.to_string().contains("unknown"));

        let err = TagError::MultipleMergeDirectives;
        assert!(err.to_string().contains("multiple"));

        let err = TagError::UnexpectedArguments("replace".to_string());
        assert!(err.to_string().contains("replace"));
        assert!(err.to_string().contains("arguments"));
    }
}
