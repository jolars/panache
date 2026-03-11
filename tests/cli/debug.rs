//! Debug subcommand tests

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_debug_format_stdin_success() {
    cargo_bin_cmd!("panache")
        .args(["debug", "format"])
        .write_stdin("# Heading\n\nParagraph.\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("All checks passed"));
}

#[test]
fn test_debug_format_json_output() {
    cargo_bin_cmd!("panache")
        .args(["debug", "format", "--json"])
        .write_stdin("# Heading\n\nParagraph.\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"checks\""))
        .stdout(predicate::str::contains("\"files_checked\""))
        .stdout(predicate::str::contains("\"failures\""));
}

#[test]
fn test_debug_format_directory_uses_supported_extensions() {
    let temp_dir = TempDir::new().unwrap();
    let file_md = temp_dir.path().join("doc.md");
    let file_txt = temp_dir.path().join("ignore.txt");
    fs::write(&file_md, "# Heading\n\nParagraph.\n").unwrap();
    fs::write(&file_txt, "not markdown\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["debug", "format", temp_dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("files: 1"));
}
