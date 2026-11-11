use assert_cmd::{Command, cargo::cargo_bin_cmd};
use panache::config::{Extensions, FormatterExtensions};
use predicates::prelude::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    dir
}

fn command(dir: &TempDir) -> Command {
    let mut cmd = cargo_bin_cmd!("panache");
    cmd.current_dir(dir.path())
        .env_remove("PANACHE_CONFIG")
        .env("XDG_CONFIG_HOME", dir.path().join("user-config"))
        .arg("inspect");
    cmd
}

fn output(cmd: &mut Command) -> String {
    String::from_utf8(cmd.assert().success().get_output().stdout.clone()).unwrap()
}

fn rows(output: &str) -> Vec<(&str, bool, bool)> {
    output
        .lines()
        .skip_while(|line| !line.starts_with("Extension "))
        .skip(1)
        .map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 3, "unexpected extension row: {line}");
            (
                fields[0],
                fields[1].parse().unwrap(),
                fields[2].parse().unwrap(),
            )
        })
        .collect()
}

fn states(output: &str) -> BTreeMap<&str, (bool, bool)> {
    rows(output)
        .into_iter()
        .map(|(name, effective, default)| (name, (effective, default)))
        .collect()
}

#[test]
fn inspect_lists_every_extension_once_in_alphabetical_order() {
    let dir = project();
    let output = output(command(&dir).args(["--isolated", "--flavor", "pandoc", "--quiet"]));
    assert!(output.contains("Flavor: pandoc\n"));
    assert!(output.contains("Configuration: built-in defaults\n"));
    let rows = rows(&output);
    let names: Vec<_> = rows.iter().map(|row| row.0).collect();
    let mut expected = Extensions::KNOWN_NAMES.to_vec();
    expected.extend(FormatterExtensions::KNOWN_NAMES);
    expected.sort_unstable();
    expected.dedup();
    assert_eq!(names, expected);
    assert!(
        rows.iter()
            .all(|(_, effective, default)| effective == default)
    );
    let states = states(&output);
    assert_eq!(states["east-asian-line-breaks"], (false, false));
    assert_eq!(states["smart"], (true, true));
    assert_eq!(states["smart-quotes"], (false, false));
}

