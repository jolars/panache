//! Translate subcommand tests

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_translate_requires_provider() {
    cargo_bin_cmd!("panache")
        .arg("translate")
        .arg("--isolated")
        .args(["--target-lang", "fr"])
        .write_stdin("# Heading\n\nParagraph.")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Translation provider not configured",
        ));
}

#[test]
fn test_translate_requires_target_language() {
    cargo_bin_cmd!("panache")
        .arg("translate")
        .arg("--isolated")
        .args(["--provider", "libretranslate"])
        .write_stdin("# Heading\n\nParagraph.")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Target language not configured"));
}

#[test]
fn test_translate_uses_config_for_provider_and_target() {
    let temp_dir = TempDir::new().unwrap();
    let config_file = temp_dir.path().join(".panache.toml");
    fs::write(
        &config_file,
        r#"
        [translate]
        provider = "libretranslate"
        target-lang = "fr"
        endpoint = "http://127.0.0.1:1/translate"
    "#,
    )
    .unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .arg("translate")
        .write_stdin("# Heading\n\nParagraph.")
        .assert()
        .failure()
        .stderr(predicate::str::contains("LibreTranslate request failed"));
}

#[test]
fn test_translate_force_exclude_noops_when_all_filtered() {
    let temp_dir = TempDir::new().unwrap();
    let config = temp_dir.path().join(".panache.toml");
    let excluded_dir = temp_dir.path().join("tests");
    let excluded = excluded_dir.join("snapshot.md");
    fs::create_dir_all(&excluded_dir).unwrap();
    fs::write(
        &config,
        r#"
exclude = ["tests/"]
[translate]
provider = "libretranslate"
target-lang = "fr"
endpoint = "http://127.0.0.1:1/translate"
"#,
    )
    .unwrap();
    fs::write(&excluded, "# Excluded\n\nParagraph.\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args(["translate", "--force-exclude", excluded.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn test_translate_directory_with_no_supported_files_is_noop() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("note.txt");
    fs::write(&test_file, "content\n").unwrap();

    cargo_bin_cmd!("panache")
        .args(["translate", temp_dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("No supported files found"));
}

#[test]
fn test_translate_two_file_args_writes_to_output_file() {
    let temp_dir = TempDir::new().unwrap();
    let input = temp_dir.path().join("in.qmd");
    let output = temp_dir.path().join("out.qmd");
    let config_file = temp_dir.path().join(".panache.toml");

    fs::write(
        &config_file,
        r#"
        [translate]
        provider = "libretranslate"
        target-lang = "fr"
        endpoint = "http://127.0.0.1:1/translate"
    "#,
    )
    .unwrap();
    fs::write(&input, "# Heading\n\nParagraph.\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(temp_dir.path())
        .args([
            "translate",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("LibreTranslate request failed"));

    assert!(
        !output.exists(),
        "output file should not be created when translation fails"
    );
}
