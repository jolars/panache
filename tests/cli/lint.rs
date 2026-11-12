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
fn jupyter_markdown_lint_runs_shared_math_rules() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("math.md"), "$x^{a$\n\n$x\\\\y$\n").unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args([
            "--isolated",
            "lint",
            "--no-cache",
            "--flavor",
            "jupyter",
            "math.md",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("math-unclosed-group"))
        .stdout(predicate::str::contains("math.md:1:"))
        .stdout(predicate::str::contains("inline-math-line-break"))
        .stdout(predicate::str::contains("math.md:3:"));
}

#[test]
fn quarto_displayed_includes_keep_source_diagnostics_and_fixes() {
    if which::which("arity").is_err() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("_quarto.yml"),
        "project:\n  type: default\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("panache.toml"),
        "[linters]\nr = \"arity\"\n",
    )
    .unwrap();
    let parent = "{{< include _examples.qmd >}}\n\n```{r}\n#| label: run\nf <- function() 2\nf()\nprint(x)\n```\n";
    let child = "> ```r\n> f <- function() 1\n> x <- 1\n> any(is.na(NA))\n> ```\n";
    fs::write(dir.path().join("parent.qmd"), parent).unwrap();
    fs::write(dir.path().join("_examples.qmd"), child).unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["lint", "--no-cache", "parent.qmd"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("undefined-symbol"))
        .stdout(predicate::str::contains("parent.qmd:7:"))
        .stdout(predicate::str::contains("_examples.qmd:4:"))
        .stdout(predicate::str::contains("duplicated-function-definition").not());
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["lint", "--no-cache", "--fix", "_examples.qmd"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(dir.path().join("_examples.qmd")).unwrap(),
        child.replace("any(is.na(NA))", "anyNA(NA)")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("parent.qmd")).unwrap(),
        parent
    );
}

#[test]
fn include_execution_uses_parent_context_for_partial_targets() {
    if which::which("arity").is_err() {
        eprintln!("Skipping include R CLI test: arity is not installed");
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("_quarto.yml"),
        "project:\n  type: default\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("panache.toml"),
        "[linters]\nr = \"arity\"\n",
    )
    .unwrap();
    let partial = "```{r}\n#| label: binding\nincluded_value <- 42\n```\n";
    fs::write(dir.path().join("_binding.qmd"), partial).unwrap();
    fs::write(
        dir.path().join("parent.qmd"),
        "{{< include _binding.qmd >}}\n\n```{r}\n#| label: use\nprint(included_value)\n```\n",
    )
    .unwrap();
    for target in ["parent.qmd", "_binding.qmd", "."] {
        cargo_bin_cmd!("panache")
            .current_dir(dir.path())
            .args(["lint", "--no-cache", target])
            .assert()
            .success();
    }
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args([
            "lint",
            "--no-cache",
            "--fix",
            "--unsafe-fixes",
            "_binding.qmd",
        ])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(dir.path().join("_binding.qmd")).unwrap(),
        partial
    );
}

#[test]
fn include_execution_fixes_only_explicit_sources() {
    if which::which("arity").is_err() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("_quarto.yml"),
        "project:\n  type: default\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("panache.toml"),
        "[linters]\nr = \"arity\"\n",
    )
    .unwrap();
    let partial = "> ```{r included}\n> print(any(is.na(NA)))\n> ```\n";
    let parent = "{{< include _partial.qmd >}}\n";
    fs::write(dir.path().join("_partial.qmd"), partial).unwrap();
    fs::write(dir.path().join("parent.qmd"), parent).unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["lint", "--no-cache", "parent.qmd"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("_partial.qmd:2:"));
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["lint", "--no-cache", "--fix", "parent.qmd"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(dir.path().join("_partial.qmd")).unwrap(),
        partial
    );
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["lint", "--no-cache", "--fix", "_partial.qmd"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(dir.path().join("_partial.qmd")).unwrap(),
        partial.replace("any(is.na(NA))", "anyNA(NA)")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("parent.qmd")).unwrap(),
        parent
    );
}

#[test]
fn include_execution_shared_warning_cannot_delete_used_binding() {
    if which::which("arity").is_err() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("_quarto.yml"),
        "project:\n  type: default\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("panache.toml"),
        "[linters]\nr = \"arity\"\n",
    )
    .unwrap();
    let partial = "```{r}\n#| label: binding\nincluded_value <- 42\n```\n";
    fs::write(dir.path().join("_binding.qmd"), partial).unwrap();
    fs::write(
        dir.path().join("used.qmd"),
        "{{< include _binding.qmd >}}\n\n`r included_value`\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("unused.qmd"),
        "{{< include _binding.qmd >}}\n",
    )
    .unwrap();
    let output = cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args([
            "lint",
            "--no-cache",
            "--fix",
            "--unsafe-fixes",
            "_binding.qmd",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("unused-binding"))
        .stdout(predicate::str::contains("In execution context:"))
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8(output).unwrap().contains("unused.qmd"));
    assert_eq!(
        fs::read_to_string(dir.path().join("_binding.qmd")).unwrap(),
        partial
    );
}

#[test]
fn include_execution_nested_paths_use_main_document_directory() {
    if which::which("arity").is_err() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("parts")).unwrap();
    fs::write(
        dir.path().join("panache.toml"),
        "[linters]\nr = \"arity\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("parent.qmd"),
        "{{< include parts/_first.qmd >}}\n\n`r included_value`\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("parts/_first.qmd"),
        "{{< include \"_binding with space.qmd\" >}}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("_binding with space.qmd"),
        "```{r}\n#| label: binding\nincluded_value <- 42\n```\n",
    )
    .unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["lint", "--no-cache", "parent.qmd"])
        .assert()
        .success();
}

