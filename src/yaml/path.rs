//! Path handling for YAML navigation.
//!
//! Provides utilities for parsing dot-notation paths and resolving indices.

use super::error::Error;

/// Split a dot-notation path into its components.
///
/// Handles escape sequences: `\.` for literal dots, `\\` for literal backslashes.
/// For example, `a.b\.c.d` becomes `["a", "b.c", "d"]`.
pub fn split_path(path: &str) -> Vec<String> {
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

/// Resolve a string index to an actual index in a sequence.
///
/// Handles:
/// - Positive indices (0, 1, 2, ...)
/// - Negative indices (-1 = last, -2 = second to last, ...)
///
/// # Errors
///
/// Returns an error if:
/// - The index is not a valid integer
/// - The index is out of range for the sequence length
pub fn resolve_index(part: &str, len: usize, full_path: &str) -> Result<usize, Error> {
    let idx: i64 = part.parse().map_err(|_| {
        Error::Path(format!(
            "invalid path '{}', non-integer index '{}' provided on a sequence.",
            full_path, part
        ))
    })?;

    let resolved = if idx < 0 {
        let abs_idx = (-idx) as usize;
        if abs_idx > len {
            return Err(Error::Path(format!(
                "invalid path '{}', index {} is out of range ({} elements in sequence).",
                full_path, idx, len
            )));
        }
        len - abs_idx
    } else {
        idx as usize
    };

    if resolved >= len {
        return Err(Error::Path(format!(
            "invalid path '{}', index {} is out of range ({} elements in sequence).",
            full_path, idx, len
        )));
    }

    Ok(resolved)
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // split_path() tests
    // =========================================================================

    #[test]
    fn test_split_path_simple() {
        assert_eq!(split_path("a.b.c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_split_path_single_element() {
        assert_eq!(split_path("foo"), vec!["foo"]);
    }

    #[test]
    fn test_split_path_escaped_dot() {
        // \. produces a literal dot in the key
        assert_eq!(split_path(r"a\.b.c"), vec!["a.b", "c"]);
    }

    #[test]
    fn test_split_path_escaped_backslash() {
        // \\ produces a literal backslash
        assert_eq!(split_path(r"a\\b.c"), vec!["a\\b", "c"]);
    }

    #[test]
    fn test_split_path_escaped_backslash_then_dot() {
        // \\. = literal backslash followed by separator dot
        assert_eq!(split_path(r"a\\.b"), vec!["a\\", "b"]);
    }

    #[test]
    fn test_split_path_escaped_backslash_and_escaped_dot() {
        // \\\. = literal backslash + literal dot (no separator)
        assert_eq!(split_path(r"a\\\.b.c"), vec!["a\\.b", "c"]);
    }

    #[test]
    fn test_split_path_empty_string() {
        // Empty string is a single empty element (valid key in YAML)
        assert_eq!(split_path(""), vec![""]);
    }

    #[test]
    fn test_split_path_empty_key_middle() {
        // a..b means keys: "a", "", "b" (middle key is empty string)
        assert_eq!(split_path("a..b"), vec!["a", "", "b"]);
    }

    #[test]
    fn test_split_path_empty_key_start() {
        // .a means keys: "", "a"
        assert_eq!(split_path(".a"), vec!["", "a"]);
    }

    #[test]
    fn test_split_path_empty_key_end() {
        // a. means keys: "a", ""
        assert_eq!(split_path("a."), vec!["a", ""]);
    }

    #[test]
    fn test_split_path_only_dots() {
        // .. means three empty keys
        assert_eq!(split_path(".."), vec!["", "", ""]);
    }

    #[test]
    fn test_split_path_trailing_backslash() {
        // Trailing backslash with nothing to escape - currently just drops it
        // (This is edge case behavior, documenting current implementation)
        assert_eq!(split_path(r"a\"), vec!["a"]);
    }

    // =========================================================================
    // resolve_index() tests
    // =========================================================================

    #[test]
    fn test_resolve_index_positive() {
        assert_eq!(resolve_index("0", 3, "test").unwrap(), 0);
        assert_eq!(resolve_index("1", 3, "test").unwrap(), 1);
        assert_eq!(resolve_index("2", 3, "test").unwrap(), 2);
    }

    #[test]
    fn test_resolve_index_negative() {
        // -1 = last element
        assert_eq!(resolve_index("-1", 3, "test").unwrap(), 2);
        // -2 = second to last
        assert_eq!(resolve_index("-2", 3, "test").unwrap(), 1);
        // -3 = first element (for len=3)
        assert_eq!(resolve_index("-3", 3, "test").unwrap(), 0);
    }

    #[test]
    fn test_resolve_index_positive_out_of_range() {
        let err = resolve_index("3", 3, "items.3").unwrap_err();
        match err {
            Error::Path(msg) => {
                assert!(msg.contains("index 3 is out of range"));
                assert!(msg.contains("3 elements"));
            }
            _ => panic!("Expected Error::Path"),
        }
    }

    #[test]
    fn test_resolve_index_negative_out_of_range() {
        let err = resolve_index("-4", 3, "items.-4").unwrap_err();
        match err {
            Error::Path(msg) => {
                assert!(msg.contains("index -4 is out of range"));
                assert!(msg.contains("3 elements"));
            }
            _ => panic!("Expected Error::Path"),
        }
    }

    #[test]
    fn test_resolve_index_non_integer() {
        let err = resolve_index("foo", 3, "items.foo").unwrap_err();
        match err {
            Error::Path(msg) => {
                assert!(msg.contains("non-integer index 'foo'"));
            }
            _ => panic!("Expected Error::Path"),
        }
    }

    #[test]
    fn test_resolve_index_empty_string() {
        let err = resolve_index("", 3, "items.").unwrap_err();
        match err {
            Error::Path(msg) => {
                assert!(msg.contains("non-integer index ''"));
            }
            _ => panic!("Expected Error::Path"),
        }
    }

    #[test]
    fn test_resolve_index_empty_sequence() {
        // Even index 0 is out of range for empty sequence
        let err = resolve_index("0", 0, "empty.0").unwrap_err();
        match err {
            Error::Path(msg) => {
                assert!(msg.contains("index 0 is out of range"));
                assert!(msg.contains("0 elements"));
            }
            _ => panic!("Expected Error::Path"),
        }
    }

    #[test]
    fn test_resolve_index_negative_on_empty_sequence() {
        let err = resolve_index("-1", 0, "empty.-1").unwrap_err();
        match err {
            Error::Path(msg) => {
                assert!(msg.contains("index -1 is out of range"));
            }
            _ => panic!("Expected Error::Path"),
        }
    }

    #[test]
    fn test_resolve_index_boundary_negative() {
        // -len is exactly the first element
        assert_eq!(resolve_index("-5", 5, "test").unwrap(), 0);
        // -(len+1) is out of range
        let err = resolve_index("-6", 5, "test.-6").unwrap_err();
        match err {
            Error::Path(msg) => {
                assert!(msg.contains("index -6 is out of range"));
            }
            _ => panic!("Expected Error::Path"),
        }
    }

    #[test]
    fn test_resolve_index_path_in_error_message() {
        // Verify the full path appears in error messages
        let err = resolve_index("99", 3, "deeply.nested.path.99").unwrap_err();
        match err {
            Error::Path(msg) => {
                assert!(msg.contains("deeply.nested.path.99"));
            }
            _ => panic!("Expected Error::Path"),
        }
    }
}