#[test]
fn inspect_discovers_config_from_document_and_shows_inheritance() {
    let dir = project();
    fs::write(dir.path().join("panache.toml"), "flavor = 'commonmark'\n").unwrap();
    fs::write(
        dir.path().join("base.toml"),
        "[extensions]\ncitations = false\nsmart = false\neast-asian-line-breaks = true\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("docs")).unwrap();
    fs::write(
        dir.path().join("docs/panache.toml"),
        "extend = '../base.toml'\n[extensions.quarto]\ncitations = true\n",
    )
    .unwrap();

    let output = output(command(&dir).arg("docs/document.qmd"));
    assert!(output.contains("Flavor: quarto\n"));
    assert!(output.contains(&format!(
        "{} (discovered)",
        Path::new("docs").join("panache.toml").display()
    )));
    assert!(output.contains(&format!(
        "  extends {}",
        dir.path().join("base.toml").canonicalize().unwrap().display()
    )));
    let states = states(&output);
    assert_eq!(states["citations"], (true, true));
    assert_eq!(states["smart"], (false, true));
    assert_eq!(states["east-asian-line-breaks"], (true, false));
    assert_eq!(states["quarto-crossrefs"], (true, true));
}

#[test]
fn inspect_applies_cli_overrides_without_replacing_other_config_values() {
    let dir = project();
    fs::write(
        dir.path().join("panache.toml"),
        "[extensions]\neast-asian-line-breaks = true\ncitations = false\nsmart = false\n",
    )
    .unwrap();
    let output = output(command(&dir).args([
        "--flavor",
        "pandoc",
        "-o",
        "extensions.east-asian-line-breaks=true",
        "-o",
        "extensions.east-asian-line-breaks=false",
        "-o",
        "extensions.smart=true",
    ]));
    let states = states(&output);
    assert_eq!(states["east-asian-line-breaks"], (false, false));
    assert_eq!(states["smart"], (true, true));
    assert_eq!(states["citations"], (false, true));
}

#[test]
fn inspect_uses_current_directory_config_when_no_document_is_given() {
    let dir = project();
    fs::write(dir.path().join("panache.toml"), "flavor = 'gfm'\n").unwrap();
    let output = output(&mut command(&dir));
    assert!(output.contains("Flavor: gfm\n"));
    assert!(output.contains("panache.toml (discovered)"));
    let states = states(&output);
    assert_eq!(states["smart"], (false, false));
    assert_eq!(states["gfm-auto-identifiers"], (true, true));
}

#[test]
fn inspect_respects_flavor_patterns_and_explicit_flavor_override() {
    let dir = project();
    fs::write(
        dir.path().join("panache.toml"),
        "[flavors]\ngfm = ['*.md']\n[extensions.gfm]\ncitations = true\n",
    )
    .unwrap();
    let inferred = output(command(&dir).arg("document.md"));
    assert!(inferred.contains("Flavor: gfm\n"));
    assert_eq!(states(&inferred)["citations"], (true, false));

    let overridden = output(command(&dir).args(["document.md", "--flavor", "commonmark"]));
    assert!(overridden.contains("Flavor: commonmark\n"));
    assert_eq!(states(&overridden)["citations"], (false, false));
}

#[test]
fn inspect_isolated_ignores_config_and_detects_synthetic_filename() {
    let dir = project();
    fs::write(dir.path().join("panache.toml"), "invalid config").unwrap();
    command(&dir)
        .args(["--isolated", "--stdin-filename", "document.qmd"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Flavor: quarto\n"))
        .stdout(predicate::str::contains(
            "Configuration: built-in defaults\n",
        ));

    command(&dir)
        .args(["--isolated", "document.qmd", "--flavor", "pandoc"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Flavor: pandoc\n"));
}

#[test]
fn inspect_reports_explicit_and_environment_config_sources() {
    let dir = project();
    let config = dir.path().join("custom.toml");
    fs::write(&config, "flavor = 'gfm'\n").unwrap();
    command(&dir)
        .arg("--config")
        .arg(&config)
        .assert()
        .success()
        .stdout(predicate::str::contains("Flavor: gfm\n"))
        .stdout(predicate::str::contains("custom.toml (--config)"));

    command(&dir)
        .env("PANACHE_CONFIG", &config)
        .assert()
        .success()
        .stdout(predicate::str::contains("custom.toml (PANACHE_CONFIG)"));
}

#[test]
fn inspect_reports_invalid_config_and_unknown_extension_overrides() {
    let dir = project();
    fs::write(
        dir.path().join("panache.toml"),
        "[extensions]\ncitationz = true\n",
    )
    .unwrap();
    command(&dir)
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("citationz"));

    command(&dir)
        .args(["--isolated", "-o", "extensions.citationz=true"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("citationz"));
}

#[test]
fn inspect_json_matches_text_extension_states_and_lists_default_config() {
    let dir = project();
    let args = [
        "--isolated",
        "--flavor",
        "pandoc",
        "-o",
        "extensions.east-asian-line-breaks=true",
        "-o",
        "extensions.smart=false",
    ];
    let text = output(command(&dir).args(args));
    let output = output(command(&dir).args(args).args(["--json", "--quiet"]));
    assert!(output.ends_with('\n'));
    let report: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(report["flavor"], "pandoc");
    assert_eq!(
        report["configuration"],
        json!({ "source": "none", "path": null, "chain": [] })
    );
    let expected: BTreeMap<_, _> = states(&text)
        .into_iter()
        .map(|(name, (effective, default))| {
            (name, json!({ "effective": effective, "default": default }))
        })
        .collect();
    assert_eq!(report["extensions"], json!(expected));
    assert_eq!(
        report["extensions"]["east-asian-line-breaks"],
        json!({ "effective": true, "default": false })
    );
    assert_eq!(
        report["extensions"]["smart"],
        json!({ "effective": false, "default": true })
    );
}

#[test]
fn inspect_json_reports_discovered_config_chain_and_scoped_overrides() {
    let dir = project();
    let base = dir.path().join("base.toml");
    fs::write(&base, "[extensions]\ncitations = false\nsmart = false\n").unwrap();
    let config = dir.path().join("panache.toml");
    fs::write(
        &config,
        "extend = 'base.toml'\n[extensions.quarto]\ncitations = true\n",
    )
    .unwrap();

    let output = output(command(&dir).args(["document.qmd", "--json"]));
    let report: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(report["flavor"], "quarto");
    assert_eq!(report["configuration"]["source"], "discovered");
    assert_eq!(
        Path::new(report["configuration"]["path"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        config.canonicalize().unwrap()
    );
    assert_eq!(
        report["configuration"]["chain"],
        json!([config.canonicalize().unwrap(), base.canonicalize().unwrap()])
    );
    assert_eq!(
        report["extensions"]["citations"],
        json!({ "effective": true, "default": true })
    );
    assert_eq!(
        report["extensions"]["smart"],
        json!({ "effective": false, "default": true })
    );
}

#[test]
fn inspect_json_uses_cli_flavor_names() {
    let dir = project();
    for flavor in ["commonmark", "rmarkdown", "multimarkdown"] {
        let output = output(command(&dir).args(["--isolated", "--flavor", flavor, "--json"]));
        let report: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(report["flavor"], flavor);
    }
}

#[test]
fn inspect_json_keeps_config_errors_on_stderr() {
    let dir = project();
    fs::write(dir.path().join("panache.toml"), "invalid config").unwrap();
    command(&dir)
        .arg("--json")
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("invalid config"));
}
