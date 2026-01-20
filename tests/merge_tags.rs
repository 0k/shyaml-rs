//! Integration tests for inline merge tags
//!
//! Tests the merge tag system:
//! - `!merge:replace` - replace parent value
//! - `!merge:append` - append to sequence (default)
//! - `!merge:prepend` - prepend to sequence
//! - Compound tags: `!custom;merge:replace`

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use indoc::indoc;
use similar::TextDiff;
use tempfile::TempDir;

/// Get path to the shyaml binary
fn binary_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target");
    path.push("debug");
    path.push("shyaml");
    path
}

/// Run shyaml with given args and stdin, return (stdout, stderr, success)
fn run_shyaml(args: &[&str], stdin_data: &str) -> (String, String, bool) {
    let binary = binary_path();

    let mut child = Command::new(&binary)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn shyaml");

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_data.as_bytes())
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to wait on child");

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// Assert that actual output equals expected, showing a colored diff on failure
fn assert_output_eq(actual: &str, expected: &str) {
    if actual != expected {
        let diff = TextDiff::from_lines(expected, actual);
        eprintln!();
        for line in diff
            .unified_diff()
            .header("expected", "actual")
            .to_string()
            .lines()
        {
            if line.starts_with('-') {
                eprintln!("\x1b[31m{}\x1b[0m", line);
            } else if line.starts_with('+') {
                eprintln!("\x1b[32m{}\x1b[0m", line);
            } else if line.starts_with('@') {
                eprintln!("\x1b[36m{}\x1b[0m", line);
            } else {
                eprintln!("{}", line);
            }
        }
        panic!("Output mismatch - see diff above");
    }
}

/// Create a temporary file with given content, return its path
fn temp_yaml_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, content).expect("Failed to write temp file");
    path
}

// =============================================================================
// !merge:replace tests
// =============================================================================

#[test]
fn test_merge_replace_on_mapping() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        config:
          host: localhost
          port: 5432
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            config: !merge:replace
              port: 3306
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    // With !merge:replace, the entire config is replaced (host is gone)
    assert_output_eq(
        &stdout,
        indoc! {"
            config:
              port: 3306
        "},
    );
}

#[test]
fn test_merge_replace_on_sequence() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        items:
          - a
          - b
          - c
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            items: !merge:replace
              - x
              - y
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            items:
            - x
            - y
        "},
    );
}

#[test]
fn test_merge_replace_on_scalar() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        name: alice
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            name: !merge:replace bob
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            name: bob
        "},
    );
}

// =============================================================================
// !merge:prepend tests
// =============================================================================

#[test]
fn test_merge_prepend_on_sequence() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        items:
          - a
          - b
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            items: !merge:prepend
              - x
              - y
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            items:
            - x
            - y
            - a
            - b
        "},
    );
}

// =============================================================================
// !merge:append tests (explicit, same as default for sequences)
// =============================================================================

#[test]
fn test_merge_append_explicit() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        items:
          - a
          - b
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            items: !merge:append
              - c
              - d
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    // Append is the default, so this behaves same as no tag
    assert_output_eq(
        &stdout,
        indoc! {"
            items:
            - a
            - b
            - c
            - d
        "},
    );
}

// =============================================================================
// Compound tags tests
// =============================================================================

#[test]
fn test_compound_tag_merge_replace() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        data: old
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            data: !custom;merge:replace new
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    // The !custom tag should be preserved, !merge:replace stripped
    assert_output_eq(
        &stdout,
        indoc! {"
            data: !custom new
        "},
    );
}

#[test]
fn test_compound_tag_merge_prepend() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        items:
          - a
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            items: !mylist;merge:prepend
              - b
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    // The !mylist tag should be preserved on the result
    assert_output_eq(
        &stdout,
        indoc! {"
            items: !mylist
            - b
            - a
        "},
    );
}

// =============================================================================
// Tag stripping tests
// =============================================================================

#[test]
fn test_merge_tag_stripped_from_output() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        value: old
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            value: !merge:replace new
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    // No tag should remain (merge:replace is stripped)
    assert_output_eq(
        &stdout,
        indoc! {"
            value: new
        "},
    );
}