#[test]
fn include_execution_cache_tracks_parent_changes_and_new_parents() {
    if which::which("arity").is_err() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("_quarto.yml"),
        "project:\n  type: default\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("panache.toml"),
        "[linters]\nr = \"arity\"\n[linters.arity]\nappend-args = [\"--no-config\"]\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("_binding.qmd"),
        "```{r}\n#| label: binding\nincluded_value <- 42\n```\n",
    )
    .unwrap();
    let used = "{{< include _binding.qmd >}}\n\n`r included_value`\n";
    let unused = "{{< include _binding.qmd >}}\n";
    fs::write(dir.path().join("parent.qmd"), used).unwrap();
    let lint = || {
        let mut cmd = cargo_bin_cmd!("panache");
        cmd.current_dir(dir.path())
            .args(["lint", "--cache-dir", ".cache", "_binding.qmd"]);
        cmd
    };
    lint().assert().success();
    let cache = dir.path().join(".cache/cli-cache-v1.bin");
    let before = fs::read(&cache).unwrap();
    lint().assert().success();
    assert_eq!(fs::read(&cache).unwrap(), before);
    fs::write(dir.path().join("parent.qmd"), unused).unwrap();
    lint()
        .assert()
        .failure()
        .stdout(predicate::str::contains("unused-binding"));
    fs::write(dir.path().join("parent.qmd"), used).unwrap();
    lint().assert().success();
    fs::write(dir.path().join("another.qmd"), unused).unwrap();
    lint()
        .assert()
        .failure()
        .stdout(predicate::str::contains("another.qmd"));
    fs::remove_file(dir.path().join("another.qmd")).unwrap();
    lint().assert().success();
}

#[test]
fn include_execution_cache_rechecks_parent_linter_configuration() {
    if which::which("arity").is_err() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("partials")).unwrap();
    fs::write(
        dir.path().join("_quarto.yml"),
        "project:\n  type: default\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("panache.toml"),
        "[lint]\nselect = []\n[linters]\nr = \"arity\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("partials/panache.toml"),
        "[lint]\nselect = []\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("parent.qmd"),
        "{{< include partials/_child.md >}}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("partials/_child.md"),
        "```{r}\nprint(missing_value)\n```\n",
    )
    .unwrap();
    let config = dir.path().join("arity.toml");
    fs::write(&config, "[lint]\nignore = []\n").unwrap();
    let lint = || {
        let mut cmd = cargo_bin_cmd!("panache");
        cmd.current_dir(dir.path())
            .args(["lint", "--cache-dir", ".cache", "partials/_child.md"]);
        cmd
    };
    lint()
        .assert()
        .failure()
        .stdout(predicate::str::contains("undefined-symbol"));
    fs::write(&config, "[lint]\nignore = [\"undefined-symbol\"]\n").unwrap();
    lint().arg("--no-cache").assert().success();
    lint().assert().success();
    fs::write(&config, "[lint]\nignore = []\n").unwrap();
    lint()
        .assert()
        .failure()
        .stdout(predicate::str::contains("undefined-symbol"));
}

#[test]
fn include_execution_stdin_filename_uses_supplied_source() {
    if which::which("arity").is_err() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("panache.toml"),
        "[linters]\nr = \"arity\"\n",
    )
    .unwrap();
    let partial = "```{r}\n#| label: binding\nincluded_value <- 42\n```\n";
    fs::write(dir.path().join("_binding.qmd"), partial).unwrap();
    let disk = "`r missing_on_disk`\n";
    fs::write(dir.path().join("parent.qmd"), disk).unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["lint", "--no-cache", "--stdin-filename", "parent.qmd"])
        .write_stdin("{{< include _binding.qmd >}}\n\n`r included_value`\n")
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(dir.path().join("parent.qmd")).unwrap(),
        disk
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("_binding.qmd")).unwrap(),
        partial
    );
}

#[test]
fn include_execution_reports_unreadable_source_without_partial_analysis() {
    if which::which("arity").is_err() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("panache.toml"),
        "[linters]\nr = \"arity\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("parent.qmd"),
        "{{< include _invalid.qmd >}}\n\n`r missing`\n",
    )
    .unwrap();
    fs::write(dir.path().join("_invalid.qmd"), [0xff]).unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["lint", "--no-cache", "parent.qmd"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("include-read-error"))
        .stdout(predicate::str::contains("undefined-symbol").not());
}

#[test]
fn include_execution_uses_parent_configuration_for_direct_partial_lint() {
    if which::which("arity").is_err() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("partials")).unwrap();
    fs::write(
        dir.path().join("panache.toml"),
        "[linters]\nr = \"arity\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("partials/panache.toml"), "").unwrap();
    fs::write(
        dir.path().join("_quarto.yml"),
        "project:\n  type: default\n",
    )
    .unwrap();
    let input = "`r any(is.na(NA))`\n";
    for extension in ["md", "qmd"] {
        let partial = format!("partials/_child.{extension}");
        fs::write(
            dir.path().join("parent.qmd"),
            format!("{{{{< include {partial} >}}}}\n"),
        )
        .unwrap();
        fs::write(dir.path().join(&partial), input).unwrap();
        cargo_bin_cmd!("panache")
            .current_dir(dir.path())
            .args(["lint", "--no-cache", &partial])
            .assert()
            .failure()
            .stdout(predicate::str::contains("any-is-na"));
        cargo_bin_cmd!("panache")
            .current_dir(dir.path())
            .args(["lint", "--no-cache", "--stdin-filename", &partial])
            .write_stdin(input)
            .assert()
            .failure()
            .stdout(predicate::str::contains("any-is-na"));
    }
}

#[test]
fn inline_execution_preserves_used_binding_in_both_fix_modes() {
    if which::which("arity").is_err() {
        eprintln!("Skipping inline R CLI test: arity is not installed");
        return;
    }
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("panache.toml");
    fs::write(&config, "[linters]\nr = \"arity\"\n").unwrap();
    let path = dir.path().join("test.qmd");
    let input = "```{r}\n#| label: setup\nx <- 42\n```\n\nThe answer is `r x`.\n";
    for unsafe_fixes in [false, true] {
        fs::write(&path, input).unwrap();
        let mut cmd = cargo_bin_cmd!("panache");
        cmd.args([
            "--config",
            config.to_str().unwrap(),
            "--no-cache",
            "lint",
            "--fix",
        ]);
        if unsafe_fixes {
            cmd.arg("--unsafe-fixes");
        }
        cmd.arg(&path)
            .assert()
            .success()
            .stdout(predicate::str::contains("unused-binding").not());
        assert_eq!(fs::read_to_string(&path).unwrap(), input);
    }
}

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
        .failure()
        .stdout(predicate::str::contains("warning"))
        .stdout(predicate::str::contains("heading-hierarchy"));
}

