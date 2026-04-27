//! Lint subcommand tests

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::thread;
use std::time::Duration;
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
fn test_lint_directory_with_no_supported_files_is_noop() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("note.txt");
    fs::write(&test_file, "content\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", temp_dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No supported files found"));
}

#[test]
fn test_lint_directory_respects_exclude_config() {
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
    fs::write(&included, "# Heading\n\n## Subheading\n").unwrap();
    fs::write(&excluded, "# Heading\n\n### Skipped\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["lint", temp_dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found in 1 file(s)"));
}

#[test]
fn test_lint_directory_include_patterns_resolve_from_config_root() {
    let temp_dir = TempDir::new().unwrap();
    let docs_dir = temp_dir.path().join("docs");
    let root_file = docs_dir.join("index.qmd");
    let nested_dir = docs_dir.join("guides");
    let nested_file = nested_dir.join("intro.qmd");
    let config = temp_dir.path().join(".panache.toml");

    fs::create_dir_all(&nested_dir).unwrap();
    fs::write(&root_file, "# Root\n\n## Section\n").unwrap();
    fs::write(&nested_file, "# Nested\n\n## Section\n").unwrap();
    fs::write(&config, "include = [\"docs/**/*.qmd\"]\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["lint", "docs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found in 2 file(s)"));
}

#[test]
fn test_lint_explicit_file_force_exclude_noops_when_all_filtered() {
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
    fs::write(&excluded, "# Heading\n\n### Skipped\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["lint", "--force-exclude", excluded.to_str().unwrap()])
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

#[test]
fn test_lint_stdin_shows_source_snippet() {
    cargo_bin_cmd!("panache")
        .args(["lint", "--color", "never"])
        .write_stdin("# Heading\n\n### Subheading")
        .assert()
        .success()
        .stdout(predicate::str::contains("--> <stdin>:3:1"))
        .stdout(predicate::str::contains("3 | ### Subheading"))
        .stdout(predicate::str::contains("^"))
        .stdout(predicate::str::contains(
            "help: Change heading level from 3 to 2",
        ))
        .stdout(predicate::str::contains(
            "= note: configure this rule in panache.toml",
        ))
        .stdout(predicate::str::contains(
            "help: Change heading level from 3 to 2",
        ))
        .stdout(predicate::str::contains("previous heading is here"))
        .stdout(predicate::str::contains("3 - ### Subheading").not())
        .stdout(predicate::str::contains("3 + ## Subheading").not());
}

#[cfg(unix)]
#[test]
fn test_lint_ignores_unwritable_global_cache_dir() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let cache_home = temp_dir.path().join("cache-home");
    fs::create_dir_all(&cache_home).unwrap();
    fs::write(&test_file, "# Heading\n\n## Subheading\n\nParagraph.\n").unwrap();

    let mut perms = fs::metadata(&cache_home).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(&cache_home, perms).unwrap();

    cargo_bin_cmd!("panache")
        .env("XDG_CACHE_HOME", &cache_home)
        .args(["lint", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found"));

    let mut restore = fs::metadata(&cache_home).unwrap().permissions();
    restore.set_mode(0o700);
    fs::set_permissions(&cache_home, restore).unwrap();
}

#[test]
fn test_lint_stdin_short_message_format() {
    cargo_bin_cmd!("panache")
        .args(["lint", "--message-format", "short", "--color", "never"])
        .write_stdin("# Heading\n\n### Subheading")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "warning[heading-hierarchy]: Heading level skipped from h1 to h3; expected h2 at <stdin>:3:1",
        ))
        .stdout(predicate::str::contains("3 | ### Subheading").not())
        .stdout(predicate::str::contains("= note:").not());
}

#[test]
fn test_lint_file_short_message_format() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("short.qmd");
    fs::write(&test_file, "# Heading\n\n### Subheading\n").unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "lint",
            "--message-format",
            "short",
            "--color",
            "never",
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "warning[heading-hierarchy]: Heading level skipped",
        ))
        .stdout(predicate::str::contains("short.qmd:3:1"))
        .stdout(predicate::str::contains("3 | ### Subheading").not())
        .stdout(predicate::str::contains("= note:").not());
}

#[test]
fn test_lint_short_message_format_preserves_diagnostic_order() {
    let mut cmd = cargo_bin_cmd!("panache");
    cmd.args(["lint", "--message-format", "short", "--color", "never"])
        .write_stdin("# H1\n\n### H3\n\n##### H5\n");

    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    let first = stdout.find("<stdin>:3:1").unwrap();
    let second = stdout.find("<stdin>:5:1").unwrap();
    assert!(
        first < second,
        "expected diagnostics in source order: {stdout}"
    );
}

