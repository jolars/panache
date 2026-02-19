//! Parse subcommand tests

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_parse_stdin() {
    cargo_bin_cmd!("panache")
        .arg("parse")
        .write_stdin("# Heading\n\nParagraph.")
        .assert()
        .success()
        .stdout(predicate::str::contains("DOCUMENT"))
        .stdout(predicate::str::contains("HEADING"));
}

#[test]
fn test_parse_simple_file() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n\nParagraph with *emphasis*.").unwrap();

    cargo_bin_cmd!("panache")
        .args(["parse", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("DOCUMENT"))
        .stdout(predicate::str::contains("HEADING"))
        .stdout(predicate::str::contains("PARAGRAPH"));
}

#[test]
fn test_parse_with_config() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let config_file = temp_dir.path().join(".panache.toml");

    fs::write(&test_file, "Inline math: $x^2$").unwrap();
    fs::write(
        &config_file,
        "flavor = \"quarto\"\n\n[extensions]\ntex_math_dollars = true",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "parse",
            "--config",
            config_file.to_str().unwrap(),
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("DOCUMENT"));
}

#[test]
fn test_parse_complex_document() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(
        &test_file,
        "# Heading\n\n## Subheading\n\n- Item 1\n- Item 2\n\n| A | B |\n|---|---|\n| 1 | 2 |",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["parse", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("HEADING"))
        .stdout(predicate::str::contains("LIST"))
        .stdout(predicate::str::contains("PIPE_TABLE"));
}

#[test]
fn test_parse_handles_complex_syntax() {
    // Parser should not panic on complex/edge case syntax
    cargo_bin_cmd!("panache")
        .arg("parse")
        .write_stdin("# Heading\n\n```python\ncode\n```\n\n$$\nx^2\n$$")
        .assert()
        .success()
        .stdout(predicate::str::contains("DOCUMENT"));
}
