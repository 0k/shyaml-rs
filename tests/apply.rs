//! Integration tests for the `apply` action
//!
//! The `apply` action merges YAML documents:
//! - Base document from stdin
//! - Overlay document(s) from file argument(s)
//! - Result to stdout

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
                eprintln!("\x1b[31m{}\x1b[0m", line); // Red for expected (missing)
            } else if line.starts_with('+') {
                eprintln!("\x1b[32m{}\x1b[0m", line); // Green for actual (extra)
            } else if line.starts_with('@') {
                eprintln!("\x1b[36m{}\x1b[0m", line); // Cyan for context markers
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
// Basic merge tests
// =============================================================================

#[test]
fn test_apply_scalar_replacement() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        name: alice
        count: 10
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            name: bob
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            name: bob
            count: 10
        "},
    );
}

#[test]
fn test_apply_deep_mapping_merge() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        database:
          host: localhost
          port: 5432
          options:
            timeout: 30
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            database:
              port: 3306
              options:
                charset: utf8
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            database:
              host: localhost
              port: 3306
              options:
                timeout: 30
                charset: utf8
        "},
    );
}

#[test]
fn test_apply_sequence_append() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        paths:
          - /var/log
          - /var/cache
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            paths:
              - /var/data
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            paths:
            - /var/log
            - /var/cache
            - /var/data
        "},
    );
}

// =============================================================================
// Multiple overlays
// =============================================================================

#[test]
fn test_apply_multiple_overlays() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        name: base
        level: 1
    "};

    let overlay1 = temp_yaml_file(
        &tmp,
        "overlay1.yaml",
        indoc! {"
            name: first
            extra: added
        "},
    );

    let overlay2 = temp_yaml_file(
        &tmp,
        "overlay2.yaml",
        indoc! {"
            name: second
            level: 2
        "},
    );

    let (stdout, stderr, success) = run_shyaml(
        &[
            "apply",
            overlay1.to_str().unwrap(),
            overlay2.to_str().unwrap(),
        ],
        base,
    );

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            name: second
            level: 2
            extra: added
        "},
    );
}

// =============================================================================
// Error cases
// =============================================================================

#[test]
fn test_apply_type_mismatch_error() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        config: simple-value
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            config:
              key: value
        "},
    );

    let (_stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    // Should fail due to type mismatch (scalar vs mapping)
    assert!(!success, "Expected failure for type mismatch");
    assert!(
        stderr.contains("type") || stderr.contains("mismatch") || stderr.contains("config"),
        "Expected type mismatch error message: {}",
        stderr
    );
}

#[test]
fn test_apply_missing_overlay_file() {
    let (_stdout, stderr, success) =
        run_shyaml(&["apply", "/nonexistent/overlay.yaml"], "key: value\n");

    assert!(!success, "Expected failure for missing file");
    assert!(
        stderr.contains("nonexistent")
            || stderr.contains("not found")
            || stderr.contains("No such file"),
        "Expected file not found error: {}",
        stderr
    );
}

#[test]
fn test_apply_no_overlay_argument() {
    let (_stdout, _stderr, success) = run_shyaml(&["apply"], "key: value\n");

    // Should fail - at least one overlay file required
    assert!(!success, "Expected failure when no overlay specified");
}

// =============================================================================
// Legacy sequence behavior - deduplication
// =============================================================================

#[test]
fn test_apply_sequence_deduplication() {
    // Legacy behavior: duplicates are moved to end position
    // Parent: [a, b, c], Child: [b, d] -> [a, c, b, d]
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
            items:
              - b
              - d
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            items:
            - a
            - c
            - b
            - d
        "},
    );
}

#[test]
fn test_apply_sequence_duplicate_in_child() {
    // When child has duplicates, only last occurrence is kept
    // Parent: [a, b], Child: [c, b, c] -> [a, b, c]
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
            items:
              - c
              - b
              - c
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            items:
            - a
            - b
            - c
        "},
    );
}

// =============================================================================
// Legacy null handling - key deletion
// =============================================================================

#[test]
fn test_apply_null_deletes_key() {
    // Legacy behavior: setting a key to null removes it from result
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        keep: 1
        remove: 2
        nested:
          a: 1
          b: 2
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            remove: null
            nested:
              b: null
        "},
    );

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            keep: 1
            nested:
              a: 1
        "},
    );
}

// =============================================================================
// Edge cases
// =============================================================================

#[test]
fn test_apply_empty_overlay() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        existing: value
    "};

    let overlay = temp_yaml_file(&tmp, "empty.yaml", "");

    let (stdout, stderr, success) = run_shyaml(&["apply", overlay.to_str().unwrap()], base);

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            existing: value
        "},
    );
}

// =============================================================================
// Merge policies
// =============================================================================

#[test]
fn test_apply_policy_replace() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        config:
          a: 1
          b: 2
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            config:
              b: 3
              c: 4
        "},
    );

    let (stdout, stderr, success) = run_shyaml(
        &["apply", "-m", "config=replace", overlay.to_str().unwrap()],
        base,
    );

    assert!(success, "Command failed: {}", stderr);
    // With replace policy, base config is completely replaced
    assert_output_eq(
        &stdout,
        indoc! {"
            config:
              b: 3
              c: 4
        "},
    );
}

#[test]
fn test_apply_policy_prepend() {
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
            items:
              - c
              - d
        "},
    );

    let (stdout, stderr, success) = run_shyaml(
        &["apply", "-m", "items=prepend", overlay.to_str().unwrap()],
        base,
    );

    assert!(success, "Command failed: {}", stderr);
    // With prepend policy, overlay items come first
    assert_output_eq(
        &stdout,
        indoc! {"
            items:
            - c
            - d
            - a
            - b
        "},
    );
}

#[test]
fn test_apply_policy_multiple_comma_separated() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        config:
          a: 1
        items:
          - x
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            config:
              b: 2
            items:
              - y
        "},
    );

    let (stdout, stderr, success) = run_shyaml(
        &[
            "apply",
            "-m",
            "config=replace,items=prepend",
            overlay.to_str().unwrap(),
        ],
        base,
    );

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            config:
              b: 2
            items:
            - y
            - x
        "},
    );
}

#[test]
fn test_apply_policy_multiple_flags() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        config:
          a: 1
        items:
          - x
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            config:
              b: 2
            items:
              - y
        "},
    );

    let (stdout, stderr, success) = run_shyaml(
        &[
            "apply",
            "-m",
            "config=replace",
            "-m",
            "items=prepend",
            overlay.to_str().unwrap(),
        ],
        base,
    );

    assert!(success, "Command failed: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            config:
              b: 2
            items:
            - y
            - x
        "},
    );
}

#[test]
fn test_apply_policy_nested_path() {
    let tmp = TempDir::new().unwrap();

    let base = indoc! {"
        database:
          config:
            host: localhost
            port: 5432
    "};

    let overlay = temp_yaml_file(
        &tmp,
        "overlay.yaml",
        indoc! {"
            database:
              config:
                port: 3306
                user: admin
        "},
    );

    let (stdout, stderr, success) = run_shyaml(
        &[
            "apply",
            "-m",
            "database.config=replace",
            overlay.to_str().unwrap(),
        ],
        base,
    );

    assert!(success, "Command failed: {}", stderr);
    // Only database.config is replaced, not merged
    assert_output_eq(
        &stdout,
        indoc! {"
            database:
              config:
                port: 3306
                user: admin
        "},
    );
}