#[test]
fn test_lint_external_staticcheck_does_not_print_panache_rule_guidance() {
    if which::which("staticcheck").is_err() || which::which("go").is_err() {
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".panache.toml");
    let file_path = temp_dir.path().join("test.qmd");
    fs::write(&config_path, "[linters]\ngo = \"staticcheck\"\n").unwrap();
    fs::write(
        &file_path,
        "```go\npackage main\nimport \"fmt\"\nfunc main() {\n    fmt.Printf(\"%d\", \"x\")\n}\n```\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["lint", "--color", "never", file_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("[SA5009]"))
        .stdout(predicate::str::contains("[lint.rules] SA5009").not())
        .stdout(predicate::str::contains("linting.html#SA5009").not());
}

#[test]
fn test_lint_color_always_shows_ansi_diagnostics() {
    cargo_bin_cmd!("panache")
        .args(["lint", "--color", "always"])
        .write_stdin("# Heading\n\n### Subheading")
        .assert()
        .success()
        .stdout(predicate::str::contains("heading-hierarchy"))
        .stdout(predicate::str::contains("\u{1b}["));
}

#[test]
fn test_lint_color_never_disables_ansi_diagnostics() {
    cargo_bin_cmd!("panache")
        .args(["lint", "--color", "never"])
        .write_stdin("# Heading\n\n### Subheading")
        .assert()
        .success()
        .stdout(predicate::str::contains("warning"))
        .stdout(predicate::str::contains("\u{1b}[").not());
}

#[test]
fn test_lint_bibliography_integration() {
    let temp_dir = TempDir::new().unwrap();
    let bib_path = temp_dir.path().join("refs.bib");
    let doc_path = temp_dir.path().join("doc.qmd");

    fs::write(
        &bib_path,
        "@article{known,\n  title = {Known Title},\n  author = {Doe, Jane},\n  year = {2020}\n}\n",
    )
    .unwrap();

    fs::write(
        &doc_path,
        "---\nbibliography: refs.bib\n---\n\nCite [@known; @missing].\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing-bibliography-key"))
        .stdout(predicate::str::contains("missing"));
}

#[test]
fn test_lint_inline_references_in_metadata() {
    let temp_dir = TempDir::new().unwrap();
    let bib_path = temp_dir.path().join("refs.bib");
    let doc_path = temp_dir.path().join("doc.qmd");

    fs::write(
        &bib_path,
        "@article{known,\n  title = {Known Title},\n  author = {Doe, Jane},\n  year = {2020}\n}\n",
    )
    .unwrap();

    fs::write(
        &doc_path,
        "---\nbibliography: refs.bib\nreferences:\n  - id: inline\n    title: Inline\n---\n\nCite [@inline; @known; @missing].\n",
    )
    .unwrap();

    let dup_path = temp_dir.path().join("dup.qmd");
    fs::write(
        &dup_path,
        "---\nreferences:\n  - id: dupe\n    title: One\n  - id: dupe\n    title: Two\n---\n\nText\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing-bibliography-key"))
        .stdout(predicate::str::contains("missing"));

    cargo_bin_cmd!("panache")
        .args(["lint", dup_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("duplicate-inline-reference-id"));
}

#[test]
fn test_lint_reports_hashpipe_yaml_parse_error() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".panache.toml");
    let doc_path = temp_dir.path().join("doc.qmd");
    fs::write(
        &config_path,
        r#"flavor = "quarto"

[lint.rules]
missing-chunk-labels = false
"#,
    )
    .unwrap();
    fs::write(&doc_path, "```{r}\n#| echo: [\n1 + 1\n```\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["lint", "--color", "never", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("yaml-parse-error"))
        .stdout(predicate::str::contains("YAML parse error"));
}

#[test]
fn test_lint_csl_yaml_bibliography() {
    let temp_dir = TempDir::new().unwrap();
    let bib_path = temp_dir.path().join("refs.yaml");
    let doc_path = temp_dir.path().join("doc.qmd");

    fs::write(
        &bib_path,
        "- id: known\n  title: Known Title\n- id: other\n  title: Other Title\n",
    )
    .unwrap();

    fs::write(
        &doc_path,
        "---\nbibliography: refs.yaml\n---\n\nCite [@known; @missing].\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing-bibliography-key"))
        .stdout(predicate::str::contains("missing"));
}

#[test]
fn test_lint_csl_json_bibliography() {
    let temp_dir = TempDir::new().unwrap();
    let bib_path = temp_dir.path().join("refs.json");
    let doc_path = temp_dir.path().join("doc.qmd");

    fs::write(
        &bib_path,
        "[{\"id\":\"known\",\"title\":\"Known Title\"},{\"id\":\"other\",\"title\":\"Other Title\"}]",
    )
    .unwrap();

    fs::write(
        &doc_path,
        "---\nbibliography: refs.json\n---\n\nCite [@known; @missing].\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing-bibliography-key"))
        .stdout(predicate::str::contains("missing"));
}

