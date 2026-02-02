//! Common test utilities shared across integration tests.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use similar::TextDiff;
use tempfile::TempDir;

/// Get path to the shyaml binary.
pub fn binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_shyaml"))
}

/// Run shyaml with given args and stdin, return (stdout, stderr, success).
pub fn run_shyaml(args: &[&str], stdin_data: &str) -> (String, String, bool) {
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

/// Assert that actual output equals expected, showing a colored diff on failure.
pub fn assert_output_eq(actual: &str, expected: &str) {
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

/// Create a temporary file with given content, return its path.
#[allow(dead_code)] // Used by apply.rs and merge_tags.rs, not all test files
pub fn temp_yaml_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, content).expect("Failed to write temp file");
    path
}
