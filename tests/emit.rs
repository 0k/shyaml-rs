//! Integration tests for YAML emission fidelity.
//!
//! Emitted YAML must round-trip: re-parsing shyaml's output must yield
//! the exact same values as the original document.

mod common;

use common::{assert_output_eq, run_shyaml};

/// A single-quoted scalar longer than the default emitter wrap width
/// (80 columns) must round-trip unchanged.
///
/// Regression test: libfyaml's emitter wraps such lines by inserting a
/// trailing `\` line-continuation, which is only valid in double-quoted
/// style. In single-quoted style the `\` is a literal character, so the
/// re-parsed value gains a spurious `\` + folded space. Fixed by fyaml
/// 0.5.2, which forces infinite emit width (`FYECF_WIDTH_INF`).
#[test]
fn test_long_single_quoted_scalar_round_trips() {
    let value = "x".repeat(81);
    let input = format!("key: '{}'\n", value);

    let (emitted, stderr, success) = run_shyaml(&["get-value", "-y"], &input);
    assert!(success, "emit failed: {}", stderr);

    let (reparsed, stderr, success) = run_shyaml(&["get-value", "key"], &emitted);
    assert!(
        success,
        "re-parse failed: {}\nemitted was:\n{}",
        stderr, emitted
    );

    assert_output_eq(&reparsed, &value);
}
