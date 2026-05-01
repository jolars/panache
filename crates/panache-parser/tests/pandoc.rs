//! Pandoc-native conformance harness.
//!
//! Layout:
//! - Corpus: `tests/fixtures/pandoc-conformance/corpus/<NNNN>-<section>-<slug>/`
//!   with `input.md` (markdown) + `expected.native` (pinned output of
//!   `pandoc -f markdown -t native`). Refresh via
//!   `scripts/update-pandoc-conformance-corpus.sh`.
//! - Allowlist: `tests/pandoc/allowlist.txt` — IDs guarded against regression.
//! - Blocked list: `tests/pandoc/blocked.txt` — IDs deliberately deferred.
//!
//! Two main test functions:
//! - `pandoc_allowlist`: panics if any allowlisted case regresses.
//! - `pandoc_full_report` (ignored by default): runs every case and writes a
//!   triage summary to `tests/pandoc/report.txt` and structured sidecar
//!   `docs/development/pandoc-report.json`.

#[path = "pandoc/corpus_loader.rs"]
mod corpus_loader;

#[path = "pandoc/native_projector.rs"]
mod native_projector;

use corpus_loader::{PandocCase, read_corpus};
use native_projector::{normalize_native, project};
use panache_parser::{Dialect, Extensions, Flavor, ParserOptions, parse};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const CORPUS_REL: &str = "tests/fixtures/pandoc-conformance/corpus";
const SOURCE_REL: &str = "tests/fixtures/pandoc-conformance/.panache-source";
const ALLOWLIST_REL: &str = "tests/pandoc/allowlist.txt";
const BLOCKED_REL: &str = "tests/pandoc/blocked.txt";
const REPORT_REL: &str = "tests/pandoc/report.txt";
/// Structured sidecar consumed by future Quarto pandoc-conformance dashboard.
/// Path is resolved relative to the workspace root, not `CARGO_MANIFEST_DIR`.
const REPORT_JSON_DOCS_REL: &str = "../../docs/development/pandoc-report.json";

fn manifest_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn pandoc_options() -> ParserOptions {
    ParserOptions {
        flavor: Flavor::Pandoc,
        dialect: Dialect::for_flavor(Flavor::Pandoc),
        extensions: Extensions::for_flavor(Flavor::Pandoc),
        ..ParserOptions::default()
    }
}

fn render_case(case: &PandocCase) -> String {
    let tree = parse(&case.markdown, Some(pandoc_options()));
    project(&tree)
}

fn matches_expected(case: &PandocCase, rendered: &str) -> bool {
    normalize_native(rendered) == normalize_native(&case.expected_native)
}

fn read_id_file(path: &Path) -> BTreeSet<u32> {
    if !path.exists() {
        return BTreeSet::new();
    }
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            l.parse::<u32>()
                .unwrap_or_else(|_| panic!("invalid case id in {}: {l:?}", path.display()))
        })
        .collect()
}

fn pandoc_version() -> String {
    let path = manifest_path(SOURCE_REL);
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    for line in raw.lines() {
        if let Some(rest) = line.trim().strip_prefix("pandoc_version=") {
            return rest.trim().to_string();
        }
    }
    "unknown".to_string()
}

#[test]
fn corpus_loader_reads_seed_corpus() {
    let cases = read_corpus(&manifest_path(CORPUS_REL));
    assert!(
        !cases.is_empty(),
        "seed corpus should not be empty; check {}",
        CORPUS_REL
    );
    assert_eq!(cases[0].id, 1);
    assert!(
        !cases[0].section.is_empty(),
        "case slugs must encode `<NNNN>-<section>-<slug>`"
    );
}

