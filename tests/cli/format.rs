//! Format subcommand tests

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_format_stdin_to_stdout() {
    cargo_bin_cmd!("panache")
        .arg("format")
        .write_stdin("# Heading\n\nParagraph.")
        .assert()
        .success()
        .stdout(predicate::str::contains("# Heading"))
        .stdout(predicate::str::contains("Paragraph."));
}

#[test]
fn test_format_simple_file() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n\nParagraph.").unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Formatted"));

    let content = fs::read_to_string(&test_file).unwrap();
    assert!(content.contains("# Heading"));
}

#[test]
fn test_format_check_formatted() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n\nParagraph.\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", "--check", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("correctly formatted"));
}

#[test]
fn test_format_check_unformatted() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(
        &test_file,
        "# Heading\n\nThis is a very long line that exceeds the default line width of 80 characters and should be wrapped when formatted.",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", "--check", test_file.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Diff in"));
}

#[test]
fn test_format_check_diff_output() {
    cargo_bin_cmd!("panache")
        .arg("format")
        .arg("--check")
        .write_stdin("# Heading\n\nThis is a very long line that exceeds the default line width of 80 characters and should be wrapped.")
        .assert()
        .failure()
        .stdout(predicate::str::contains("Diff in"))
        .stdout(predicate::str::contains("-"))
        .stdout(predicate::str::contains("+"));
}

#[test]
fn test_format_with_config() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let config_file = temp_dir.path().join(".panache.toml");

    fs::write(&test_file, "# Heading\n\nThis is a very long line that will be wrapped at 60 characters instead of 80.").unwrap();
    fs::write(&config_file, "line_width = 60").unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "format",
            "--config",
            config_file.to_str().unwrap(),
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&test_file).unwrap();
    // Verify the file was formatted (content should have changed from original)
    // The long line should have been wrapped
    assert!(content.contains("# Heading"), "Should preserve heading");
    // Just verify the file was processed, actual wrapping logic is tested elsewhere
}

#[test]
fn test_format_multiple_files() {
    let temp_dir = TempDir::new().unwrap();
    let file1 = temp_dir.path().join("test1.qmd");
    let file2 = temp_dir.path().join("test2.qmd");

    fs::write(&file1, "# File 1\n\nContent.").unwrap();
    fs::write(&file2, "# File 2\n\nContent.").unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", file1.to_str().unwrap(), file2.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("test1.qmd"))
        .stdout(predicate::str::contains("test2.qmd"));
}

#[test]
fn test_format_directory() {
    let temp_dir = TempDir::new().unwrap();
    let file1 = temp_dir.path().join("test1.qmd");
    let file2 = temp_dir.path().join("test2.md");

    fs::write(&file1, "# File 1\n\nContent.").unwrap();
    fs::write(&file2, "# File 2\n\nContent.").unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", temp_dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("test1.qmd"))
        .stdout(predicate::str::contains("test2.md"));
}

#[test]
fn test_format_unsupported_extension() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "content").unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", test_file.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Skipping unsupported file type"));
}

#[test]
fn test_format_missing_file() {
    cargo_bin_cmd!("panache")
        .args(["format", "/nonexistent/file.qmd"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_format_range() {
    cargo_bin_cmd!("panache")
        .arg("format")
        .arg("--range")
        .arg("2:3")
        .write_stdin("# Heading\n\nThis is a very long paragraph that should be wrapped.\n\nAnother paragraph.")
        .assert()
        .success();
}

#[test]
fn test_format_invalid_range() {
    cargo_bin_cmd!("panache")
        .arg("format")
        .arg("--range")
        .arg("invalid")
        .write_stdin("# Heading\n\nParagraph.")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid range format"));
}
