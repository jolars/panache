//! Cross-cutting CLI tests (help, version, error handling)

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn test_help() {
    cargo_bin_cmd!("panache")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Panache is a CLI formatter"));
}

#[test]
fn test_version() {
    cargo_bin_cmd!("panache")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_no_subcommand() {
    cargo_bin_cmd!("panache")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn test_invalid_subcommand() {
    cargo_bin_cmd!("panache")
        .arg("invalid")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn test_format_help() {
    cargo_bin_cmd!("panache")
        .args(["format", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Format a Quarto"));
}

#[test]
fn test_parse_help() {
    cargo_bin_cmd!("panache")
        .args(["parse", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Parse"));
}

#[test]
fn test_lint_help() {
    cargo_bin_cmd!("panache")
        .args(["lint", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Lint a"));
}