#[test]
fn test_lint_default_exits_nonzero_on_violations() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n\n### Subheading").unwrap();

    // No `--check` flag: a violation must fail the exit code by default.
    cargo_bin_cmd!("panache")
        .args(["lint", test_file.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Found"));
}

#[test]
fn test_lint_clean_file_succeeds() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n\n## Subheading\n\nParagraph.").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", test_file.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn test_lint_check_flag_is_deprecated_noop() {
    let temp_dir = TempDir::new().unwrap();
    let clean = temp_dir.path().join("clean.qmd");
    let dirty = temp_dir.path().join("dirty.qmd");
    fs::write(&clean, "# Heading\n\n## Subheading\n\nParagraph.").unwrap();
    fs::write(&dirty, "# Heading\n\n### Subheading").unwrap();

    // `--check` still emits a deprecation warning and preserves the exit codes
    // that are now the default: success on clean input, failure on violations.
    cargo_bin_cmd!("panache")
        .args(["lint", "--check", clean.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("deprecated"));

    cargo_bin_cmd!("panache")
        .args(["lint", "--check", dirty.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("deprecated"))
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
fn test_lint_fix_skips_unfixable_diagnostics() {
    // Diagnostics without an auto-fix (e.g. missing-chunk-labels) must not
    // be reported as "Fixed" and must not silently rewrite the file.
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let original = "---\ntitle: \"Test\"\n---\n\n```{r}\nx <- 1\n```\n";
    fs::write(&test_file, original).unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "--fix", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fixed").not())
        .stdout(predicate::str::contains("missing-chunk-labels"))
        .stdout(predicate::str::contains("no auto-fix available"));

    let after = fs::read_to_string(&test_file).unwrap();
    assert_eq!(after, original, "unfixable diagnostic must not modify file");
}

#[test]
fn test_lint_fix_reports_remaining_when_some_fixed() {
    // Mixed case: heading-hierarchy is auto-fixable, missing-chunk-labels is not.
    // Expect "Fixed N ... K remaining; no auto-fix available".
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(
        &test_file,
        "# Heading\n\n### Skipped\n\n```{r}\n1 + 1\n```\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "--fix", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fixed 1 issue(s)"))
        .stdout(predicate::str::contains("remaining; no auto-fix available"));
}

#[test]
fn test_lint_fix_skips_unsafe_fixes_and_hints() {
    // `empty-values` only offers an unsafe fix; a bare --fix must not apply it
    // (the key stays) and should surface the --unsafe-fixes hint.
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    let original = "---\ntitle:\nauthor: Jane\n---\n\n# Doc\n\nBody.\n";
    fs::write(&test_file, original).unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "--fix", test_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("empty-values"))
        .stdout(predicate::str::contains("--unsafe-fixes"));

    let after = fs::read_to_string(&test_file).unwrap();
    assert_eq!(
        after, original,
        "unsafe fix must not apply under bare --fix"
    );
}

#[test]
fn punctuation_spacing_requires_unsafe_fixes() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.md");
    let original = "Words   , more ; end .\n";
    fs::write(&test_file, original).unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["lint", "--isolated", "--no-cache", "--fix", "test.md"])
        .assert()
        .success()
        .stdout(predicate::str::contains("space-before-punctuation"))
        .stdout(predicate::str::contains("--unsafe-fixes"));
    assert_eq!(fs::read_to_string(&test_file).unwrap(), original);

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args([
            "lint",
            "--isolated",
            "--no-cache",
            "--fix",
            "--unsafe-fixes",
            "test.md",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fixed 3 issue(s)"));
    assert_eq!(
        fs::read_to_string(&test_file).unwrap(),
        "Words, more; end.\n"
    );
}

#[test]
fn test_lint_unsafe_fixes_removes_empty_key() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(
        &test_file,
        "---\ntitle:\nauthor: Jane\n---\n\n# Doc\n\nBody.\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args([
            "lint",
            "--fix",
            "--unsafe-fixes",
            test_file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fixed"));

    let after = fs::read_to_string(&test_file).unwrap();
    assert!(
        !after.contains("title:"),
        "empty key should be removed: {after:?}"
    );
    assert!(
        after.contains("author: Jane"),
        "other keys preserved: {after:?}"
    );
}

#[test]
fn test_lint_unsafe_fixes_requires_fix() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "---\ntitle:\n---\n\n# Doc\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "--unsafe-fixes", test_file.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--fix"));
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
        .failure()
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
        .failure();
}

