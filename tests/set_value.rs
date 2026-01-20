//! Integration tests for the `set-value` action

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
        panic!("Output mismatch - see diff above");
    }
}

#[test]
fn test_set_value_simple() {
    let (stdout, stderr, success) = run_shyaml(&["set-value", "name", "new"], "name: old\n");
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(&stdout, "name: new\n");
}

#[test]
fn test_set_value_nested_path() {
    let (stdout, stderr, success) =
        run_shyaml(&["set-value", "config.host", "localhost"], "config: {}\n");
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            config:
              host: localhost
        "},
    );
}

#[test]
fn test_set_value_creates_intermediate_mappings() {
    let (stdout, stderr, success) = run_shyaml(&["set-value", "a.b.c", "deep"], "\n");
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            a:
              b:
                c: deep
        "},
    );
}

#[test]
fn test_set_value_yaml_flag() {
    let (stdout, stderr, success) = run_shyaml(
        &["set-value", "data.items", "[1, 2, 3]", "-y"],
        "data: {}\n",
    );
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            data:
              items:
              - 1
              - 2
              - 3
        "},
    );
}

#[test]
fn test_set_value_yaml_flag_complex_structure() {
    let (stdout, stderr, success) = run_shyaml(
        &[
            "set-value",
            "config.db",
            "{host: localhost, port: 5432}",
            "-y",
        ],
        "config: {}\n",
    );
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            config:
              db:
                host: localhost
                port: 5432
        "},
    );
}

#[test]
fn test_set_value_without_yaml_flag_literal() {
    let (stdout, stderr, success) = run_shyaml(
        &["set-value", "config.data", "{host: localhost}"],
        "config: {}\n",
    );
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {r#"
            config:
              data: "{host: localhost}"
        "#},
    );
}

#[test]
fn test_set_value_sequence_index() {
    let (stdout, stderr, success) =
        run_shyaml(&["set-value", "items.1", "changed"], "items: [a, b, c]\n");
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            items:
            - a
            - changed
            - c
        "},
    );
}

#[test]
fn test_set_value_negative_index() {
    let (stdout, stderr, success) =
        run_shyaml(&["set-value", "items.-1", "last"], "items: [a, b, c]\n");
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            items:
            - a
            - b
            - last
        "},
    );
}

#[test]
fn test_set_value_negative_index_first() {
    let (stdout, stderr, success) =
        run_shyaml(&["set-value", "items.-3", "first"], "items: [a, b, c]\n");
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            items:
            - first
            - b
            - c
        "},
    );
}

#[test]
fn test_set_value_index_out_of_range() {
    let (stdout, stderr, success) =
        run_shyaml(&["set-value", "items.5", "x"], "items: [a, b, c]\n");
    assert!(!success);
    assert!(stdout.is_empty());
    assert!(stderr.contains("out of range"));
    assert!(stderr.contains("3 elements"));
}

#[test]
fn test_set_value_negative_index_out_of_range() {
    let (stdout, stderr, success) =
        run_shyaml(&["set-value", "items.-10", "x"], "items: [a, b, c]\n");
    assert!(!success);
    assert!(stdout.is_empty());
    assert!(stderr.contains("out of range"));
}

#[test]
fn test_set_value_non_integer_index_on_sequence() {
    let (stdout, stderr, success) =
        run_shyaml(&["set-value", "items.foo", "x"], "items: [a, b, c]\n");
    assert!(!success);
    assert!(stdout.is_empty());
    assert!(stderr.contains("non-integer index"));
}

#[test]
fn test_set_value_cannot_set_on_scalar() {
    let (stdout, stderr, success) = run_shyaml(&["set-value", "name.sub", "x"], "name: scalar\n");
    assert!(!success);
    assert!(stdout.is_empty());
    assert!(stderr.contains("cannot"));
}

#[test]
fn test_set_value_escaped_dot_in_key() {
    let (stdout, stderr, success) =
        run_shyaml(&["set-value", r"config\.key", "value"], "config: {}\n");
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            config: {}
            config.key: value
        "},
    );
}

#[test]
fn test_set_value_empty_input() {
    let (stdout, stderr, success) = run_shyaml(&["set-value", "key", "value"], "");
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(&stdout, "key: value\n");
}

#[test]
fn test_set_value_overwrite_existing() {
    let (stdout, stderr, success) = run_shyaml(
        &["set-value", "config.port", "3306"],
        "config:\n  host: localhost\n  port: 5432\n",
    );
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            config:
              host: localhost
              port: 3306
        "},
    );
}

#[test]
fn test_set_value_traverse_sequence_then_set() {
    let (stdout, stderr, success) = run_shyaml(
        &["set-value", "users.0.name", "alice"],
        "users:\n  - name: bob\n    age: 30\n",
    );
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            users:
            - name: alice
              age: 30
        "},
    );
}
