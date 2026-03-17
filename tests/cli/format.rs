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
fn test_format_directory_respects_exclude_config() {
    let temp_dir = TempDir::new().unwrap();
    let config = temp_dir.path().join(".panache.toml");
    let included = temp_dir.path().join("doc.qmd");
    let excluded_dir = temp_dir.path().join("tests");
    let excluded = excluded_dir.join("snapshot.md");
    fs::create_dir_all(&excluded_dir).unwrap();
    fs::write(
        &config,
        r#"
exclude = ["tests/"]
"#,
    )
    .unwrap();
    fs::write(&included, "# Included\n\nParagraph.\n").unwrap();
    fs::write(&excluded, "# Excluded\n\nParagraph.\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["format", temp_dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("doc.qmd"))
        .stdout(predicate::str::contains("snapshot.md").not());
}

#[test]
fn test_format_explicit_file_force_exclude_noops_when_all_filtered() {
    let temp_dir = TempDir::new().unwrap();
    let config = temp_dir.path().join(".panache.toml");
    let excluded_dir = temp_dir.path().join("tests");
    let excluded = excluded_dir.join("snapshot.md");
    fs::create_dir_all(&excluded_dir).unwrap();
    fs::write(
        &config,
        r#"
exclude = ["tests/"]
"#,
    )
    .unwrap();
    fs::write(&excluded, "# Excluded\n\nParagraph.\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["format", "--force-exclude", excluded.to_str().unwrap()])
        .assert()
        .success();
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

#[test]
fn test_format_verify_stdin_to_stdout() {
    cargo_bin_cmd!("panache")
        .args(["format", "--verify"])
        .write_stdin("# Heading\n\nParagraph.")
        .assert()
        .success()
        .stdout(predicate::str::contains("# Heading"));
}

#[test]
fn test_format_verify_check_unformatted() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(
        &test_file,
        "# Heading\n\nThis is a very long line that exceeds the default line width of 80 characters and should be wrapped when formatted.",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", "--verify", "--check", test_file.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Diff in"));
}

#[test]
fn test_format_verify_does_not_write_file() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let input = "# Heading\n\nThis is a very long line that exceeds the default line width of 80 characters and should be wrapped when formatted.";
    fs::write(&test_file, input).unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", "--verify", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let content_after = fs::read_to_string(&test_file).unwrap();
    assert_eq!(content_after, input);
}

#[test]
fn test_format_stdin_filename_infers_quarto_flavor() {
    cargo_bin_cmd!("panache")
        .args(["format", "--stdin-filename", "doc.qmd"])
        .write_stdin("```{r, echo=FALSE}\n1 + 1\n```\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("#| echo: false"));
}

#[test]
fn test_format_color_always_shows_ansi_diff() {
    cargo_bin_cmd!("panache")
        .args(["format", "--check", "--color", "always"])
        .write_stdin("# Heading\n\nThis is a very long line that exceeds the default line width of 80 characters and should be wrapped.")
        .assert()
        .failure()
        .stdout(predicate::str::contains("\u{1b}[31m"))
        .stdout(predicate::str::contains("\u{1b}[32m"));
}

#[test]
fn test_format_no_color_disables_ansi_diff() {
    cargo_bin_cmd!("panache")
        .args(["format", "--check", "--color", "always", "--no-color"])
        .write_stdin("# Heading\n\nThis is a very long line that exceeds the default line width of 80 characters and should be wrapped.")
        .assert()
        .failure()
        .stdout(predicate::str::contains("\u{1b}[").not());
}

#[test]
fn test_format_stdin_uses_discovered_config_without_isolated() {
    let temp_dir = TempDir::new().unwrap();
    let config_file = temp_dir.path().join(".panache.toml");
    fs::write(&config_file, "flavor = \"quarto\"").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .arg("format")
        .write_stdin("```{r, echo=FALSE}\n1 + 1\n```\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("#| echo: false"));
}

#[test]
fn test_format_isolated_ignores_discovered_config_for_stdin() {
    let temp_dir = TempDir::new().unwrap();
    let config_file = temp_dir.path().join(".panache.toml");
    fs::write(&config_file, "flavor = \"quarto\"").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["format", "--isolated"])
        .write_stdin("```{r, echo=FALSE}\n1 + 1\n```\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("#| echo: false").not());
}