#[test]
fn test_lint_directory_resolves_cross_file_quarto_anchor() {
    // Linting a whole Quarto project must resolve anchors/references defined in
    // sibling documents (the batch shares one database per project). An anchor
    // defined in intro.qmd is referenced from body.qmd; neither
    // `undefined-anchor` nor `undefined-reference-label` should fire.
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(root.join("_quarto.yml"), "project:\n  type: book\n").unwrap();
    fs::write(root.join("intro.qmd"), "# Intro {#shared}\n").unwrap();
    fs::write(
        root.join("body.qmd"),
        "# Body\n\nSee [the intro](#shared).\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "--flavor", "quarto", root.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("undefined-anchor").not());
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
        .failure()
        .stdout(predicate::str::contains("warning"));
}

#[test]
fn test_lint_stdin_shows_source_snippet() {
    cargo_bin_cmd!("panache")
        .args(["lint", "--color", "never"])
        .write_stdin("# Heading\n\n### Subheading")
        .assert()
        .failure()
        .stdout(predicate::str::contains("--> <stdin>:3:1"))
        .stdout(predicate::str::contains("3 | ### Subheading"))
        .stdout(predicate::str::contains("^"))
        .stdout(predicate::str::contains(
            "= help: Change heading level from 3 to 2",
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
        .failure()
        .stdout(predicate::str::contains(
            "<stdin>:3:1: warning[heading-hierarchy]: Heading level skipped from h1 to h3; expected h2",
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
        .failure()
        .stdout(predicate::str::contains(
            ":3:1: warning[heading-hierarchy]: Heading level skipped",
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
    assert!(!output.status.success());
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
        .failure()
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
        .failure()
        .stdout(predicate::str::contains("heading-hierarchy"))
        .stdout(predicate::str::contains("\u{1b}["));
}

#[test]
fn test_lint_color_never_disables_ansi_diagnostics() {
    cargo_bin_cmd!("panache")
        .args(["lint", "--color", "never"])
        .write_stdin("# Heading\n\n### Subheading")
        .assert()
        .failure()
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
        .failure()
        .stdout(predicate::str::contains("missing-bibliography-key"))
        .stdout(predicate::str::contains("missing"));
}

#[test]
fn test_lint_project_manifest_bibliography_duplicate() {
    // A bibliography declared in `_quarto.yml` (not in any document) with an
    // internal duplicate key is reported once, anchored to the manifest's own
    // `bibliography:` value, when the project directory is linted.
    let temp_dir = TempDir::new().unwrap();
    let project = temp_dir.path();
    fs::create_dir(project.join("assets")).unwrap();
    fs::write(
        project.join("assets/bibliography.bib"),
        "@article{dup,\n  title = {One},\n  year = {2020}\n}\n\
         @article{dup,\n  title = {Two},\n  year = {2021}\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("_quarto.yml"),
        "project:\n  type: website\nbibliography: assets/bibliography.bib\n",
    )
    .unwrap();
    let doc_path = project.join("index.qmd");
    fs::write(&doc_path, "---\ntitle: Home\n---\n\nText.\n").unwrap();

    // Linting the directory reports the duplicate, anchored to `_quarto.yml`.
    cargo_bin_cmd!("panache")
        .args(["--no-cache", "lint", project.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("duplicate-bibliography-key"))
        .stdout(predicate::str::contains("'dup'"))
        .stdout(predicate::str::contains("_quarto.yml"));

    // Linting a single document stays quiet about the ambient project manifest.
    cargo_bin_cmd!("panache")
        .args(["--no-cache", "lint", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("duplicate-bibliography-key").not());

    // Targeting the manifest explicitly also reports it.
    cargo_bin_cmd!("panache")
        .args([
            "--no-cache",
            "lint",
            project.join("_quarto.yml").to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("duplicate-bibliography-key"));
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
        .failure()
        .stdout(predicate::str::contains("missing-bibliography-key"))
        .stdout(predicate::str::contains("missing"))
        // A single inline reference must not be flagged as a duplicate.
        .stdout(predicate::str::contains("duplicate-inline-reference-id").not());

    cargo_bin_cmd!("panache")
        .args(["lint", dup_path.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("duplicate-inline-reference-id"));
}

#[test]
fn test_lint_include_partial_diagnostics_reported_once() {
    // Regression: an include partial that is also discovered by the directory
    // walk (and is pulled in by multiple parents) must have its diagnostics
    // reported exactly once, not once per includer plus once standalone.
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // A partial with a single heading-hierarchy violation (h1 -> h3).
    fs::write(root.join("_partial.md"), "# A\n\n### B\n").unwrap();
    fs::write(
        root.join("index.qmd"),
        "---\ntitle: One\n---\n\n{{< include _partial.md >}}\n",
    )
    .unwrap();
    fs::write(
        root.join("index2.qmd"),
        "---\ntitle: Two\n---\n\n{{< include _partial.md >}}\n",
    )
    .unwrap();

    // Directory mode: the partial is a walk target *and* an include of two
    // parents (3 files scanned). Its single issue must be reported once, not
    // three times. The summary is emitted on stderr.
    cargo_bin_cmd!("panache")
        .current_dir(root)
        .args(["lint", "--no-cache", "--color", "never", "."])
        .assert()
        .failure()
        .stdout(predicate::str::contains("heading-hierarchy"))
        .stderr(predicate::str::contains(
            "Found 1 issue(s) across 3 file(s)",
        ));

    // Explicit multi-file mode where the partial is not itself a target but is
    // included by both parents: still reported once.
    cargo_bin_cmd!("panache")
        .current_dir(root)
        .args([
            "lint",
            "--no-cache",
            "--color",
            "never",
            "index.qmd",
            "index2.qmd",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("heading-hierarchy"))
        .stderr(predicate::str::contains(
            "Found 1 issue(s) across 2 file(s)",
        ));
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
        .failure()
        .stdout(predicate::str::contains("yaml-parse-error"))
        .stdout(predicate::str::contains("YAML parse error"));
}

#[test]
fn test_lint_reports_frontmatter_yaml_parse_error() {
    // Regression: a frontmatter key with an empty inline value whose
    // continuation is NOT indented past the key. pandoc/libyaml reject
    // this ("could not find expected ':'"); panache previously accepted
    // the unindented lines as a multi-line plain scalar value.
    let temp_dir = TempDir::new().unwrap();
    let doc_path = temp_dir.path().join("doc.md");
    fs::write(
        &doc_path,
        "---\ntitle: Demo\ndescription:\nBasin is my new Rust library\n  with pluggable backends\n---\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["lint", "--color", "never", doc_path.to_str().unwrap()])
        .assert()
        .failure()
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
        .failure()
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
        .failure()
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
        .failure()
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
        .failure()
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
        .failure()
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
        .failure()
        .stdout(predicate::str::contains("_child.qmd"))
        .stdout(predicate::str::contains("heading-hierarchy"));
}

#[test]
fn test_lint_resolves_anchors_between_excluded_quarto_includes() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(
        root.join("_quarto.yml"),
        "project:\n  render:\n    - '*.qmd'\n    - '!proposal/'\n",
    )
    .unwrap();
    fs::create_dir(root.join("proposal")).unwrap();
    fs::write(
        root.join("main.qmd"),
        "{{< include proposal/timeline.qmd >}}\n\n{{< include proposal/success.qmd >}}\n",
    )
    .unwrap();
    fs::write(
        root.join("proposal/timeline.qmd"),
        "See [Definition of Done](#definition-of-done).\n",
    )
    .unwrap();
    fs::write(root.join("proposal/success.qmd"), "# Definition of Done\n").unwrap();

    for target in [".", "main.qmd", "proposal/timeline.qmd"] {
        cargo_bin_cmd!("panache")
            .current_dir(root)
            .args(["lint", "--no-cache", target])
            .assert()
            .success()
            .stdout(predicate::str::contains("undefined-anchor").not());
    }

    fs::write(root.join("proposal/unrelated.qmd"), "# Missing\n").unwrap();
    fs::write(
        root.join("proposal/timeline.qmd"),
        "See [missing](#missing).\n",
    )
    .unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(root)
        .args(["lint", "--no-cache", "proposal/timeline.qmd"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("[undefined-anchor]"))
        .stdout(predicate::str::contains("Anchor '#missing'"));
}

#[test]
fn test_lint_validates_custom_crossrefs_across_quarto_project() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(root.join("_quarto.yml"), "project:\n  type: book\n").unwrap();
    fs::write(
        root.join("panache.toml"),
        "crossref-prefixes = [\"qdr\"]\n\n[lint]\nselect = [\"undefined-references\", \"missing-bibliography-key\"]\n",
    )
    .unwrap();
    fs::write(root.join("body.qmd"), "See @qdr-example.\n").unwrap();

    for target in [
        "::: {#qdr-example}\nContent\n:::\n",
        "```{r}\n#| label: qdr-example\n1\n```\n",
    ] {
        fs::write(root.join("defs.qmd"), target).unwrap();
        cargo_bin_cmd!("panache")
            .current_dir(root)
            .args(["lint", "--no-cache", "body.qmd"])
            .assert()
            .success();

        fs::write(
            root.join("defs.qmd"),
            target.replace("qdr-example", "qdr-other"),
        )
        .unwrap();
        cargo_bin_cmd!("panache")
            .current_dir(root)
            .args(["lint", "--no-cache", "body.qmd"])
            .assert()
            .failure()
            .stdout(predicate::str::contains("undefined-reference-label"))
            .stdout(predicate::str::contains(
                "Cross-reference label '@qdr-example'",
            ))
            .stdout(predicate::str::contains("body.qmd:1:"))
            .stdout(predicate::str::contains("missing-bibliography-key").not());
    }
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
        .failure()
        .stdout(predicate::str::contains("duplicate-reference-labels"));
}

#[test]
fn test_lint_quarto_siblings_do_not_collide_on_footnote_ids() {
    // Two independently-rendered Quarto documents each defining `[^1]` are not a
    // conflict: Quarto renders each `.qmd` as its own Pandoc pass, so footnote
    // labels are file-scoped. Only merged units (includes, bookdown) collide.
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(root.join("_quarto.yml"), "project:\n  type: website\n").unwrap();
    let first = root.join("first.qmd");
    let second = root.join("second.qmd");
    fs::write(&first, "Text.[^1]\n\n[^1]: First note.\n").unwrap();
    fs::write(&second, "More.[^1]\n\n[^1]: Second note.\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "--no-cache", second.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("duplicate-reference-labels").not());
}

#[test]
fn test_lint_reports_unused_definitions() {
    let temp_dir = TempDir::new().unwrap();
    let doc_path = temp_dir.path().join("unused.qmd");
    fs::write(
        &doc_path,
        "Used note[^1].\n\n[^1]: Used.\n[^2]: Unused.\n\nSee [UsedLabel][].\n\n[UsedLabel]: https://example.com\n[UnusedLabel]: https://unused.example.com\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", doc_path.to_str().unwrap()])
        .assert()
        .failure()
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
    assert!(!output.status.success());
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
    assert!(!output.status.success());
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
    assert!(!output.status.success());
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
        .failure()
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
        .failure()
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
        .failure()
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
        .failure()
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
        .failure()
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
        .failure();

    assert!(
        !cache_file.exists(),
        "--no-cache should disable cache reads and writes"
    );
}

#[test]
fn test_lint_dash_reads_stdin() {
    cargo_bin_cmd!("panache")
        .args(["lint", "-"])
        .write_stdin("# Heading\n\n### Subheading")
        .assert()
        .failure()
        .stdout(predicate::str::contains("warning"))
        .stdout(predicate::str::contains("heading-hierarchy"));
}

#[test]
fn test_lint_dash_with_fix_writes_to_stdout() {
    cargo_bin_cmd!("panache")
        .args(["lint", "--fix", "-"])
        .write_stdin("# Heading\n\n### Subheading")
        .assert()
        .success()
        .stdout(predicate::str::contains("## Subheading"));
}

#[test]
fn test_lint_dash_mixed_with_path_errors() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.qmd");
    fs::write(&test_file, "# Heading\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "-", test_file.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "'-' (stdin) cannot be combined with file path arguments",
        ));
}

#[test]
fn test_lint_dot_config_exclude_anchors_at_project_root_off_cwd() {
    // A `.config/panache.toml` exclude anchors at the project root even when
    // the process cwd is elsewhere.
    let project = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    fs::create_dir_all(project.path().join(".git")).unwrap();
    fs::create_dir_all(project.path().join(".config")).unwrap();
    fs::write(
        project.path().join(".config").join("panache.toml"),
        "exclude = [\"tests/\"]\n",
    )
    .unwrap();
    fs::write(project.path().join("doc.qmd"), "# Heading\n\n## Sub\n").unwrap();
    let excluded_dir = project.path().join("tests");
    fs::create_dir_all(&excluded_dir).unwrap();
    fs::write(
        excluded_dir.join("snapshot.md"),
        "# Heading\n\n### Skipped\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(elsewhere.path())
        .args(["lint", project.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found in 1 file(s)"));
}

#[test]
fn test_lint_explicit_config_exclude_anchors_at_config_dir() {
    // `--config <dir>/panache.toml` excludes resolve relative to <dir>, not cwd.
    let project = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    let config = project.path().join("panache.toml");
    fs::write(&config, "exclude = [\"tests/\"]\n").unwrap();
    fs::write(project.path().join("doc.qmd"), "# Heading\n\n## Sub\n").unwrap();
    let excluded_dir = project.path().join("tests");
    fs::create_dir_all(&excluded_dir).unwrap();
    fs::write(
        excluded_dir.join("snapshot.md"),
        "# Heading\n\n### Skipped\n",
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(elsewhere.path())
        .args([
            "lint",
            "--config",
            config.to_str().unwrap(),
            project.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found in 1 file(s)"));
}

#[test]
fn test_lint_quarto_yml_schema_violation() {
    let temp_dir = TempDir::new().unwrap();
    let manifest = temp_dir.path().join("_quarto.yml");
    // `render` is an array in project-config; a boolean is a type mismatch, which
    // rides the on-by-default `quarto-schema` rule.
    fs::write(&manifest, "project:\n  type: website\n  render: true\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", manifest.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("quarto-schema-type-mismatch"));
}

#[test]
fn test_lint_quarto_yml_unknown_key_is_opt_in() {
    let temp_dir = TempDir::new().unwrap();
    let manifest = temp_dir.path().join("_quarto.yml");
    fs::write(&manifest, "forrmat: html\n").unwrap();

    // Off by default: a key typo Quarto itself tolerates is not flagged.
    cargo_bin_cmd!("panache")
        .args(["lint", manifest.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found"));

    // Opt in via config: the typo is now reported.
    let config = temp_dir.path().join("panache.toml");
    fs::write(&config, "[lint.rules]\nquarto-schema-unknown-key = true\n").unwrap();
    cargo_bin_cmd!("panache")
        .args([
            "lint",
            "--config",
            config.to_str().unwrap(),
            manifest.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("quarto-schema-unknown-key"))
        .stdout(predicate::str::contains("forrmat"));
}

#[test]
fn test_lint_quarto_yml_clean() {
    let temp_dir = TempDir::new().unwrap();
    let manifest = temp_dir.path().join("_quarto.yml");
    fs::write(&manifest, "project:\n  type: website\nformat: html\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", manifest.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found"));
}

#[test]
fn test_lint_quarto_yml_check_mode_fails() {
    let temp_dir = TempDir::new().unwrap();
    let manifest = temp_dir.path().join("_quarto.yml");
    // A type mismatch (on by default) so `--check` fails without opting into
    // the unknown-key rule.
    fs::write(&manifest, "project:\n  render: true\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "--check", manifest.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn test_lint_quarto_yml_schema_skipped_under_explicit_pandoc_flavor() {
    // The manifest filename detects as Quarto, but an explicit `--flavor pandoc`
    // overrides that (as it would for a `.qmd`), so the schema half is skipped.
    let temp_dir = TempDir::new().unwrap();
    let manifest = temp_dir.path().join("_quarto.yml");
    fs::write(&manifest, "forrmat: html\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", "--flavor", "pandoc", manifest.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found"));
}

#[test]
fn test_lint_metadata_yml_validates_against_front_matter() {
    let temp_dir = TempDir::new().unwrap();
    let manifest = temp_dir.path().join("_metadata.yml");
    // `toc` is a boolean in the front-matter schema; `_metadata.yml` is directory
    // metadata, so it validates against `front-matter`, not `project-config`.
    fs::write(&manifest, "toc: maybe\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["lint", manifest.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("quarto-schema-type-mismatch"));
}

#[test]
fn test_lint_discovers_parent_config_from_relative_target() {
    // Issue #441: `panache lint .` inside a subdirectory must discover a
    // `panache.toml` in an ancestor directory. Discovery walks
    // `start_dir.ancestors()`, which for a relative start dir never climbs
    // above the working directory.
    let temp_dir = TempDir::new().unwrap();
    fs::create_dir(temp_dir.path().join(".git")).unwrap();
    fs::write(
        temp_dir.path().join("panache.toml"),
        "[lint.rules]\nheading-hierarchy = false\n",
    )
    .unwrap();
    let subdir = temp_dir.path().join("subdir");
    fs::create_dir(&subdir).unwrap();
    fs::write(subdir.join("doc.md"), "# Title\n\n### Skipped level\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(&subdir)
        .args(["lint", "--no-cache", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found"));

    // A bare relative filename yields an empty start dir; it must resolve to
    // the working directory and walk up from there.
    cargo_bin_cmd!("panache")
        .current_dir(&subdir)
        .args(["lint", "--no-cache", "doc.md"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found"));
}

#[test]
fn deprecated_lint_rules_keep_behavior_and_warn_with_removal_date() {
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("panache.toml");
    fs::write(&config, "[lint.rules]\nheading-hierarchy = false\n").unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["--config", config.to_str().unwrap(), "lint"])
        .write_stdin("# Title\n\n### Skipped level\n")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`[lint.rules]` boolean tables are deprecated",
        ))
        .stderr(predicate::str::contains(config.to_str().unwrap()))
        .stderr(predicate::str::contains("extend-select"))
        .stderr(predicate::str::contains("2027-04-12"));
}

#[test]
fn inherited_lint_rules_warn_with_the_declaring_path() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("base.toml");
    fs::write(&base, "[lint.rules]\nheading-hierarchy = false\n").unwrap();
    fs::write(dir.path().join("panache.toml"), "extend = \"base.toml\"\n").unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["lint"])
        .write_stdin("# Title\n\n### Skipped level\n")
        .assert()
        .success()
        .stderr(predicate::str::contains(base.to_str().unwrap()));
}

#[test]
fn lint_cli_selects_only_requested_rules_for_stdin_and_files() {
    let dir = TempDir::new().unwrap();
    let input = "# Title\n\n### Skipped level\n\nSee [missing][target].\n";
    fs::write(dir.path().join("doc.md"), input).unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["--isolated", "lint", "--select", "undefined-references"])
        .write_stdin(input)
        .assert()
        .failure()
        .stdout(predicate::str::contains("undefined-reference-label"))
        .stdout(predicate::str::contains(
            "[lint] ignore = [\"undefined-references\"]",
        ))
        .stdout(predicate::str::contains("heading-hierarchy").not())
        .stderr(predicate::str::contains("deprecated").not());
    for target in ["doc.md", "."] {
        cargo_bin_cmd!("panache")
            .current_dir(dir.path())
            .args([
                "--isolated",
                "lint",
                "--select",
                "undefined-references",
                target,
            ])
            .assert()
            .failure()
            .stdout(predicate::str::contains("undefined-reference-label"))
            .stdout(predicate::str::contains("heading-hierarchy").not());
    }
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args([
            "--isolated",
            "lint",
            "--select",
            "heading-hierarchy,undefined-references",
        ])
        .write_stdin(input)
        .assert()
        .failure()
        .stdout(predicate::str::contains("undefined-reference-label"))
        .stdout(predicate::str::contains("heading-hierarchy"));
}

#[test]
fn lint_cli_overrides_selection_adds_rules_and_replaces_ignores() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("panache.toml"),
        "[lint]\nselect = []\nignore = [\"heading-hierarchy\"]\n",
    )
    .unwrap();
    let input = "# Title\n\n### Skipped level\n";
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["lint", "--extend-select", "heading-hierarchy"])
        .write_stdin(input)
        .assert()
        .success();
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args([
            "lint",
            "--extend-select",
            "heading-hierarchy",
            "--ignore",
            "",
        ])
        .write_stdin(input)
        .assert()
        .failure()
        .stdout(predicate::str::contains("heading-hierarchy"));
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args([
            "lint",
            "--select",
            "heading-hierarchy",
            "--ignore",
            "undefined-references",
        ])
        .write_stdin(input)
        .assert()
        .failure()
        .stdout(predicate::str::contains("heading-hierarchy"));
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["--isolated", "lint", "--select", ""])
        .write_stdin(input)
        .assert()
        .success();
}

#[test]
fn lint_cli_selection_changes_invalidate_cached_diagnostics() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("doc.md"), "# Title\n\n### Skipped level\n").unwrap();
    let cache = dir.path().join("cache");
    for (select, reports_heading) in [
        ("heading-hierarchy", true),
        ("", false),
        ("heading-hierarchy", true),
    ] {
        let assertion = cargo_bin_cmd!("panache")
            .current_dir(dir.path())
            .args([
                "--isolated",
                "--cache-dir",
                cache.to_str().unwrap(),
                "lint",
                "--select",
                select,
                "doc.md",
            ])
            .assert();
        if reports_heading {
            assertion
                .failure()
                .stdout(predicate::str::contains("heading-hierarchy"));
        } else {
            assertion
                .success()
                .stdout(predicate::str::contains("heading-hierarchy").not());
        }
    }
}

#[test]
fn lint_cli_selection_applies_to_explicit_quarto_manifests() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("_quarto.yml"),
        "project:\n  type: website\n  render: true\n",
    )
    .unwrap();
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args(["--isolated", "lint", "_quarto.yml"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("quarto-schema"));
    cargo_bin_cmd!("panache")
        .current_dir(dir.path())
        .args([
            "--isolated",
            "lint",
            "--select",
            "heading-hierarchy",
            "_quarto.yml",
        ])
        .assert()
        .success();
}

#[test]
fn ruff_project_config_changes_refresh_cached_diagnostics() {
    if which::which("ruff").is_err() {
        return;
    }
    let project = TempDir::new().unwrap();
    let caller = TempDir::new().unwrap();
    let nested = project.path().join("notebooks");
    fs::create_dir(&nested).unwrap();
    fs::write(
        project.path().join("panache.toml"),
        "[lint]\nselect = []\n[linters]\npython = \"ruff\"\n",
    )
    .unwrap();
    let config = project.path().join("ruff.toml");
    for filename in ["doc.md", "doc.qmd"] {
        let document = nested.join(filename);
        fs::write(&document, "```python\nimport os\n```\n").unwrap();
        let run = || {
            let mut command = cargo_bin_cmd!("panache");
            command
                .current_dir(caller.path())
                .args(["lint", "--cache-dir"])
                .arg(project.path().join("cache"))
                .arg(&document);
            command
        };
        fs::write(&config, "[lint]\nignore = [\"F401\"]\n").unwrap();
        run().assert().success();
        fs::write(&config, "[lint]\nignore = []\n").unwrap();
        run()
            .arg("--no-cache")
            .assert()
            .failure()
            .stdout(predicate::str::contains("F401"));
        run()
            .assert()
            .failure()
            .stdout(predicate::str::contains("F401"));
    }
}

#[test]
fn eslint_config_override_changes_refresh_cached_diagnostics() {
    if which::which("eslint").is_err() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("eslint.config.mjs");
    fs::write(
        dir.path().join("panache.toml"),
        format!(
            "[lint]\nselect = []\n[linters]\njs = \"eslint\"\n[linters.eslint]\nappend-args = [\"--config\", '{}']\n",
            config.display()
        ),
    )
    .unwrap();
    for filename in ["doc.md", "doc.qmd"] {
        fs::write(dir.path().join(filename), "```js\nmissing_value;\n```\n").unwrap();
        let run = || {
            let mut command = cargo_bin_cmd!("panache");
            command
                .current_dir(dir.path())
                .args(["lint", "--cache-dir", "cache", filename]);
            command
        };
        fs::write(&config, "export default [{rules: {'no-undef': 'off'}}];\n").unwrap();
        run().assert().success();
        fs::write(
            &config,
            "export default [{rules: {'no-undef': 'error'}}];\n",
        )
        .unwrap();
        run()
            .arg("--no-cache")
            .assert()
            .failure()
            .stdout(predicate::str::contains("no-undef"));
        run()
            .assert()
            .failure()
            .stdout(predicate::str::contains("no-undef"));
    }
}

#[test]
fn arity_uses_notebook_project_config_and_allows_preset_overrides() {
    if which::which("arity").is_err() {
        return;
    }
    let project = TempDir::new().unwrap();
    let caller = TempDir::new().unwrap();
    let nested = project.path().join("notebooks");
    fs::create_dir(&nested).unwrap();
    let document = nested.join("analysis.qmd");
    fs::write(&document, "```{r}\nprint(missing_value)\n```\n").unwrap();
    fs::write(
        project.path().join("panache.toml"),
        "[lint]\nselect = []\n[linters]\nr = \"arity\"\n",
    )
    .unwrap();
    fs::write(
        project.path().join("arity.toml"),
        "[lint]\nignore = [\"undefined-symbol\"]\n",
    )
    .unwrap();
    let run = || {
        let mut command = cargo_bin_cmd!("panache");
        command
            .current_dir(caller.path())
            .arg("lint")
            .arg(&document);
        command
    };
    run().assert().success();
    fs::write(project.path().join("arity.toml"), "[lint]\nignore = []\n").unwrap();
    run()
        .assert()
        .failure()
        .stdout(predicate::str::contains("undefined-symbol"));
    fs::write(
        project.path().join("arity.toml"),
        "[lint]\nignore = [\"undefined-symbol\"]\n",
    )
    .unwrap();

    fs::write(project.path().join("panache.toml"), "[lint]\nselect = []\n[linters]\nr = \"arity\"\n[linters.arity]\nargs = [\"lint\", \"--no-config\", \"--output\", \"json\"]\n").unwrap();
    run()
        .assert()
        .failure()
        .stdout(predicate::str::contains("undefined-symbol"));
    fs::write(project.path().join("panache.toml"), "[lint]\nselect = []\n[linters]\nr = \"arity\"\n[linters.arity]\nappend-args = [\"--ignore\", \"undefined-symbol\"]\n").unwrap();
    fs::write(project.path().join("arity.toml"), "[lint]\nignore = []\n").unwrap();
    run().assert().success();
}

#[cfg(unix)]
#[test]
fn linter_command_and_argument_overrides_reach_the_process() {
    let dir = TempDir::new().unwrap();
    let executable = dir.path().join("custom linter");
    fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$@\" > \"$PANACHE_TEST_LINTER_LOG\"\nprintf '[]\\n'\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let config = r#"
[lint]
select = []
[linters]
r = "arity"
[linters.arity]
cmd = './custom linter'
args = ["lint", "--output", "json"]
prepend-args = ["--prefix"]
append-args = ["--suffix"]
"#;
    fs::write(dir.path().join("panache.toml"), config).unwrap();
    let caller = TempDir::new().unwrap();
    for filename in ["doc.md", "doc.qmd"] {
        let document = dir.path().join(filename);
        fs::write(&document, "```r\nprint(x)\n```\n").unwrap();
        let log = dir.path().join("invocation.txt");
        for stdin in [false, true] {
            let mut cmd = cargo_bin_cmd!("panache");
            cmd.current_dir(caller.path())
                .env("PANACHE_TEST_LINTER_LOG", &log)
                .args(["lint", "--no-cache"]);
            if stdin {
                cmd.arg("--stdin-filename")
                    .arg(&document)
                    .write_stdin("```r\nprint(x)\n```\n");
            } else {
                cmd.arg(&document);
            }
            cmd.assert().success();
            let invocation = fs::read_to_string(&log).unwrap();
            let lines: Vec<_> = invocation.lines().collect();
            assert_eq!(
                fs::canonicalize(lines[0]).unwrap(),
                fs::canonicalize(dir.path()).unwrap()
            );
            assert_eq!(
                &lines[1..6],
                &["--prefix", "lint", "--output", "json", "--suffix"]
            );
            assert!(lines[6].ends_with("/input.R"));
            fs::remove_file(&log).unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn pathless_stdin_runs_relative_linter_commands_in_isolated_directories() {
    let dir = TempDir::new().unwrap();
    let executable = dir.path().join("node_modules/.bin/custom linter");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    let log = dir.path().join("invocation.txt");
    let cases = [
        (
            "eslint",
            "js",
            "const unused = 1;",
            r#"[{"messages":[{"ruleId":"relative-command","severity":2,"message":"Relative executable ran","line":2,"column":1}]}]"#,
        ),
        (
            "clippy",
            "rust",
            "fn main() {}",
            r#"{"$message_type":"diagnostic","message":"Relative executable ran","level":"error","code":{"code":"relative-command"},"spans":[{"line_start":2,"column_start":1,"line_end":2,"column_end":2,"is_primary":true}]}"#,
        ),
    ];
    for (linter, language, code, output) in cases {
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$PWD\" > \"$PANACHE_TEST_LINTER_LOG\"\nprintf '%s\\n' '{output}'\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(
            dir.path().join("panache.toml"),
            format!(
                "[lint]\nselect = []\n[linters]\n{language} = '{linter}'\n[linters.{linter}]\ncmd = './node_modules/.bin/custom linter'\n"
            ),
        )
        .unwrap();
        for explicit_stdin in [false, true] {
            let mut command = cargo_bin_cmd!("panache");
            command
                .current_dir(dir.path())
                .env("PANACHE_TEST_LINTER_LOG", &log)
                .args(["lint", "--no-cache"]);
            if explicit_stdin {
                command.arg("-");
            }
            command
                .write_stdin(format!("```{language}\n{code}\n```\n"))
                .assert()
                .failure()
                .stdout(predicate::str::contains("relative-command"));
            let working_dir = fs::read_to_string(&log).unwrap();
            assert_ne!(std::path::Path::new(working_dir.trim()), dir.path());
            fs::remove_file(&log).unwrap();
        }
    }
}

#[test]
fn stdin_filename_uses_linter_project_configuration() {
    if which::which("arity").is_err() {
        return;
    }
    let project = TempDir::new().unwrap();
    let caller = TempDir::new().unwrap();
    fs::write(
        project.path().join("panache.toml"),
        "[lint]\nselect = []\n[linters]\nr = \"arity\"\n",
    )
    .unwrap();
    fs::write(caller.path().join("arity.toml"), "[lint]\nignore = []\n").unwrap();
    let source = "```r\nprint(missing_value)\n```\n";
    for filename in ["doc.md", "doc.qmd"] {
        let document = project.path().join(filename);
        let run = || {
            let mut cmd = cargo_bin_cmd!("panache");
            cmd.current_dir(caller.path())
                .args(["lint", "--stdin-filename"])
                .arg(&document)
                .write_stdin(source);
            cmd
        };
        fs::write(
            project.path().join("arity.toml"),
            "[lint]\nignore = [\"undefined-symbol\"]\n",
        )
        .unwrap();
        run().assert().success();
        fs::write(project.path().join("arity.toml"), "[lint]\nignore = []\n").unwrap();
        run()
            .assert()
            .failure()
            .stdout(predicate::str::contains("undefined-symbol"));
        fs::write(
            project.path().join("panache.toml"),
            "[lint]\nselect = []\n[linters]\nr = \"arity\"\n[linters.arity]\nappend-args = [\"--config\", \"./lint-config.toml\"]\n",
        )
        .unwrap();
        fs::write(
            project.path().join("lint-config.toml"),
            "[lint]\nignore = [\"undefined-symbol\"]\n",
        )
        .unwrap();
        run().assert().success();
        fs::write(
            project.path().join("lint-config.toml"),
            "[lint]\nignore = []\n",
        )
        .unwrap();
        run()
            .assert()
            .failure()
            .stdout(predicate::str::contains("undefined-symbol"));
        fs::write(
            project.path().join("panache.toml"),
            "[lint]\nselect = []\n[linters]\nr = \"arity\"\n",
        )
        .unwrap();
    }
}
