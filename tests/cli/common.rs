//! Cross-cutting CLI tests (help, version, error handling)

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_help() {
    cargo_bin_cmd!("panache")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Panache is a language server, formatter, and linter for Quarto",
        ))
        .stdout(predicate::str::contains("Global options:"))
        .stdout(predicate::str::contains("--color <WHEN>"))
        .stdout(predicate::str::contains("--no-color"))
        .stdout(predicate::str::contains("--isolated"))
        .stdout(predicate::str::contains("clean"))
        .stdout(predicate::str::contains("inspect"));
}

#[test]
fn test_help_forced_color_outputs_ansi() {
    cargo_bin_cmd!("panache")
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("\u{1b}["));
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

/// Top-level errors are rendered via `Display`, so a config error must read as
/// a plain message instead of leaking the `io::Error` `Debug` wrapper.
#[test]
fn test_invalid_config_error_is_not_debug_formatted() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("panache.toml"),
        "line-width = \"wide\"\n",
    )
    .unwrap();
    let doc = temp_dir.path().join("doc.md");
    fs::write(&doc, "# Title\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", "--check"])
        .arg(&doc)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Error: invalid config"))
        .stderr(predicate::str::contains("expected usize"))
        .stderr(predicate::str::contains("Custom {").not())
        .stderr(predicate::str::contains("InvalidData").not());
}

/// A plain OS error carries no custom payload; it must still print its message
/// rather than the `Os { code: .. }` debug form, and it must name the file it
/// failed on.
#[test]
fn test_os_error_is_not_debug_formatted() {
    let temp_dir = TempDir::new().unwrap();
    let missing = temp_dir.path().join("nope.md");

    // The wording of "file not found" belongs to the platform, not to us, so
    // take it from the same read the binary performs instead of hardcoding the
    // Unix phrasing.
    let not_found = fs::read_to_string(&missing).unwrap_err();

    cargo_bin_cmd!("panache")
        .arg("parse")
        .arg(&missing)
        .assert()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "Error: {}: {not_found}",
            missing.display()
        )))
        .stderr(predicate::str::contains("Os {").not());
}

/// A decoding failure has no OS-level path either, so the read site must supply
/// it; otherwise a batch run cannot tell which file was not UTF-8.
#[test]
fn test_undecodable_file_error_names_the_file() {
    let temp_dir = TempDir::new().unwrap();
    let doc = temp_dir.path().join("binary.md");
    fs::write(&doc, [0x23, 0x20, 0xff, 0xfe, 0x0a]).unwrap();

    cargo_bin_cmd!("panache")
        .arg("parse")
        .arg(&doc)
        .assert()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "Error: {}: stream did not contain valid UTF-8",
            doc.display()
        )));
}

/// `format` reads through its own call site, and it walks several files, so the
/// path matters even more there.
#[test]
fn test_format_read_error_names_the_file() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(temp_dir.path().join("good.md"), "# Title\n").unwrap();
    let bad = temp_dir.path().join("binary.md");
    fs::write(&bad, [0x23, 0x20, 0xff, 0xfe, 0x0a]).unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", "--check"])
        .arg(temp_dir.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "Error: {}: stream did not contain valid UTF-8",
            bad.display()
        )));
}

#[test]
fn test_clean_help() {
    cargo_bin_cmd!("panache")
        .args(["clean", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Delete Panache"))
        .stdout(predicate::str::contains("--all"));
}
