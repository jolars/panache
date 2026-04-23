//! Format subcommand tests

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::thread;
use std::time::Duration;
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
        .stdout(predicate::str::contains("1 file left unchanged"))
        .stdout(predicate::str::contains("snapshot.md").not());
}

#[test]
fn test_format_directory_reports_only_changed_files_and_summary() {
    let temp_dir = TempDir::new().unwrap();
    let changed = temp_dir.path().join("changed.qmd");
    let unchanged = temp_dir.path().join("unchanged.qmd");

    fs::write(
        &changed,
        "# Heading\n\nThis is a very long line that exceeds the default line width of 80 characters and should be wrapped when formatted.",
    )
    .unwrap();
    fs::write(&unchanged, "# Unchanged\n\nParagraph.\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", temp_dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Formatted").and(predicate::str::contains("changed.qmd")))
        .stdout(predicate::str::contains("unchanged.qmd").not())
        .stdout(predicate::str::contains(
            "1 file reformatted, 1 file left unchanged",
        ));
}

#[test]
fn test_format_directory_all_unchanged_prints_summary_only() {
    let temp_dir = TempDir::new().unwrap();
    let file1 = temp_dir.path().join("a.qmd");
    let file2 = temp_dir.path().join("b.md");

    fs::write(&file1, "# A\n\nParagraph.\n").unwrap();
    fs::write(&file2, "# B\n\nParagraph.\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", temp_dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Formatted").not())
        .stdout(predicate::str::contains("2 files left unchanged"));
}

#[test]
fn test_format_directory_include_patterns_resolve_from_config_root() {
    let temp_dir = TempDir::new().unwrap();
    let docs_dir = temp_dir.path().join("docs");
    let root_file = docs_dir.join("index.qmd");
    let nested_dir = docs_dir.join("guides");
    let nested_file = nested_dir.join("intro.qmd");
    let config = temp_dir.path().join(".panache.toml");

    fs::create_dir_all(&nested_dir).unwrap();
    fs::write(&root_file, "# Root\n\nParagraph.\n").unwrap();
    fs::write(&nested_file, "# Nested\n\nParagraph.\n").unwrap();
    fs::write(&config, "include = [\"docs/**/*.qmd\"]\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["format", "--check", "docs"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "All 2 files are correctly formatted",
        ));
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
fn test_format_directory_with_no_supported_files_is_noop() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("note.txt");
    fs::write(&test_file, "content\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["format", temp_dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No supported files found"));
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

#[test]
fn test_format_check_cache_reuse_and_config_invalidation() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let cache_dir = temp_dir.path().join(".panache-cache");
    let cache_file = cache_dir.join("cli-cache-v1.bin");
    let config = temp_dir.path().join(".panache.toml");

    fs::write(&test_file, "# Heading\n\nParagraph.\n").unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "format",
            "--check",
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("correctly formatted"));
    assert!(cache_file.exists(), "expected cache file to be created");

    let first_modified = fs::metadata(&cache_file).unwrap().modified().unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "format",
            "--check",
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("correctly formatted"));

    let second_modified = fs::metadata(&cache_file).unwrap().modified().unwrap();
    assert_eq!(
        first_modified, second_modified,
        "cache file should not be rewritten on a no-change rerun"
    );

    thread::sleep(Duration::from_millis(5));
    fs::write(&config, "line-width = 120\n").unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "format",
            "--check",
            "--config",
            config.to_str().unwrap(),
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("correctly formatted"));

    let third_modified = fs::metadata(&cache_file).unwrap().modified().unwrap();
    assert!(
        third_modified > second_modified,
        "cache file should be rewritten after config fingerprint changes"
    );
}

#[test]
fn test_format_no_cache_skips_cache_file_creation() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let cache_dir = temp_dir.path().join(".panache-cache");
    let cache_file = cache_dir.join("cli-cache-v1.bin");
    fs::write(&test_file, "# Heading\n\nParagraph.\n").unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "--no-cache",
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "format",
            "--check",
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(
        !cache_file.exists(),
        "--no-cache should disable cache reads and writes"
    );
}

#[cfg(unix)]
#[test]
fn test_format_ignores_unwritable_global_cache_dir() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let cache_home = temp_dir.path().join("cache-home");
    fs::create_dir_all(&cache_home).unwrap();
    fs::write(&test_file, "# Heading\n\nParagraph.\n").unwrap();

    let mut perms = fs::metadata(&cache_home).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(&cache_home, perms).unwrap();

    cargo_bin_cmd!("panache")
        .env("XDG_CACHE_HOME", &cache_home)
        .args(["format", "--check", test_file.to_str().unwrap()])
        .assert()
        .success();

    let mut restore = fs::metadata(&cache_home).unwrap().permissions();
    restore.set_mode(0o700);
    fs::set_permissions(&cache_home, restore).unwrap();
}
