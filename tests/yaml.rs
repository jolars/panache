use panache::parser::yaml::{
    ShadowYamlOptions, ShadowYamlOutcome, parse_basic_entry, parse_shadow,
};
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_DIR: &str = "tests/fixtures/yaml-test-suite";
const ALLOWLIST_PATH: &str = "tests/yaml/allowlist.txt";
const BLOCKED_PATH: &str = "tests/yaml/blocked.txt";

fn read_lines(path: &Path) -> Vec<String> {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect()
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIR)
}

#[test]
fn yaml_allowlist_cases_snapshot() {
    let fixture_root = fixture_root();
    assert!(
        fixture_root.exists(),
        "yaml-test-suite fixtures missing; run `task update-yaml-fixtures`"
    );

    let allowlist = Path::new(env!("CARGO_MANIFEST_DIR")).join(ALLOWLIST_PATH);
    let blocked = Path::new(env!("CARGO_MANIFEST_DIR")).join(BLOCKED_PATH);
    assert!(
        allowlist.exists(),
        "missing allowlist file: {}",
        allowlist.display()
    );
    assert!(
        blocked.exists(),
        "missing blocked file: {}",
        blocked.display()
    );

    let case_ids = read_lines(&allowlist);
    assert!(
        !case_ids.is_empty(),
        "allowlist must include at least one case"
    );

    for case_id in case_ids {
        let in_yaml = fixture_root.join(&case_id).join("in.yaml");
        let input = fs::read_to_string(&in_yaml).unwrap_or_else(|e| {
            panic!(
                "failed to read case {} ({}): {e}",
                case_id,
                in_yaml.display()
            )
        });

        let parsed = parse_basic_entry(input.trim_end_matches('\n'));
        let snapshot = format!("case_id: {case_id}\ninput: {input:?}\nparsed: {parsed:#?}\n");

        insta::assert_snapshot!(format!("yaml_suite_{}", case_id), snapshot);
    }
}

#[test]
fn yaml_shadow_defaults_to_noop_and_does_not_replace_pipeline() {
    let report = parse_shadow("title: Shadow", ShadowYamlOptions::default());
    assert_eq!(report.outcome, ShadowYamlOutcome::SkippedDisabled);
    assert!(report.normalized_input.is_none());

    let parsed = parse_basic_entry("title: Shadow");
    assert!(parsed.is_some());
}
