//! Integration tests for compound (chained) commands

mod common;

use common::{assert_output_eq, run_shyaml};
use indoc::indoc;

#[test]
fn test_compound_set_value_twice() {
    let input = indoc! {r#"
        a: 1
    "#};

    let (stdout, stderr, success) =
        run_shyaml(&["set-value", "b", "2", ";", "set-value", "c", "3"], input);

    assert!(success, "Expected success, got stderr: {}", stderr);
    let expected = indoc! {r#"
        a: 1
        b: 2
        c: 3
    "#};
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_compound_set_then_del() {
    let input = indoc! {r#"
        a: 1
        b: 2
    "#};

    let (stdout, stderr, success) = run_shyaml(&["set-value", "c", "3", ";", "del", "a"], input);

    assert!(success, "Expected success, got stderr: {}", stderr);
    let expected = indoc! {r#"
        b: 2
        c: 3
    "#};
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_compound_del_then_set() {
    let input = indoc! {r#"
        a: 1
        b: 2
    "#};

    let (stdout, stderr, success) = run_shyaml(&["del", "a", ";", "set-value", "c", "3"], input);

    assert!(success, "Expected success, got stderr: {}", stderr);
    let expected = indoc! {r#"
        b: 2
        c: 3
    "#};
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_compound_three_operations() {
    let input = indoc! {r#"
        x: 1
    "#};

    let (stdout, stderr, success) = run_shyaml(
        &[
            "set-value",
            "a",
            "10",
            ";",
            "set-value",
            "b",
            "20",
            ";",
            "set-value",
            "c",
            "30",
        ],
        input,
    );

    assert!(success, "Expected success, got stderr: {}", stderr);
    let expected = indoc! {r#"
        x: 1
        a: 10
        b: 20
        c: 30
    "#};
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_compound_modify_existing_key() {
    let input = indoc! {r#"
        a: 1
        b: 2
    "#};

    let (stdout, stderr, success) = run_shyaml(
        &["set-value", "a", "100", ";", "set-value", "b", "200"],
        input,
    );

    assert!(success, "Expected success, got stderr: {}", stderr);
    let expected = indoc! {r#"
        a: 100
        b: 200
    "#};
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_compound_nested_path() {
    let input = indoc! {r#"
        config:
          server:
            host: localhost
    "#};

    let (stdout, stderr, success) = run_shyaml(
        &[
            "set-value",
            "config.server.port",
            "8080",
            ";",
            "set-value",
            "config.debug",
            "true",
            "-y",
        ],
        input,
    );

    assert!(success, "Expected success, got stderr: {}", stderr);
    let expected = indoc! {r#"
        config:
          server:
            host: localhost
            port: 8080
          debug: true
    "#};
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_compound_del_multiple() {
    let input = indoc! {r#"
        a: 1
        b: 2
        c: 3
        d: 4
    "#};

    let (stdout, stderr, success) = run_shyaml(&["del", "a", ";", "del", "c"], input);

    assert!(success, "Expected success, got stderr: {}", stderr);
    let expected = indoc! {r#"
        b: 2
        d: 4
    "#};
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_compound_no_intermediate_output() {
    let input = indoc! {r#"
        value: original
    "#};

    let (stdout, stderr, success) = run_shyaml(
        &[
            "set-value",
            "value",
            "first",
            ";",
            "set-value",
            "value",
            "second",
            ";",
            "set-value",
            "value",
            "final",
        ],
        input,
    );

    assert!(success, "Expected success, got stderr: {}", stderr);
    assert!(
        !stdout.contains("first"),
        "Should not contain intermediate value 'first'"
    );
    assert!(
        !stdout.contains("second"),
        "Should not contain intermediate value 'second'"
    );
    let expected = indoc! {r#"
        value: final
    "#};
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_compound_with_yaml_value() {
    let input = indoc! {r#"
        items: []
    "#};

    let (stdout, stderr, success) = run_shyaml(
        &[
            "set-value",
            "items",
            "[a, b, c]",
            "-y",
            ";",
            "set-value",
            "count",
            "3",
        ],
        input,
    );

    assert!(success, "Expected success, got stderr: {}", stderr);
    let expected = indoc! {r#"
        items:
        - a
        - b
        - c
        count: 3
    "#};
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_single_command_still_works() {
    let input = indoc! {r#"
        a: 1
    "#};

    let (stdout, stderr, success) = run_shyaml(&["set-value", "b", "2"], input);

    assert!(success, "Expected success, got stderr: {}", stderr);
    let expected = indoc! {r#"
        a: 1
        b: 2
    "#};
    assert_output_eq(&stdout, expected);
}

#[test]
fn test_backslash_semicolon_is_not_separator() {
    // When user types `shyaml set-value a 1 '\;' set-value b 2` in shell,
    // the '\;' is passed literally as `\;` (not `;`).
    // This should NOT be treated as a command separator.
    let input = "a: 1\n";

    let (_stdout, _stderr, success) = run_shyaml(
        &["set-value", "b", "2", r"\;", "set-value", "c", "3"],
        input,
    );

    assert!(
        !success,
        "Expected failure when using literal '\\;' instead of ';'"
    );
}
