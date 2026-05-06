use panache_parser::parser::yaml::{
    ShadowYamlOptions, ShadowYamlOutcome, YamlInputKind, parse_shadow, parse_yaml_report,
    parse_yaml_tree, project_events, shadow_scanner_check,
};
use panache_parser::syntax::SyntaxNode as ParserSyntaxNode;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_DIR: &str = "tests/fixtures/yaml-test-suite";
const ALLOWLIST_PATH: &str = "tests/yaml/allowlist.txt";

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

fn all_case_paths() -> Vec<(String, PathBuf)> {
    let root = fixture_root();
    let mut entries: Vec<(String, PathBuf)> = fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", root.display()))
        .filter_map(|entry| {
            let entry = entry.unwrap_or_else(|e| panic!("failed to read dir entry: {e}"));
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let case_id = path
                .file_name()
                .and_then(|s| s.to_str())
                .expect("valid UTF-8 case id")
                .to_string();
            Some((case_id, path))
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
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

fn cst_text(node: &ParserSyntaxNode) -> String {
    format!("{:#?}\n", node)
}

fn render_shadow_report(
    label: &str,
    report: &panache_parser::parser::yaml::ShadowYamlReport,
) -> String {
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
#[ignore = "manual triage report generation"]
fn yaml_suite_generate_triage_report() {
    let mut passes_now = Vec::new();
    let mut error_contract_ok = Vec::new();
    let mut fails_needs_feature = Vec::new();
    let mut fails_needs_error_path = Vec::new();

    for (case_id, case_path) in all_case_paths() {
        let in_yaml = case_path.join("in.yaml");
        if !in_yaml.exists() {
            continue;
        }

        let input = fs::read_to_string(&in_yaml)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", in_yaml.display()));
        let error_contract = case_path.join("error").exists();
        let has_test_event = case_path.join("test.event").exists();
        let report = parse_yaml_report(&input);

        let event_parity = if has_test_event {
            let expected = fixture_case_events(&case_path);
            let actual = std::panic::catch_unwind(|| project_events(&input));
            actual.ok().map(|events| events == expected)
        } else {
            None
        };

        if !error_contract {
            if report.tree.is_some() && event_parity == Some(true) {
                passes_now.push(case_id);
            } else {
                fails_needs_feature.push(json!({
                    "case_id": case_id,
                    "tree": report.tree.is_some(),
                    "event_parity": event_parity,
                    "diagnostic_codes": report
                        .diagnostics
                        .iter()
                        .map(|d| d.code)
                        .collect::<Vec<_>>(),
                }));
            }
            continue;
        }

        if report.tree.is_none() && !report.diagnostics.is_empty() {
            error_contract_ok.push(json!({
                "case_id": case_id,
                "diagnostic_codes": report
                    .diagnostics
                    .iter()
                    .map(|d| d.code)
                    .collect::<Vec<_>>(),
                "event_parity": event_parity,
            }));
        } else {
            fails_needs_error_path.push(json!({
                "case_id": case_id,
                "tree": report.tree.is_some(),
                "diagnostic_codes": report
                    .diagnostics
                    .iter()
                    .map(|d| d.code)
                    .collect::<Vec<_>>(),
                "event_parity": event_parity,
            }));
        }
    }

    let triage = json!({
        "summary": {
            "total_cases": all_case_paths().len(),
            "passes_now_count": passes_now.len(),
            "error_contract_ok_count": error_contract_ok.len(),
            "fails_needs_feature_count": fails_needs_feature.len(),
            "fails_needs_error_path_count": fails_needs_error_path.len(),
        },
        "passes_now": passes_now,
        "error_contract_ok": error_contract_ok,
        "fails_needs_feature": fails_needs_feature,
        "fails_needs_error_path": fails_needs_error_path,
    });

    let out_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/yaml/triage.json");
    fs::write(
        &out_path,
        serde_json::to_string_pretty(&triage)
            .unwrap_or_else(|e| panic!("failed to serialize triage JSON: {e}")),
    )
    .unwrap_or_else(|e| panic!("failed to write {}: {e}", out_path.display()));
}

#[test]
fn yaml_allowlist_cases_snapshot() {
    let fixture_root = fixture_root();
    assert!(
        fixture_root.exists(),
        "yaml-test-suite fixtures missing; run `task update-yaml-fixtures`"
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
        let input = fs::read_to_string(&in_yaml).unwrap_or_else(|e| {
            panic!(
                "failed to read case {} ({}): {e}",
                case_id,
                in_yaml.display()
            )
        });

        let has_error_contract = error_file.exists();
        let report = parse_yaml_report(&input);
        let parsed = report.tree.is_some();
        let diagnostic_snapshot = report
            .diagnostics
            .iter()
            .map(|d| format!("{}:{}@{}..{}", d.code, d.message, d.byte_start, d.byte_end))
            .collect::<Vec<_>>()
            .join(", ");
        let snapshot = format!(
            "case_id: {case_id}\ninput: {input:?}\nhas_error_contract: {has_error_contract}\nparsed_mapping_tree: {parsed}\ndiagnostics: [{diagnostic_snapshot}]\n"
        );

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

        let report = parse_yaml_report(&input);
        let diagnostics = report
            .diagnostics
            .iter()
            .map(|d| format!("{}:{}@{}..{}", d.code, d.message, d.byte_start, d.byte_end))
            .collect::<Vec<_>>()
            .join("\n");
        let cst = report.tree.as_ref().map(cst_text).unwrap_or_default();
        let snapshot = format!(
            "case_id: {case_id}\ninput: {input:?}\ndiagnostics:\n{diagnostics}\ncst:\n{cst}"
        );
        insta::assert_snapshot!(format!("yaml_cst_suite_{}", case_id), snapshot);
    }
}

#[test]
fn yaml_allowlist_losslessness_raw_input() {
    for (case_id, case_path) in allowlisted_case_paths() {
        let input_path = case_path.join("in.yaml");
        let error_file = case_path.join("error");
        let input = fs::read_to_string(&input_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", input_path.display()));
        let report = parse_yaml_report(&input);
        let tree = report.tree;

        if error_file.exists() {
            assert!(
                tree.is_none(),
                "error-contract case {} should fail YAML parse",
                case_id
            );
            assert!(
                !report.diagnostics.is_empty(),
                "error-contract case {} should provide diagnostics",
                case_id
            );
            continue;
        }

        let tree = tree.unwrap_or_else(|| panic!("failed to parse raw input for {}", case_id));
        let tree_text = tree.text().to_string();
        assert_eq!(
            input, tree_text,
            "yaml raw losslessness mismatch for {}",
            case_id
        );
    }
}

#[test]
fn yaml_allowlist_projected_event_parity() {
    for (case_id, case_path) in allowlisted_case_paths() {
        let input_path = case_path.join("in.yaml");
        let error_file = case_path.join("error");
        let input = fs::read_to_string(&input_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", input_path.display()));
        let expected_events = fixture_case_events(&case_path);
        let actual_events = project_events(&input);
        let report = parse_yaml_report(&input);

        if error_file.exists() {
            assert!(
                report.tree.is_none(),
                "error-contract case {} should fail YAML parse",
                case_id
            );
            assert!(
                !report.diagnostics.is_empty(),
                "error-contract case {} should provide diagnostics",
                case_id
            );
            assert_ne!(
                actual_events, expected_events,
                "error-contract case {} unexpectedly matches success event parity",
                case_id
            );
        } else {
            assert_eq!(
                actual_events, expected_events,
                "projected event stream mismatch for {}",
                case_id
            );
        }
    }
}

#[test]
fn yaml_shadow_defaults_to_noop_and_does_not_replace_pipeline() {
    let report = parse_shadow("title: Shadow", ShadowYamlOptions::default());
    assert_eq!(report.outcome, ShadowYamlOutcome::SkippedDisabled);
    assert_eq!(report.shadow_reason, "shadow-disabled");
    assert!(report.normalized_input.is_none());

    let parsed = parse_yaml_tree("title: Shadow");
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

#[test]
fn yaml_document_start_emitted_as_dedicated_token() {
    use panache_parser::syntax::SyntaxKind;

    let report = parse_yaml_report("---\ntitle: test\n");
    let tree = report.tree.expect("should parse");

    let has_doc_start = tree
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::YAML_DOCUMENT_START);
    assert!(
        has_doc_start,
        "tree should contain YAML_DOCUMENT_START token"
    );

    assert_eq!(
        tree.text().to_string(),
        "---\ntitle: test\n",
        "losslessness"
    );

    let events = project_events("---\ntitle: test\n");
    assert_eq!(
        events,
        vec![
            "+STR",
            "+DOC ---",
            "+MAP",
            "=VAL :title",
            "=VAL :test",
            "-MAP",
            "-DOC",
            "-STR"
        ]
    );
}

#[test]
fn yaml_document_end_emitted_as_dedicated_token() {
    use panache_parser::syntax::SyntaxKind;

    let report = parse_yaml_report("title: test\n...\n");
    let tree = report.tree.expect("should parse");

    let has_doc_end = tree
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::YAML_DOCUMENT_END);
    assert!(has_doc_end, "tree should contain YAML_DOCUMENT_END token");

    assert_eq!(
        tree.text().to_string(),
        "title: test\n...\n",
        "losslessness"
    );
}

#[test]
fn yaml_block_sequence_scalar_items_cst() {
    use panache_parser::syntax::SyntaxKind;

    let report = parse_yaml_report("- foo\n- bar\n- 42\n");
    let tree = report.tree.expect("should parse");

    let has_seq = tree
        .descendants()
        .any(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE);
    assert!(has_seq, "tree should contain YAML_BLOCK_SEQUENCE node");

    let item_count = tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
        .count();
    assert_eq!(item_count, 3, "should have 3 sequence items");

    assert_eq!(
        tree.text().to_string(),
        "- foo\n- bar\n- 42\n",
        "losslessness"
    );
}

#[test]
fn yaml_block_sequence_scalar_projected_events() {
    let events = project_events("- foo\n- bar\n- 42\n");
    assert_eq!(
        events,
        vec![
            "+STR",
            "+DOC",
            "+SEQ",
            "=VAL :foo",
            "=VAL :bar",
            "=VAL :42",
            "-SEQ",
            "-DOC",
            "-STR"
        ]
    );
}

#[test]
fn yaml_block_sequence_single_item() {
    let events = project_events("- foo\n");
    assert_eq!(
        events,
        vec!["+STR", "+DOC", "+SEQ", "=VAL :foo", "-SEQ", "-DOC", "-STR"]
    );
}

#[test]
fn yaml_empty_value_key_only_lossless() {
    use panache_parser::syntax::SyntaxKind;

    let report = parse_yaml_report("foo:\nbar: baz\n");
    let tree = report.tree.expect("should parse");
    assert_eq!(tree.text().to_string(), "foo:\nbar: baz\n", "losslessness");

    let entry_count = tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
        .count();
    assert_eq!(entry_count, 2, "should have 2 map entries");
}

#[test]
fn yaml_nested_map_lossless() {
    use panache_parser::syntax::SyntaxKind;

    let report = parse_yaml_report("foo:\n  bar: baz\n");
    let tree = report.tree.expect("should parse");
    assert_eq!(
        tree.text().to_string(),
        "foo:\n  bar: baz\n",
        "losslessness"
    );

    let outer = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
        .expect("outer entry");
    let outer_key = outer
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
        .expect("outer key");
    let key_text = outer_key
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::YAML_KEY)
        .expect("key token")
        .text()
        .to_string();
    assert_eq!(key_text, "foo");
}

#[test]
fn yaml_nested_map_projected_events() {
    let events = project_events("foo:\n  bar: baz\n");
    assert_eq!(
        events,
        vec![
            "+STR",
            "+DOC",
            "+MAP",
            "=VAL :foo",
            "+MAP",
            "=VAL :bar",
            "=VAL :baz",
            "-MAP",
            "-MAP",
            "-DOC",
            "-STR"
        ]
    );
}

#[test]
fn yaml_flat_map_with_empty_value_projected_events() {
    let events = project_events("foo:\nbar: baz\n");
    assert_eq!(
        events,
        vec![
            "+STR",
            "+DOC",
            "+MAP",
            "=VAL :foo",
            "=VAL :",
            "=VAL :bar",
            "=VAL :baz",
            "-MAP",
            "-DOC",
            "-STR"
        ]
    );
}

/// Pins the post-PR-C empty-stream contract: inputs that contain only
/// trivia (comments / whitespace / a bare `...` marker) produce a
/// `YAML_STREAM` with no `YAML_DOCUMENT` children, which the events
/// projection emits as just `+STR -STR`. Regressing this would re-introduce
/// the comment-only-document bug fixed at the start of the shadow parser
/// refactor sequence.
#[test]
fn yaml_empty_stream_projects_no_document_events() {
    for input in ["# Comment only.\n", "...\n", "# comment\n...\n"] {
        let events = project_events(input);
        assert_eq!(
            events,
            vec!["+STR", "-STR"],
            "input {input:?} should project to an empty stream"
        );
    }
}

/// Manual harness: run the new streaming scanner over every allowlisted
/// fixture and assert byte-completeness (no gaps, no overlaps, last
/// non-synthetic token reaches `input.len()`). This gates the
/// step-12 cutover — the new scanner cannot replace the line-based
/// lexer until every allowlisted case is byte-complete. Ignored by
/// default so the regular test run stays fast; invoke with
/// `cargo test -p panache-parser --test yaml -- --ignored
/// shadow_scanner_byte_completeness_over_allowlist` after each scanner
/// step.
#[test]
#[ignore = "manual scanner-completeness check over allowlisted fixtures"]
fn shadow_scanner_byte_completeness_over_allowlist() {
    let mut failures: Vec<(String, String)> = Vec::new();
    for (case_id, case_path) in allowlisted_case_paths() {
        let input_path = case_path.join("in.yaml");
        if !input_path.exists() {
            continue;
        }
        let input = fs::read_to_string(&input_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", input_path.display()));
        let report = shadow_scanner_check(&input);
        if !report.byte_complete {
            failures.push((
                case_id,
                format!(
                    "byte_complete=false: last_token_end={}, input_len={}, gap_at={:?}, \
                     overlapping={}, diagnostic_codes={:?}",
                    report.last_token_end,
                    report.input_len,
                    report.gap_at,
                    report.overlapping,
                    report.diagnostic_codes,
                ),
            ));
        }
    }
    if !failures.is_empty() {
        let summary = failures
            .iter()
            .map(|(id, msg)| format!("- {id}: {msg}"))
            .collect::<Vec<_>>()
            .join("\n");
        panic!(
            "scanner not byte-complete over {} allowlisted case(s):\n{summary}",
            failures.len(),
        );
    }
}
