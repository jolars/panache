use panache::parser::yaml::{
    ShadowYamlOptions, ShadowYamlOutcome, YamlInputKind, parse_basic_mapping_tree, parse_shadow,
};
use panache::syntax::cst_to_json;
use serde_json::json;
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

fn fixture_case_path(case_id: &str) -> PathBuf {
    fixture_root().join(case_id)
}

fn allowlisted_case_paths() -> Vec<(String, PathBuf)> {
    let allowlist = Path::new(env!("CARGO_MANIFEST_DIR")).join(ALLOWLIST_PATH);
    assert!(
        allowlist.exists(),
        "missing allowlist file: {}",
        allowlist.display()
    );
    let case_ids = read_lines(&allowlist);
    assert!(
        !case_ids.is_empty(),
        "allowlist must include at least one case"
    );

    case_ids
        .into_iter()
        .map(|case_id| {
            let case_path = fixture_case_path(&case_id);
            assert!(
                case_path.exists(),
                "fixture case directory missing for {} ({})",
                case_id,
                case_path.display()
            );
            (case_id, case_path)
        })
        .collect()
}

fn fixture_case_events(case_path: &Path) -> Vec<String> {
    let event_path = case_path.join("test.event");
    let event_text = fs::read_to_string(&event_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", event_path.display()));
    event_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn cst_yaml_projected_events(input: &str) -> Vec<String> {
    let Some(tree) = parse_basic_mapping_tree(input) else {
        return Vec::new();
    };

    let mut values: Vec<String> = tree
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            tok.kind() == panache::syntax::SyntaxKind::YAML_KEY
                || tok.kind() == panache::syntax::SyntaxKind::YAML_SCALAR
        })
        .map(|tok| format!("=VAL :{}", tok.text()))
        .collect();

    let mut events = Vec::with_capacity(values.len() + 6);
    events.push("+STR".to_string());
    events.push("+DOC".to_string());
    events.push("+MAP".to_string());
    events.append(&mut values);
    events.push("-MAP".to_string());
    events.push("-DOC".to_string());
    events.push("-STR".to_string());
    events
}

fn render_shadow_report(label: &str, report: &panache::parser::yaml::ShadowYamlReport) -> String {
    format!(
        "{label}\noutcome={:?}\nreason={}\nkind={:?}\nbytes={}\nlines={}\nnormalized={:?}\n",
        report.outcome,
        report.shadow_reason,
        report.input_kind,
        report.input_len_bytes,
        report.line_count,
        report.normalized_input
    )
}

#[test]
fn yaml_allowlist_cases_snapshot() {
    let fixture_root = fixture_root();
    assert!(
        fixture_root.exists(),
        "yaml-test-suite fixtures missing; run `task update-yaml-fixtures`"
    );

    let blocked = Path::new(env!("CARGO_MANIFEST_DIR")).join(BLOCKED_PATH);
    assert!(
        blocked.exists(),
        "missing blocked file: {}",
        blocked.display()
    );

    for (case_id, case_path) in allowlisted_case_paths() {
        let in_yaml = case_path.join("in.yaml");
        let test_event = case_path.join("test.event");
        let error_file = case_path.join("error");
        assert!(
            test_event.exists(),
            "allowlisted case {} must include test.event ({})",
            case_id,
            test_event.display()
        );
        assert!(
            !error_file.exists(),
            "allowlisted case {} must not include error fixture ({})",
            case_id,
            error_file.display()
        );
        let input = fs::read_to_string(&in_yaml).unwrap_or_else(|e| {
            panic!(
                "failed to read case {} ({}): {e}",
                case_id,
                in_yaml.display()
            )
        });

        let parsed = parse_basic_mapping_tree(&input).is_some();
        let snapshot =
            format!("case_id: {case_id}\ninput: {input:?}\nparsed_mapping_tree: {parsed}\n");

        insta::assert_snapshot!(format!("yaml_suite_{}", case_id), snapshot);
    }
}

#[test]
fn yaml_allowlist_cases_cst_snapshot() {
    let fixture_root = fixture_root();
    assert!(
        fixture_root.exists(),
        "yaml-test-suite fixtures missing; run `task update-yaml-fixtures`"
    );

    for (case_id, case_path) in allowlisted_case_paths() {
        let in_yaml = case_path.join("in.yaml");
        let input = fs::read_to_string(&in_yaml).unwrap_or_else(|e| {
            panic!(
                "failed to read case {} ({}): {e}",
                case_id,
                in_yaml.display()
            )
        });

        let tree = parse_basic_mapping_tree(&input);
        let snapshot_json = json!({
            "case_id": case_id,
            "input": input,
            "cst": tree.as_ref().map(cst_to_json),
        });
        insta::assert_json_snapshot!(format!("yaml_cst_suite_{}", case_id), snapshot_json);
    }
}

