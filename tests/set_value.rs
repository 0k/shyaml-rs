//! Integration tests for the `set-value` action

mod common;

use common::{assert_output_eq, run_shyaml};
use indoc::indoc;

#[test]
fn test_set_value_simple() {
    let (stdout, stderr, success) = run_shyaml(&["set-value", "name", "new"], "name: old\n");
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(&stdout, "name: new\n");
}

#[test]
fn test_set_value_nested_path() {
    let (stdout, stderr, success) = run_shyaml(
        &["set-value", "config.host", "localhost"],
        "config:\n  existing: value\n",
    );
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            config:
              existing: value
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
        "data:\n  existing: value\n",
    );
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            data:
              existing: value
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
        "config:\n  existing: value\n",
    );
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {"
            config:
              existing: value
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
        "config:\n  existing: value\n",
    );
    assert!(success, "stderr: {}", stderr);
    assert_output_eq(
        &stdout,
        indoc! {r#"
            config:
              existing: value
              data: "{host: localhost}"
        "#},
    );
}

#[test]
fn test_set_value_sequence_index() {
    let (stdout, stderr, success) = run_shyaml(
        &["set-value", "items.1", "changed"],
        "items:\n  - a\n  - b\n  - c\n",
    );
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
    let (stdout, stderr, success) = run_shyaml(
        &["set-value", "items.-1", "last"],
        "items:\n  - a\n  - b\n  - c\n",
    );
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
    let (stdout, stderr, success) = run_shyaml(
        &["set-value", "items.-3", "first"],
        "items:\n  - a\n  - b\n  - c\n",
    );
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
    assert!(stderr.contains("out of bounds"));
}

#[test]
fn test_set_value_negative_index_out_of_range() {
    let (stdout, stderr, success) =
        run_shyaml(&["set-value", "items.-10", "x"], "items: [a, b, c]\n");
    assert!(!success);
    assert!(stdout.is_empty());
    assert!(stderr.contains("out of bounds"));
}

#[test]
fn test_set_value_non_integer_index_on_sequence() {
    let (stdout, stderr, success) =
        run_shyaml(&["set-value", "items.foo", "x"], "items: [a, b, c]\n");
    assert!(!success);
    assert!(stdout.is_empty());
    assert!(stderr.contains("invalid sequence index"));
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
