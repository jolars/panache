use panache::parser::yaml::{
    ShadowYamlOptions, ShadowYamlOutcome, YamlInputKind, parse_basic_entry, parse_shadow,
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
    assert_eq!(report.shadow_reason, "shadow-disabled");
    assert!(report.normalized_input.is_none());

    let parsed = parse_basic_entry("title: Shadow");
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
reason=prototype-basic-entry-parsed
kind=Plain
bytes=15
lines=1
normalized=Some(\"title: Snapshot\")

[enabled-hashpipe]
outcome=PrototypeParsed
reason=prototype-basic-entry-parsed
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
outcome=PrototypeRejected
reason=prototype-basic-entry-rejected
kind=Plain
bytes=29
lines=2
normalized=Some(\"title: Snapshot\\r\\nauthor: Me\\r\\n\")

[enabled-hashpipe-crlf-multiline]
outcome=PrototypeRejected
reason=prototype-basic-entry-rejected
kind=Hashpipe
bytes=35
lines=2
normalized=Some(\"title: Snapshot\\nauthor: Me\")
";

    assert_eq!(snapshot, expected);
}
