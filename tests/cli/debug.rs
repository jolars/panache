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
fn test_debug_format_report_output() {
    cargo_bin_cmd!("panache")
        .args(["debug", "format", "--report"])
        .write_stdin("# Heading\n\nParagraph.\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("# Debug-format regression report"))
        .stdout(predicate::str::contains("All checks passed."));
}

#[test]
fn test_debug_format_report_and_json_are_mutually_exclusive() {
    cargo_bin_cmd!("panache")
        .args(["debug", "format", "--json", "--report"])
        .write_stdin("# Heading\n\nParagraph.\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Error: --json and --report cannot be used together",
        ));
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

#[test]
fn test_debug_format_directory_respects_include_override() {
    let temp_dir = TempDir::new().unwrap();
    let config = temp_dir.path().join(".panache.toml");
    let file_qmd = temp_dir.path().join("doc.qmd");
    let file_md = temp_dir.path().join("doc.md");
    fs::write(
        &config,
        r#"
include = ["*.qmd"]
"#,
    )
    .unwrap();
    fs::write(&file_qmd, "# Heading\n\nParagraph.\n").unwrap();
    fs::write(&file_md, "# Heading\n\nParagraph.\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["debug", "format", temp_dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("files: 1"));
}

#[test]
fn test_debug_format_directory_with_no_supported_files_is_noop() {
    let temp_dir = TempDir::new().unwrap();
    let file_txt = temp_dir.path().join("ignore.txt");
    fs::write(&file_txt, "not markdown\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["debug", "format", temp_dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No supported files found"));
}

#[test]
fn test_debug_format_json_directory_with_no_supported_files_reports_zero_files() {
    let temp_dir = TempDir::new().unwrap();
    let file_txt = temp_dir.path().join("ignore.txt");
    fs::write(&file_txt, "not markdown\n").unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "debug",
            "format",
            "--json",
            temp_dir.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"files_checked\": 0"))
        .stdout(predicate::str::contains("\"failure_count\": 0"));
}

#[test]
fn test_debug_format_dump_passes_requires_dump_dir() {
    cargo_bin_cmd!("panache")
        .args(["debug", "format", "--dump-passes"])
        .write_stdin("# Heading\n\nParagraph.\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Error: --dump-passes requires --dump-dir <DIR>",
        ));
}

#[test]
fn test_debug_format_dump_passes_writes_artifacts() {
    let temp_dir = TempDir::new().unwrap();
    let dump_dir = temp_dir.path().join("debug-artifacts");

    cargo_bin_cmd!("panache")
        .args([
            "debug",
            "format",
            "--checks",
            "all",
            "--dump-dir",
            dump_dir.to_str().unwrap(),
            "--dump-passes",
        ])
        .write_stdin("# Heading\n\nParagraph.\n")
        .assert()
        .success();

    let entries = fs::read_dir(&dump_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert!(
        entries
            .iter()
            .any(|name| name == "stdin.losslessness.input.txt"),
        "missing losslessness input artifact"
    );
    assert!(
        entries
            .iter()
            .any(|name| name == "stdin.losslessness.parsed.txt"),
        "missing losslessness parsed artifact"
    );
    assert!(
        entries
            .iter()
            .any(|name| name == "stdin.idempotency.input.txt"),
        "missing idempotency input artifact"
    );
    assert!(
        entries
            .iter()
            .any(|name| name == "stdin.idempotency.once.txt"),
        "missing idempotency first-pass artifact"
    );
    assert!(
        entries
            .iter()
            .any(|name| name == "stdin.idempotency.twice.txt"),
        "missing idempotency second-pass artifact"
    );
}

#[test]
fn test_debug_format_dash_reads_stdin() {
    cargo_bin_cmd!("panache")
        .args(["debug", "format", "-"])
        .write_stdin("# Heading\n\nParagraph.\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("All checks passed"));
}

#[test]
fn test_debug_format_dash_mixed_with_path_errors() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("doc.md");
    fs::write(&test_file, "# Heading\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["debug", "format", "-", test_file.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "'-' (stdin) cannot be combined with file path arguments",
        ));
}
