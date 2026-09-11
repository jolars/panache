//! Configuration fallback tests use child-process environments so they remain
//! independent of other tests and the developer's own configuration.

use assert_cmd::{Command, cargo::cargo_bin_cmd};
use predicates::prelude::*;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

struct ConfigProject {
    dir: TempDir,
    project: PathBuf,
    env_config: PathBuf,
}

impl ConfigProject {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("project");
        fs::create_dir_all(project.join(".git")).unwrap();
        let global = dir.path().join("panache");
        fs::create_dir(&global).unwrap();
        fs::write(global.join("config.toml"), "[format]\nwrap = \"reflow\"\n").unwrap();
        let env_config = dir.path().join("shared.toml");
        fs::write(&env_config, "[format]\nwrap = \"preserve\"\n").unwrap();
        Self {
            dir,
            project,
            env_config,
        }
    }

    fn command(&self) -> Command {
        let mut cmd = cargo_bin_cmd!("panache");
        cmd.current_dir(&self.project)
            .env("PANACHE_CONFIG", &self.env_config)
            .env("XDG_CONFIG_HOME", self.dir.path())
            .args(["format", "--no-cache"]);
        cmd
    }
}

#[test]
fn panache_config_overrides_global_config() {
    let project = ConfigProject::new();
    project
        .command()
        .write_stdin("Alpha\nbravo.\n")
        .assert()
        .success()
        .stdout("Alpha\nbravo.\n");
}

#[test]
fn project_config_overrides_panache_config_without_merging() {
    let project = ConfigProject::new();
    fs::write(project.project.join("panache.toml"), "").unwrap();
    project
        .command()
        .write_stdin("Alpha\nbravo.\n")
        .assert()
        .success()
        .stdout("Alpha bravo.\n");
}

#[test]
fn explicit_config_overrides_missing_panache_config() {
    let project = ConfigProject::new();
    fs::remove_file(&project.env_config).unwrap();
    let explicit = project.dir.path().join("explicit.toml");
    fs::write(&explicit, "[format]\nwrap = \"preserve\"\n").unwrap();
    project
        .command()
        .arg("--config")
        .arg(explicit)
        .write_stdin("Alpha\nbravo.\n")
        .assert()
        .success()
        .stdout("Alpha\nbravo.\n");
}

#[test]
fn isolated_ignores_missing_panache_config() {
    let project = ConfigProject::new();
    fs::remove_file(&project.env_config).unwrap();
    project
        .command()
        .arg("--isolated")
        .write_stdin("Alpha\nbravo.\n")
        .assert()
        .success()
        .stdout("Alpha bravo.\n");
}

#[test]
fn missing_panache_config_is_an_error() {
    let project = ConfigProject::new();
    fs::remove_file(&project.env_config).unwrap();
    project
        .command()
        .write_stdin("Alpha\nbravo.\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("shared.toml"));
}

#[test]
fn malformed_panache_config_is_an_error() {
    let project = ConfigProject::new();
    fs::write(&project.env_config, "[format]\nwrpa = \"preserve\"\n").unwrap();
    project
        .command()
        .write_stdin("Alpha\nbravo.\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("shared.toml"))
        .stderr(predicate::str::contains("wrpa"));
}

#[test]
fn empty_panache_config_uses_global_config() {
    let project = ConfigProject::new();
    fs::write(
        project.dir.path().join("panache/config.toml"),
        "[format]\nwrap = \"preserve\"\n",
    )
    .unwrap();
    project
        .command()
        .env("PANACHE_CONFIG", "")
        .write_stdin("Alpha\nbravo.\n")
        .assert()
        .success()
        .stdout("Alpha\nbravo.\n");
}

#[test]
fn panache_config_extends_relative_to_its_own_directory() {
    let project = ConfigProject::new();
    fs::write(
        project.dir.path().join("base.toml"),
        "[format]\nwrap = \"preserve\"\n",
    )
    .unwrap();
    fs::write(&project.env_config, "extend = \"base.toml\"\n").unwrap();
    project
        .command()
        .env("PANACHE_CONFIG", "../shared.toml")
        .write_stdin("Alpha\nbravo.\n")
        .assert()
        .success()
        .stdout("Alpha\nbravo.\n");
}

#[test]
fn panache_config_patterns_anchor_at_traversal_directory() {
    let project = ConfigProject::new();
    fs::write(
        &project.env_config,
        "include = [\"docs/**\"]\nexclude = [\"docs/vendor/**\"]\n",
    )
    .unwrap();
    let docs = project.project.join("docs");
    fs::create_dir_all(docs.join("vendor")).unwrap();
    fs::write(docs.join("intro.md"), "# Included\n").unwrap();
    fs::write(docs.join("vendor/third-party.md"), "#   Excluded\n").unwrap();
    project
        .command()
        .args(["--check", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("intro.md is correctly formatted"))
        .stdout(predicate::str::contains("third-party.md").not());
}
