//! Cache subcommand tests

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

fn workspace_namespace(path: &std::path::Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut hasher = DefaultHasher::new();
    canonical.to_string_lossy().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn expected_global_cache_base(xdg_cache_home: &Path, _home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        _home
            .join("Library")
            .join("Caches")
            .join("panache")
            .join("cli-cache")
    }

    #[cfg(not(target_os = "macos"))]
    {
        xdg_cache_home.join("panache").join("cli-cache")
    }
}

#[test]
fn test_cache_clean_removes_current_workspace_bucket() {
    let temp_dir = TempDir::new().unwrap();
    let cache_home = temp_dir.path().join("cache-home");
    let home_dir = temp_dir.path().join("home");
    let workspace = temp_dir.path().join("workspace");
    let test_file = workspace.join("test.qmd");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&cache_home).unwrap();
    fs::create_dir_all(&home_dir).unwrap();
    fs::write(&test_file, "# Heading\n\nParagraph.\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(&workspace)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("HOME", &home_dir)
        .args(["format", "--check", test_file.to_str().unwrap()])
        .assert()
        .success();

    let global_base = expected_global_cache_base(&cache_home, &home_dir);
    let cache_file = global_base
        .join(workspace_namespace(&workspace))
        .join("cli-cache-v1.bin");
    assert!(cache_file.exists(), "expected cache file to exist");

    cargo_bin_cmd!("panache")
        .current_dir(&workspace)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("HOME", &home_dir)
        .args(["cache", "clean"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed cache directory"));

    assert!(!cache_file.exists(), "expected cache file to be removed");
}

#[test]
fn test_cache_clean_all_removes_all_buckets() {
    let temp_dir = TempDir::new().unwrap();
    let cache_home = temp_dir.path().join("cache-home");
    let home_dir = temp_dir.path().join("home");
    let workspace_one = temp_dir.path().join("workspace-one");
    let workspace_two = temp_dir.path().join("workspace-two");
    fs::create_dir_all(&workspace_one).unwrap();
    fs::create_dir_all(&workspace_two).unwrap();
    fs::create_dir_all(&cache_home).unwrap();
    fs::create_dir_all(&home_dir).unwrap();
    fs::write(workspace_one.join("one.qmd"), "# One\n").unwrap();
    fs::write(workspace_two.join("two.qmd"), "# Two\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(&workspace_one)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("HOME", &home_dir)
        .args(["format", "--check", "one.qmd"])
        .assert()
        .success();

    cargo_bin_cmd!("panache")
        .current_dir(&workspace_two)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("HOME", &home_dir)
        .args(["format", "--check", "two.qmd"])
        .assert()
        .success();

    let global_base = expected_global_cache_base(&cache_home, &home_dir);
    assert!(global_base.exists(), "expected global cache base to exist");

    cargo_bin_cmd!("panache")
        .current_dir(&workspace_one)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("HOME", &home_dir)
        .args(["cache", "clean", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed all cache buckets"));

    assert!(
        !global_base.exists(),
        "expected global cache base to be removed"
    );
}

#[test]
fn test_cache_clean_uses_cache_dir_override() {
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().join("workspace");
    let cache_dir = temp_dir.path().join("custom-cache");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("doc.qmd"), "# Heading\n").unwrap();

    cargo_bin_cmd!("panache")
        .current_dir(&workspace)
        .args([
            "--cache-dir",
            cache_dir.to_str().unwrap(),
            "format",
            "--check",
            "doc.qmd",
        ])
        .assert()
        .success();

    let cache_file = cache_dir.join("cli-cache-v1.bin");
    assert!(cache_file.exists(), "expected override cache file to exist");

    cargo_bin_cmd!("panache")
        .current_dir(&workspace)
        .args(["--cache-dir", cache_dir.to_str().unwrap(), "cache", "clean"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed cache directory"));

    assert!(
        !cache_dir.exists(),
        "expected override cache directory to be removed"
    );
}
