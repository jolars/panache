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

#[test]
fn test_parse_json_output() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let output_file = temp_dir.path().join("cst.json");

    fs::write(&test_file, "# Heading\n\nParagraph.").unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "parse",
            "--json",
            output_file.to_str().unwrap(),
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    let json_output = fs::read_to_string(&output_file).unwrap();
    assert!(json_output.contains("\"kind\""));
    assert!(json_output.contains("\"DOCUMENT\""));
    assert!(json_output.contains("\"text\""));
}

#[test]
fn test_parse_verify_stdin() {
    cargo_bin_cmd!("panache")
        .args(["parse", "--verify"])
        .write_stdin("# Heading\n\nParagraph.")
        .assert()
        .success()
        .stdout(predicate::str::contains("DOCUMENT"));
}

#[test]
fn test_parse_quiet_stdin() {
    cargo_bin_cmd!("panache")
        .args(["parse", "--quiet"])
        .write_stdin("# Heading\n\nParagraph.")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn test_parse_verify_quiet_stdin() {
    cargo_bin_cmd!("panache")
        .args(["parse", "--verify", "--quiet"])
        .write_stdin("# Heading\n\nParagraph.")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn test_parse_stdin_filename_infers_quarto_flavor() {
    cargo_bin_cmd!("panache")
        .args(["parse", "--stdin-filename", "doc.qmd"])
        .write_stdin("{{< include _child.qmd >}}")
        .assert()
        .success()
        .stdout(predicate::str::contains("SHORTCODE"));
}

#[test]
fn test_parse_to_pandoc_ast_stdin() {
    cargo_bin_cmd!("panache")
        .args(["parse", "--to", "pandoc-ast"])
        .write_stdin("# Heading\n\nA **bold** word.")
        .assert()
        .success()
        .stdout(predicate::str::contains("Header 1"))
        .stdout(predicate::str::contains("Para"))
        .stdout(predicate::str::contains("Strong"))
        .stdout(predicate::str::contains("Str \"bold\""))
        // Make sure we did not also dump the CST debug tree.
        .stdout(predicate::str::contains("DOCUMENT").not());
}

#[test]
fn test_parse_default_format_is_cst() {
    cargo_bin_cmd!("panache")
        .arg("parse")
        .write_stdin("# Heading")
        .assert()
        .success()
        .stdout(predicate::str::contains("DOCUMENT"))
        .stdout(predicate::str::contains("HEADING"))
        // CST debug output, not pandoc-ast.
        .stdout(predicate::str::contains("Header 1").not());
}

#[test]
fn test_parse_to_pandoc_ast_with_json_writes_both() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let output_file = temp_dir.path().join("cst.json");

    fs::write(&test_file, "# Heading").unwrap();

    // --to controls stdout, --json keeps writing CST JSON to file.
    cargo_bin_cmd!("panache")
        .args([
            "parse",
            "--to",
            "pandoc-ast",
            "--json",
            output_file.to_str().unwrap(),
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Header 1"));

    let json_output = fs::read_to_string(&output_file).unwrap();
    assert!(json_output.contains("\"DOCUMENT\""));
}