#[test]
fn test_lint_ris_bibliography() {
    let temp_dir = TempDir::new().unwrap();
    let bib_path = temp_dir.path().join("refs.ris");
    let doc_path = temp_dir.path().join("doc.qmd");

    fs::write(&bib_path, "TY  - JOUR\nID  - known\nER  - \n").unwrap();

    fs::write(
        &doc_path,
        "---\nbibliography: refs.ris\n---\n\nCite [@known; @missing].\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing-bibliography-key"))
        .stdout(predicate::str::contains("missing"));
}

#[test]
fn test_lint_ris_missing_id() {
    let temp_dir = TempDir::new().unwrap();
    let bib_path = temp_dir.path().join("refs.ris");
    let doc_path = temp_dir.path().join("doc.qmd");

    fs::write(&bib_path, "TY  - JOUR\nER  - \n").unwrap();

    fs::write(
        &doc_path,
        "---\nbibliography: refs.ris\n---\n\nCite [@missing].\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing-bibliography-key"))
        .stdout(predicate::str::contains("missing"));
}

#[test]
fn test_lint_ris_invalid_tag() {
    let temp_dir = TempDir::new().unwrap();
    let bib_path = temp_dir.path().join("refs.ris");
    let doc_path = temp_dir.path().join("doc.qmd");

    fs::write(&bib_path, "TY  - JOUR\nID  - good\nOops\nER  - \n").unwrap();

    fs::write(
        &doc_path,
        "---\nbibliography: refs.ris\n---\n\nCite [@good].\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("bibliography-parse-error"))
        .stdout(predicate::str::contains("invalid content"));
}

#[test]
fn test_lint_includes_reports_child_diagnostics() {
    let temp_dir = TempDir::new().unwrap();
    let parent_path = temp_dir.path().join("parent.qmd");
    let child_path = temp_dir.path().join("_child.qmd");

    fs::write(&child_path, "# Heading 1\n\n### Heading 3\n").unwrap();
    fs::write(&parent_path, "{{< include _child.qmd >}}\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", parent_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("_child.qmd"))
        .stdout(predicate::str::contains("heading-hierarchy"));
}

#[test]
fn test_lint_includes_duplicate_reference_definitions() {
    let temp_dir = TempDir::new().unwrap();
    let parent_path = temp_dir.path().join("parent.qmd");
    let child_path = temp_dir.path().join("_child.qmd");

    fs::write(&child_path, "[ref]: https://example.com\n").unwrap();
    fs::write(
        &parent_path,
        "{{< include _child.qmd >}}\n\n[ref]: https://example.org\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", parent_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("duplicate-reference-labels"));
}

#[test]
fn test_lint_reports_unused_definitions() {
    let temp_dir = TempDir::new().unwrap();
    let doc_path = temp_dir.path().join("unused.qmd");
    fs::write(
        &doc_path,
        "Used note[^1].\n\n[^1]: Used.\n[^2]: Unused.\n\nSee [UsedLabel][].\n[UsedLabel]: https://example.com\n[UnusedLabel]: https://unused.example.com\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("unused-footnote-id"))
        .stdout(predicate::str::contains("unused-definition-label"));
}

#[test]
fn test_lint_quiet_suppresses_diagnostic_output() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n\n### Subheading\n").unwrap();

    let output = cargo_bin_cmd!("panache")
        .args([
            "lint",
            "--quiet",
            "--color",
            "never",
            test_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "--quiet should suppress diagnostic output, got: {stdout}"
    );
}

#[test]
fn test_lint_quiet_check_mode_still_signals_via_exit_code() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n\n### Subheading\n").unwrap();

    let output = cargo_bin_cmd!("panache")
        .args([
            "lint",
            "--check",
            "--quiet",
            "--color",
            "never",
            test_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "--check with violations must fail"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "--quiet --check should not print diagnostics to stdout, got: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Found"),
        "--check summary should still be on stderr even with --quiet, got: {stderr}"
    );
}

#[test]
fn test_lint_includes_cycle_reports_single_diagnostic() {
    let temp_dir = TempDir::new().unwrap();
    let parent_path = temp_dir.path().join("parent.qmd");
    let child_path = temp_dir.path().join("_child.qmd");

    fs::write(&parent_path, "{{< include _child.qmd >}}\n").unwrap();
    fs::write(&child_path, "{{< include parent.qmd >}}\n").unwrap();

    let output = cargo_bin_cmd!("panache")
        .args(["lint", "--color", "never", parent_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let occurrences = stdout.matches("[include-cycle]").count();
    assert_eq!(
        occurrences, 1,
        "expected exactly one include-cycle diagnostic header, got {occurrences}: {stdout}"
    );
}

#[test]
fn test_lint_hashpipe_yaml_parse_error_reported_once() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join(".panache.toml");
    let doc_path = temp_dir.path().join("doc.qmd");
    fs::write(
        &config_path,
        r#"flavor = "quarto"

[lint.rules]
missing-chunk-labels = false
"#,
    )
    .unwrap();
    fs::write(&doc_path, "```{r}\n#| echo: [\n1 + 1\n```\n").unwrap();

    let output = cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["lint", "--color", "never", doc_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let occurrences = stdout.matches("[yaml-parse-error]").count();
    assert_eq!(
        occurrences, 1,
        "expected yaml-parse-error to be reported exactly once, got {occurrences}: {stdout}"
    );
}

#[test]
fn test_lint_bookdown_cross_file_definitions_resolve() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("_bookdown.yml"),
        "book_filename: book\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("01-intro.Rmd"),
        "# Intro\n\n[link-target]: https://example.com\n",
    )
    .unwrap();
    let referencing = temp_dir.path().join("02-body.Rmd");
    fs::write(&referencing, "# Body\n\nSee [link-target][].\n").unwrap();

    let output = cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["lint", "--color", "never", referencing.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("undefined-reference-label"),
        "definition declared in sibling file should resolve via bookdown project: {stdout}"
    );
}

#[test]
fn test_lint_includes_missing_file_reports_diagnostic() {
    let temp_dir = TempDir::new().unwrap();
    let parent_path = temp_dir.path().join("parent.qmd");

    fs::write(&parent_path, "{{< include missing.qmd >}}\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", parent_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("include-not-found"))
        .stdout(predicate::str::contains("missing.qmd"));
}

#[test]
fn test_lint_cache_reuse_and_invalidation_on_input_change() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let cache_dir = temp_dir.path().join(".panache-cache");
    let cache_file = cache_dir.join("cli-cache-v1.bin");

    fs::write(&test_file, "# Heading\n\n### Subheading\n").unwrap();
    cargo_bin_cmd!("panache")
        .args([
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "lint",
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("heading-hierarchy"));

    assert!(cache_file.exists(), "expected cache file to be created");
    let first_modified = fs::metadata(&cache_file).unwrap().modified().unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "lint",
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("heading-hierarchy"));

    let second_modified = fs::metadata(&cache_file).unwrap().modified().unwrap();
    assert_eq!(
        first_modified, second_modified,
        "cache file should not be rewritten on a no-change rerun"
    );

    thread::sleep(Duration::from_millis(5));
    fs::write(&test_file, "# Heading\n\n## Subheading\n").unwrap();
    cargo_bin_cmd!("panache")
        .args([
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "lint",
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found"));

    let third_modified = fs::metadata(&cache_file).unwrap().modified().unwrap();
    assert!(
        third_modified > second_modified,
        "cache file should be rewritten after input fingerprint changes"
    );
}

#[test]
fn test_lint_cache_invalidation_on_included_file_change() {
    let temp_dir = TempDir::new().unwrap();
    let parent = temp_dir.path().join("parent.qmd");
    let child = temp_dir.path().join("_child.qmd");
    let cache_dir = temp_dir.path().join(".panache-cache");
    let cache_file = cache_dir.join("cli-cache-v1.bin");

    fs::write(&child, "# Heading 1\n\n### Heading 3\n").unwrap();
    fs::write(&parent, "{{< include _child.qmd >}}\n").unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "lint",
            parent.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("heading-hierarchy"));

    let first_modified = fs::metadata(&cache_file).unwrap().modified().unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "lint",
            parent.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("heading-hierarchy"));
    let second_modified = fs::metadata(&cache_file).unwrap().modified().unwrap();
    assert_eq!(first_modified, second_modified);

    thread::sleep(Duration::from_millis(5));
    fs::write(&child, "# Heading 1\n\n## Heading 2\n").unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "lint",
            parent.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found"));

    let third_modified = fs::metadata(&cache_file).unwrap().modified().unwrap();
    assert!(
        third_modified > second_modified,
        "cache file should be rewritten after included document changes"
    );
}

#[test]
fn test_lint_no_cache_skips_cache_file_creation() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let cache_dir = temp_dir.path().join(".panache-cache");
    let cache_file = cache_dir.join("cli-cache-v1.bin");

    fs::write(&test_file, "# Heading\n\n### Subheading\n").unwrap();
    cargo_bin_cmd!("panache")
        .args([
            "--no-cache",
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "lint",
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(
        !cache_file.exists(),
        "--no-cache should disable cache reads and writes"
    );
}
