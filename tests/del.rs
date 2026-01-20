use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use indoc::indoc;
use similar::TextDiff;

fn binary_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target");
    path.push("debug");
    path.push("shyaml");
    path
}

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
        panic!("Output mismatch (see diff above)");
    }
}

#[test]
fn test_del_simple_key() {
    let base = indoc! {"
        a: 1
        b: 2
        c: 3
    "};

    let expected = indoc! {"
        a: 1
        c: 3
    "};

    let (stdout, stderr, success) = run_shyaml(&["del", "b"], base);
    assert!(success, "del failed: {}", stderr);
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_del_nested_key() {
    let base = indoc! {"
        config:
          db:
            host: localhost
            port: 5432
    "};

    let expected = indoc! {"
        config:
          db:
            host: localhost
    "};

    let (stdout, stderr, success) = run_shyaml(&["del", "config.db.port"], base);
    assert!(success, "del failed: {}", stderr);
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_del_sequence_element() {
    let base = indoc! {"
        items:
        - a
        - b
        - c
    "};

    let expected = indoc! {"
        items:
        - a
        - c
    "};

    let (stdout, stderr, success) = run_shyaml(&["del", "items.1"], base);
    assert!(success, "del failed: {}", stderr);
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_del_sequence_negative_index() {
    let base = indoc! {"
        items:
        - a
        - b
        - c
    "};

    let expected = indoc! {"
        items:
        - a
        - b
    "};

    let (stdout, stderr, success) = run_shyaml(&["del", "items.-1"], base);
    assert!(success, "del failed: {}", stderr);
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_del_missing_key_error() {
    let base = indoc! {"
        a: 1
    "};

    let (_, stderr, success) = run_shyaml(&["del", "nonexistent"], base);
    assert!(!success, "del should fail for missing key");
    assert!(stderr.contains("missing key 'nonexistent'"));
}

#[test]
fn test_del_empty_path_error() {
    let base = indoc! {"
        a: 1
    "};

    let (_, stderr, success) = run_shyaml(&["del", ""], base);
    assert!(!success, "del should fail for empty path");
    assert!(stderr.contains("Empty path"));
}

#[test]
fn test_del_index_out_of_range_error() {
    let base = indoc! {"
        items:
        - a
        - b
    "};

    let (_, stderr, success) = run_shyaml(&["del", "items.5"], base);
    assert!(!success, "del should fail for out of range index");
    assert!(stderr.contains("out of range"));
}

#[test]
fn test_del_non_integer_index_error() {
    let base = indoc! {"
        items:
        - a
        - b
    "};

    let (_, stderr, success) = run_shyaml(&["del", "items.foo"], base);
    assert!(!success, "del should fail for non-integer index");
    assert!(stderr.contains("non-integer index"));
}

#[test]
fn test_del_negative_index_out_of_range_error() {
    let base = indoc! {"
        items:
        - a
        - b
    "};

    let (_, stderr, success) = run_shyaml(&["del", "items.-5"], base);
    assert!(!success, "del should fail for negative index out of range");
    assert!(stderr.contains("out of range"));
    assert!(stderr.contains("-5"));
}

#[test]
fn test_del_preserves_order() {
    let base = indoc! {"
        z: 1
        a: 2
        m: 3
        b: 4
    "};

    let expected = indoc! {"
        z: 1
        m: 3
        b: 4
    "};

    let (stdout, stderr, success) = run_shyaml(&["del", "a"], base);
    assert!(success, "del failed: {}", stderr);
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_del_from_root_sequence() {
    let base = indoc! {"
        - first
        - second
        - third
    "};

    let expected = indoc! {"
        - first
        - third
    "};

    let (stdout, stderr, success) = run_shyaml(&["del", "1"], base);
    assert!(success, "del failed: {}", stderr);
    assert_output_eq(&stdout, expected);
}