#[test]
fn test_parent_tags_preserved() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        secret: !encrypted abc123
        plain: value
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            plain: updated
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    // Parent's !encrypted tag should be preserved
    assert_output_eq(
        &stdout,
        indoc! {"
            secret: !encrypted abc123
            plain: updated
        "},
    );
}

// =============================================================================
// CLI policy overrides inline tag
// =============================================================================

#[test]
fn test_cli_policy_overrides_inline_tag() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        items:
          - a
          - b
    "};

    // Overlay uses !merge:prepend but CLI will override to replace
    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            items: !merge:prepend
              - x
        "},
    );

    let (stdout, stderr, success) = run_shyaml(
        &["apply", "-m", "items=replace", overlay.to_str().unwrap()],
        base,
    );

    assert!(success, "Command failed: {}", stderr);
    // CLI replace wins over inline prepend
    assert_output_eq(
        &stdout,
        indoc! {"
            items:
            - x
        "},
    );
}

// =============================================================================
// New key with merge tag (tag stripped, value used as-is)
// =============================================================================

#[test]
fn test_new_key_with_merge_tag() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        existing: value
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            new_key: !merge:append
              - item
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    // Tag stripped, value used as-is
    assert_output_eq(
        &stdout,
        indoc! {"
            existing: value
            new_key:
            - item
        "},
    );
}

#[test]
fn test_new_key_with_compound_tag() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        existing: value
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            new_key: !custom;merge:prepend
              - item
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    // merge:prepend stripped, !custom preserved
    assert_output_eq(
        &stdout,
        indoc! {"
            existing: value
            new_key: !custom
            - item
        "},
    );
}

#[test]
fn test_error_append_on_mapping() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        config:
          host: localhost
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            config: !merge:append
              port: 3306
        "},
    );

    let (_, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(!success, "Expected command to fail");
    assert!(
        stderr.contains("!merge:append can only be used on sequences"),
        "Expected error about append on non-sequence, got: {}",
        stderr
    );
    assert!(
        stderr.contains("at 'config'"),
        "Expected path in error message, got: {}",
        stderr
    );
}

#[test]
fn test_error_prepend_on_scalar() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        name: alice
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            name: !merge:prepend bob
        "},
    );

    let (_, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(!success, "Expected command to fail");
    assert!(
        stderr.contains("!merge:prepend can only be used on sequences"),
        "Expected error about prepend on non-sequence, got: {}",
        stderr
    );
    assert!(
        stderr.contains("got string"),
        "Expected type in error message, got: {}",
        stderr
    );
}

#[test]
fn test_error_append_at_root() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        key: value
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            !merge:append
            other: value
        "},
    );

    let (_, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(!success, "Expected command to fail");
    assert!(
        stderr.contains("!merge:append can only be used on sequences"),
        "Expected error about append on non-sequence, got: {}",
        stderr
    );
    assert!(
        stderr.contains("at root"),
        "Expected 'at root' in error message, got: {}",
        stderr
    );
}

#[test]
fn test_error_prepend_on_nested_mapping() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        outer:
          inner:
            value: 1
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            outer:
              inner: !merge:prepend
                value: 2
        "},
    );

    let (_, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(!success, "Expected command to fail");
    assert!(
        stderr.contains("!merge:prepend can only be used on sequences"),
        "Expected error about prepend on non-sequence, got: {}",
        stderr
    );
    assert!(
        stderr.contains("at 'outer.inner'"),
        "Expected nested path in error message, got: {}",
        stderr
    );
}

#[test]
fn test_cli_policy_bypasses_type_validation() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        config:
          host: localhost
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            config:
              port: 3306
        "},
    );

    let (stdout, stderr, success) = run_shyaml(
        &["apply", "-m", "config=prepend", overlay.to_str().unwrap()],
        base,
    );

    assert!(
        success,
        "CLI prepend policy on mapping should not error: {}",
        stderr
    );
    assert_output_eq(
        &stdout,
        indoc! {"
            config:
              port: 3306
        "},
    );
}