#[test]
fn yaml_allowlist_losslessness_raw_input() {
    for (case_id, case_path) in allowlisted_case_paths() {
        let input_path = case_path.join("in.yaml");
        let input = fs::read_to_string(&input_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", input_path.display()));
        let tree = parse_basic_mapping_tree(&input)
            .unwrap_or_else(|| panic!("failed to parse raw input for {}", case_id));
        let tree_text = tree.text().to_string();
        similar_asserts::assert_eq!(
            input,
            tree_text,
            "yaml raw losslessness mismatch for {}",
            case_id
        );
    }
}

#[test]
fn yaml_allowlist_projected_event_parity() {
    for (case_id, case_path) in allowlisted_case_paths() {
        let input_path = case_path.join("in.yaml");
        let input = fs::read_to_string(&input_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", input_path.display()));
        let expected_events = fixture_case_events(&case_path);
        let actual_events = cst_yaml_projected_events(&input);
        assert_eq!(
            actual_events, expected_events,
            "projected event stream mismatch for {}",
            case_id
        );
    }
}

#[test]
fn yaml_shadow_defaults_to_noop_and_does_not_replace_pipeline() {
    let report = parse_shadow("title: Shadow", ShadowYamlOptions::default());
    assert_eq!(report.outcome, ShadowYamlOutcome::SkippedDisabled);
    assert_eq!(report.shadow_reason, "shadow-disabled");
    assert!(report.normalized_input.is_none());

    let parsed = parse_basic_mapping_tree("title: Shadow");
    assert!(parsed.is_some());
}

#[test]
fn yaml_shadow_report_snapshot_shape() {
    let disabled = parse_shadow("title: Snapshot", ShadowYamlOptions::default());
    let enabled_plain = parse_shadow(
        "title: Snapshot",
        ShadowYamlOptions {
            enabled: true,
            input_kind: YamlInputKind::Plain,
        },
    );
    let enabled_hashpipe = parse_shadow(
        "#| title: Snapshot",
        ShadowYamlOptions {
            enabled: true,
            input_kind: YamlInputKind::Hashpipe,
        },
    );

    let snapshot = [
        render_shadow_report("[disabled]", &disabled),
        render_shadow_report("[enabled-plain]", &enabled_plain),
        render_shadow_report("[enabled-hashpipe]", &enabled_hashpipe),
    ]
    .join("\n");

    let expected = "[disabled]
outcome=SkippedDisabled
reason=shadow-disabled
kind=Plain
bytes=15
lines=1
normalized=None

[enabled-plain]
outcome=PrototypeParsed
reason=prototype-basic-mapping-parsed
kind=Plain
bytes=15
lines=1
normalized=Some(\"title: Snapshot\")

[enabled-hashpipe]
outcome=PrototypeParsed
reason=prototype-basic-mapping-parsed
kind=Hashpipe
bytes=18
lines=1
normalized=Some(\"title: Snapshot\")
";

    assert_eq!(snapshot, expected);
}

#[test]
fn yaml_shadow_report_snapshot_multiline_crlf_shape() {
    let plain_multiline = parse_shadow(
        "title: Snapshot\r\nauthor: Me\r\n",
        ShadowYamlOptions {
            enabled: true,
            input_kind: YamlInputKind::Plain,
        },
    );
    let hashpipe_multiline = parse_shadow(
        "#| title: Snapshot\r\n#| author: Me\r\n",
        ShadowYamlOptions {
            enabled: true,
            input_kind: YamlInputKind::Hashpipe,
        },
    );

    let snapshot = [
        render_shadow_report("[enabled-plain-crlf-multiline]", &plain_multiline),
        render_shadow_report("[enabled-hashpipe-crlf-multiline]", &hashpipe_multiline),
    ]
    .join("\n");

    let expected = "[enabled-plain-crlf-multiline]
outcome=PrototypeParsed
reason=prototype-basic-mapping-parsed
kind=Plain
bytes=29
lines=2
normalized=Some(\"title: Snapshot\\r\\nauthor: Me\\r\\n\")

[enabled-hashpipe-crlf-multiline]
outcome=PrototypeParsed
reason=prototype-basic-mapping-parsed
kind=Hashpipe
bytes=35
lines=2
normalized=Some(\"title: Snapshot\\nauthor: Me\")
";

    assert_eq!(snapshot, expected);
}