#[test]
fn pandoc_allowlist() {
    let allowlist_path = manifest_path(ALLOWLIST_REL);
    assert!(
        allowlist_path.exists(),
        "missing allowlist file: {}",
        allowlist_path.display()
    );
    let allowed = read_id_file(&allowlist_path);
    if allowed.is_empty() {
        return; // baseline still being seeded
    }

    let cases = read_corpus(&manifest_path(CORPUS_REL));
    let by_id: std::collections::HashMap<u32, &PandocCase> =
        cases.iter().map(|c| (c.id, c)).collect();

    let mut regressions = Vec::new();
    for id in &allowed {
        let case = by_id
            .get(id)
            .unwrap_or_else(|| panic!("allowlisted case #{id} not found in corpus"));
        let rendered = render_case(case);
        if !matches_expected(case, &rendered) {
            regressions.push((*id, case.slug.clone()));
        }
    }

    assert!(
        regressions.is_empty(),
        "allowlisted pandoc-conformance cases regressed:\n{}",
        regressions
            .iter()
            .map(|(n, s)| format!("  #{n} ({s})"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
#[ignore = "manual: run to generate triage report and seed/grow the allowlist"]
fn pandoc_full_report() {
    let cases = read_corpus(&manifest_path(CORPUS_REL));
    let blocked = read_id_file(&manifest_path(BLOCKED_REL));
    let pandoc_ver = pandoc_version();

    let mut passing = Vec::new();
    let mut failing = Vec::new();
    let mut by_section_pass: std::collections::BTreeMap<String, u32> =
        std::collections::BTreeMap::new();
    let mut by_section_fail: std::collections::BTreeMap<String, u32> =
        std::collections::BTreeMap::new();

    for case in &cases {
        let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render_case(case)));
        let ok = match rendered {
            Ok(s) => matches_expected(case, &s),
            Err(_) => false,
        };
        if ok {
            passing.push(case.id);
            *by_section_pass.entry(case.section.clone()).or_insert(0) += 1;
        } else {
            failing.push(case.id);
            *by_section_fail.entry(case.section.clone()).or_insert(0) += 1;
        }
    }

    let total = cases.len();
    let pass = passing.len();
    let fail = failing.len();

    let mut report = String::new();
    report.push_str(&format!(
        "Pandoc-native conformance report (pandoc {pandoc_ver})\n"
    ));
    report.push_str(&format!(
        "{pass} / {total} cases passing ({:.1}%)\n",
        if total == 0 {
            0.0
        } else {
            (pass as f64 / total as f64) * 100.0
        }
    ));
    report.push_str(&format!("{fail} failing\n"));
    report.push_str(&format!("{} blocked-list entries\n\n", blocked.len()));

    report.push_str("=== Per-section pass / fail ===\n");
    let all_sections: BTreeSet<&String> = by_section_pass
        .keys()
        .chain(by_section_fail.keys())
        .collect();
    for section in &all_sections {
        let p = by_section_pass.get(*section).copied().unwrap_or(0);
        let f = by_section_fail.get(*section).copied().unwrap_or(0);
        report.push_str(&format!("  {section}: {p} pass / {f} fail\n"));
    }

    report.push_str("\n=== Passing case ids (allowlist candidates) ===\n");
    for n in &passing {
        report.push_str(&format!("{n}\n"));
    }

    report.push_str("\n=== Passing cases grouped by section ===\n");
    let by_id: std::collections::HashMap<u32, &PandocCase> =
        cases.iter().map(|c| (c.id, c)).collect();
    let mut section_to_passing: std::collections::BTreeMap<&str, Vec<u32>> =
        std::collections::BTreeMap::new();
    for n in &passing {
        let sec = by_id[n].section.as_str();
        section_to_passing.entry(sec).or_default().push(*n);
    }
    for (section, nums) in &section_to_passing {
        report.push_str(&format!("# {section}\n"));
        for n in nums {
            report.push_str(&format!("{n}\n"));
        }
        report.push('\n');
    }

    if !failing.is_empty() {
        report.push_str("=== Failing case slugs (with rendered diff hint) ===\n");
        for n in &failing {
            let case = by_id[n];
            report.push_str(&format!("# {} ({})\n", case.id, case.slug));
            let rendered =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render_case(case)))
                    .unwrap_or_else(|_| "<panicked>".to_string());
            report.push_str(&format!(
                "  expected: {}\n",
                normalize_native(&case.expected_native)
            ));
            report.push_str(&format!("  got:      {}\n", normalize_native(&rendered)));
        }
        report.push('\n');
    }

    let report_path = manifest_path(REPORT_REL);
    fs::write(&report_path, &report)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", report_path.display()));

    let pass_pct = if total == 0 {
        0.0
    } else {
        (pass as f64 / total as f64) * 100.0
    };
    let sections: Vec<serde_json::Value> = all_sections
        .iter()
        .map(|name| {
            let p = by_section_pass.get(*name).copied().unwrap_or(0);
            let f = by_section_fail.get(*name).copied().unwrap_or(0);
            serde_json::json!({ "name": name, "pass": p, "fail": f })
        })
        .collect();
    let json = serde_json::json!({
        "pandoc_version": pandoc_ver,
        "total_cases": total,
        "passing": pass,
        "failing": fail,
        "pass_pct": (pass_pct * 10.0).round() / 10.0,
        "blocked": blocked.len(),
        "sections": sections,
        "passing_ids": passing,
    });
    let json_path = manifest_path(REPORT_JSON_DOCS_REL);
    fs::write(&json_path, format!("{:#}\n", json))
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", json_path.display()));

    eprintln!("\n{}", report);
    eprintln!("(report written to {})", report_path.display());
    eprintln!("(json written to {})", json_path.display());
}
