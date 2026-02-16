//! Lint subcommand tests

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_lint_clean_file() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n\n## Subheading\n\nParagraph.").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found"));
}

#[test]
fn test_lint_with_violations() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(
        &test_file,
        "# Heading\n\n### Subheading\n\nSkipped heading level.",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("warning"))
        .stdout(predicate::str::contains("heading-hierarchy"));
}

#[test]
fn test_lint_check_mode_clean() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n\n## Subheading\n\nParagraph.").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "--check", test_file.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn test_lint_check_mode_violations() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n\n### Subheading").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "--check", test_file.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Found"));
}

#[test]
fn test_lint_fix_mode() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n\n### Subheading\n\nContent.").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "--fix", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fixed"));

    let content = fs::read_to_string(&test_file).unwrap();
    // Heading should be fixed to h2
    assert!(content.contains("## Subheading"));
}

#[test]
fn test_lint_fix_stdin() {
    cargo_bin_cmd!("panache")
        .arg("lint")
        .arg("--fix")
        .write_stdin("# Heading\n\n### Subheading")
        .assert()
        .success()
        .stdout(predicate::str::contains("## Subheading"));
}

#[test]
fn test_lint_multiple_files() {
    let temp_dir = TempDir::new().unwrap();
    let file1 = temp_dir.path().join("test1.qmd");
    let file2 = temp_dir.path().join("test2.qmd");

    fs::write(&file1, "# Heading\n\n### Subheading").unwrap();
    fs::write(&file2, "# Heading\n\n## Subheading").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", file1.to_str().unwrap(), file2.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("test1.qmd"));
}

#[test]
fn test_lint_directory() {
    let temp_dir = TempDir::new().unwrap();
    let file1 = temp_dir.path().join("test1.qmd");
    let file2 = temp_dir.path().join("test2.md");

    fs::write(&file1, "# Heading\n\n### Subheading").unwrap();
    fs::write(&file2, "# Heading\n\n## Subheading").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", temp_dir.path().to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn test_lint_stdin() {
    cargo_bin_cmd!("panache")
        .arg("lint")
        .write_stdin("# Heading\n\n### Subheading")
        .assert()
        .success()
        .stdout(predicate::str::contains("warning"));
}
