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
        "flavor = \"quarto\"\n\n[extensions]\ntex-math-dollars = true",
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
fn test_parse_quiet_stdin() {
    cargo_bin_cmd!("panache")
        .args(["parse", "--quiet"])
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
fn test_parse_flavor_quarto_enables_chunk_options() {
    // Quarto enables executable_code, so ```{r, echo=FALSE} is parsed as a
    // CODE_BLOCK with CHUNK_OPTIONS rather than the plain Pandoc fallback.
    cargo_bin_cmd!("panache")
        .args(["parse", "--flavor", "quarto"])
        .write_stdin("```{r, echo=FALSE}\n1 + 1\n```\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("CHUNK_OPTIONS"));
}

#[test]
fn test_parse_flavor_overrides_stdin_filename() {
    // --stdin-filename suggests Quarto (which would emit CHUNK_OPTIONS), but
    // --flavor pandoc forces pandoc, so chunk options are not recognized.
    cargo_bin_cmd!("panache")
        .args(["parse", "--flavor", "pandoc", "--stdin-filename", "doc.qmd"])
        .write_stdin("```{r, echo=FALSE}\n1 + 1\n```\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("CHUNK_OPTIONS").not());
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
fn test_parse_dash_reads_stdin() {
    cargo_bin_cmd!("panache")
        .args(["parse", "-"])
        .write_stdin("# Heading\n\nParagraph.")
        .assert()
        .success()
        .stdout(predicate::str::contains("DOCUMENT"))
        .stdout(predicate::str::contains("HEADING"));
}

#[test]
fn test_parse_dash_with_stdin_filename_infers_flavor() {
    cargo_bin_cmd!("panache")
        .args(["parse", "--stdin-filename", "doc.qmd", "-"])
        .write_stdin("{{< include _child.qmd >}}")
        .assert()
        .success()
        .stdout(predicate::str::contains("SHORTCODE"));
}

#[test]
fn test_parse_to_pandoc_json_stdin() {
    let assert = cargo_bin_cmd!("panache")
        .args(["parse", "--to", "pandoc-json"])
        .write_stdin("# Heading\n\nA **bold** word.")
        .assert()
        .success()
        // Compact JSON, pinned api-version, contains the expected node tags.
        .stdout(predicate::str::starts_with("{\"blocks\":"))
        .stdout(predicate::str::contains(
            "\"pandoc-api-version\":[1,23,1,1]",
        ))
        .stdout(predicate::str::contains("\"t\":\"Header\""))
        .stdout(predicate::str::contains("\"t\":\"Para\""))
        .stdout(predicate::str::contains("\"t\":\"Strong\""))
        .stdout(predicate::str::contains("\"t\":\"Str\""))
        .stdout(predicate::str::contains("\"bold\""))
        // Make sure we did not also dump the CST debug tree or pandoc-ast.
        .stdout(predicate::str::contains("DOCUMENT").not())
        .stdout(predicate::str::contains("Header 1").not());

    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    // Output must round-trip as valid JSON.
    serde_json::from_str::<serde_json::Value>(stdout.trim_end())
        .expect("pandoc-json output must be valid JSON");
}

#[test]
fn test_parse_to_pandoc_json_utf8_round_trips() {
    // Regression for issue #269 — pandoc-ast emits Haskell-style numeric
    // escapes for non-ASCII chars (matching real pandoc -t native), which
    // tools like ascii2uni can't decode. pandoc-json must keep UTF-8
    // intact so the value parses back to the original string.
    let assert = cargo_bin_cmd!("panache")
        .args(["parse", "--to", "pandoc-json"])
        .write_stdin("Räksmörgås")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(stdout.trim_end()).expect("valid JSON");
    let str_node_content = value
        .pointer("/blocks/0/c/0/c")
        .expect("Para → first inline → content");
    assert_eq!(str_node_content.as_str(), Some("Räksmörgås"));
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
