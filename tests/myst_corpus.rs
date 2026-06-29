//! MyST smoke corpus: losslessness + idempotency over the upstream
//! `myst-spec` examples.
//!
//! The corpus is vendored from `jupyter-book/myst-spec` (`docs/examples/*.yml`)
//! by `crates/panache-parser/scripts/update-myst-spec-fixtures.sh`. Each example
//! file is a list of cases carrying a `myst:` markdown input alongside
//! `mdast`/`html` targets. Those targets describe a JS AST and rendered HTML
//! that do not map onto Panache's CST, so this harness consumes only the `myst`
//! inputs and asserts two flavor-agnostic invariants under `flavor = myst`:
//!
//!   1. Losslessness: `parse(input)` round-trips to the exact input bytes.
//!   2. Idempotency:  `format(format(input)) == format(input)`.
//!
//! This is a vetting tool, not a conformance gate. Known divergences are tracked
//! in `EXPECTED_FAILURES` so regressions fail loudly while the remaining gaps
//! stay explicit. The CommonMark-derived upstream files are deliberately not
//! vendored (covered by the dedicated `tests/commonmark.rs` spec.txt harness).
//!
//! Refresh the corpus with
//! `sh crates/panache-parser/scripts/update-myst-spec-fixtures.sh`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use panache::config::{Extensions, Flavor};
use panache::{Config, format, parse};

/// `file::title` identifiers for cases that do not yet satisfy losslessness or
/// idempotency. Populated from the first run; shrinking this set is the point of
/// the corpus. A case that starts passing must be removed here (the harness
/// fails on unexpected passes so the list cannot rot).
const EXPECTED_FAILURES: &[&str] = &[];

fn myst_config() -> Config {
    Config {
        flavor: Flavor::Myst,
        extensions: Extensions::for_flavor(Flavor::Myst),
        ..Default::default()
    }
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/panache-parser/tests/fixtures/myst-spec/examples")
}

/// One extracted `myst:` input with a `file::title` identifier for reporting.
struct Case {
    id: String,
    input: String,
}

/// Extract every `myst: |`/`|-`/`|+` block-literal scalar from one example
/// file, pairing each with the nearest preceding `title:`.
///
/// The MyST-specific fixtures use only literal block scalars (no quoted/escaped
/// or explicit-indent forms), so a focused block-scalar reader is sufficient and
/// avoids pulling in a YAML dependency. Any unhandled scalar style is skipped
/// rather than mis-extracted.
fn extract_cases(file_stem: &str, text: &str) -> Vec<Case> {
    let lines: Vec<&str> = text.lines().collect();
    let mut cases = Vec::new();
    let mut current_title = String::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix("- title:") {
            current_title = rest.trim().trim_matches(['\'', '"']).to_string();
        } else if let Some(rest) = trimmed.strip_prefix("title:") {
            current_title = rest.trim().trim_matches(['\'', '"']).to_string();
        }

        if let Some((key_indent, chomp)) = parse_block_scalar_header(line, "myst:") {
            let (content, next) = read_block_scalar(&lines, i + 1, key_indent, chomp);
            cases.push(Case {
                id: format!("{file_stem}::{current_title}"),
                input: content,
            });
            i = next;
            continue;
        }

        i += 1;
    }

    cases
}

/// If `line` is `<indent><key> |` (optionally with a `-`/`+` chomping
/// indicator), return `(indent_width, chomp)`. Returns `None` for any other
/// scalar style (quoted, plain, explicit indent indicator), which the harness
/// skips.
fn parse_block_scalar_header(line: &str, key: &str) -> Option<(usize, char)> {
    let indent = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    let value = trimmed.strip_prefix(key)?.trim();
    match value {
        "|" => Some((indent, ' ')),
        "|-" => Some((indent, '-')),
        "|+" => Some((indent, '+')),
        _ => None,
    }
}

/// Read a literal block scalar starting at `start`, given the indentation of the
/// owning key and a chomping indicator (`' '` clip, `'-'` strip, `'+'` keep).
/// Returns the dedented content and the index of the first line after the block.
fn read_block_scalar(
    lines: &[&str],
    start: usize,
    key_indent: usize,
    chomp: char,
) -> (String, usize) {
    // Content indentation is the indent of the first non-blank line.
    let mut content_indent = None;
    let mut end = start;
    while end < lines.len() {
        let line = lines[end];
        if line.trim().is_empty() {
            end += 1;
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent <= key_indent {
            break;
        }
        content_indent = Some(indent);
        break;
    }

    let content_indent = match content_indent {
        Some(width) => width,
        None => return (String::new(), end),
    };

    let mut out: Vec<String> = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            out.push(String::new());
            i += 1;
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent < content_indent {
            break;
        }
        out.push(line[content_indent..].to_string());
        i += 1;
    }

    // Apply chomping to trailing blank lines.
    match chomp {
        '+' => {} // keep
        _ => {
            while matches!(out.last(), Some(s) if s.is_empty()) {
                out.pop();
            }
        }
    }

    let mut content = out.join("\n");
    if chomp != '-' && !content.is_empty() {
        content.push('\n'); // clip / keep: single trailing newline
    }
    (content, i)
}

fn all_cases() -> Vec<Case> {
    let dir = fixtures_dir();
    assert!(
        dir.is_dir(),
        "MyST fixtures missing at {}; run \
         `sh crates/panache-parser/scripts/update-myst-spec-fixtures.sh`",
        dir.display()
    );

    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("read myst-spec fixtures dir")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "yml"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no MyST fixture files vendored");

    let mut cases = Vec::new();
    for path in files {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let text = fs::read_to_string(&path).expect("read fixture file");
        cases.extend(extract_cases(&stem, &text));
    }
    cases
}

fn losslessness_failure(input: &str) -> bool {
    let tree = parse(input, Some(myst_config()));
    tree.text() != input
}

fn idempotency_failure(input: &str) -> bool {
    let once = format(input, Some(myst_config()), None);
    let twice = format(&once, Some(myst_config()), None);
    once != twice
}

fn non_blank_lines(s: &str) -> usize {
    s.lines().filter(|l| !l.trim().is_empty()).count()
}

#[test]
fn corpus_extraction_is_sane() {
    let cases = all_cases();
    assert!(
        cases.len() >= 50,
        "expected a substantial MyST corpus, got {} cases",
        cases.len()
    );
    // Every vendored MyST-specific case uses a non-empty `|-` block scalar, so a
    // blank extraction means the block reader silently dropped content (e.g.
    // after an upstream layout change). Guard against vacuous passes.
    let empty: Vec<&str> = cases
        .iter()
        .filter(|c| c.input.trim().is_empty())
        .map(|c| c.id.as_str())
        .collect();
    assert!(
        empty.is_empty(),
        "extracted empty `myst:` inputs (extractor likely out of sync):\n  {}",
        empty.join("\n  ")
    );
}

#[test]
fn losslessness_and_idempotency_smoke() {
    let cases = all_cases();
    let expected: BTreeSet<&str> = EXPECTED_FAILURES.iter().copied().collect();

    let mut failures: BTreeSet<String> = BTreeSet::new();
    for case in &cases {
        if losslessness_failure(&case.input) {
            failures.insert(format!("{} [losslessness]", case.id));
        }
        if idempotency_failure(&case.input) {
            failures.insert(format!("{} [idempotency]", case.id));
        }
    }

    let failing_ids: BTreeSet<&str> = failures
        .iter()
        .map(|s| s.split(" [").next().unwrap())
        .collect();

    let unexpected: Vec<&String> = failures
        .iter()
        .filter(|f| {
            let id = f.split(" [").next().unwrap();
            !expected.contains(id)
        })
        .collect();
    let fixed: Vec<&&str> = expected
        .iter()
        .filter(|id| !failing_ids.contains(**id))
        .collect();

    assert!(
        unexpected.is_empty(),
        "MyST smoke regressions ({} new failing checks):\n  {}",
        unexpected.len(),
        unexpected
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    assert!(
        fixed.is_empty(),
        "cases now passing; remove from EXPECTED_FAILURES:\n  {}",
        fixed.iter().map(|s| **s).collect::<Vec<_>>().join("\n  ")
    );
}

/// Output-divergence triage. Losslessness + idempotency only prove the formatter
/// is byte-preserving and stable -- they happily certify *idempotent garbage*
/// (e.g. a directive's options and code body reflowed onto one line). This tool
/// surfaces where `format(input)` diverges from the canonical upstream `myst`
/// input and splits the divergences into:
///
///   - STRUCTURAL: the non-blank line count changed, which almost always means
///     content was merged or split (the serious botches: option/body merges,
///     code-body reflow, table/footnote collapse).
///   - cosmetic: same line structure (emphasis `_`->`*`, list-marker spacing,
///     table alignment), which is normally legitimate normalization.
///
/// Non-gating by design (`#[ignore]`): canonical upstream form differs from
/// Panache's house style for valid reasons, so this is a review aid, not an
/// assertion. Run with:
///   `cargo test --test myst_corpus -- --ignored --nocapture`
#[test]
#[ignore = "review aid: prints an output-divergence report, does not assert"]
fn triage_format_divergence() {
    let cases = all_cases();
    let mut structural: Vec<(String, String, String)> = Vec::new();
    let mut cosmetic: Vec<String> = Vec::new();

    for case in &cases {
        let out = format(&case.input, Some(myst_config()), None);
        if out.trim_end() == case.input.trim_end() {
            continue;
        }
        if non_blank_lines(&out) != non_blank_lines(&case.input) {
            structural.push((case.id.clone(), case.input.clone(), out));
        } else {
            cosmetic.push(case.id.clone());
        }
    }

    eprintln!(
        "\nMyST format divergence: {}/{} cases differ from canonical input \
         ({} STRUCTURAL, {} cosmetic)\n",
        structural.len() + cosmetic.len(),
        cases.len(),
        structural.len(),
        cosmetic.len(),
    );

    if !structural.is_empty() {
        eprintln!("== STRUCTURAL (line count changed -- likely botch) ==");
        for (id, input, out) in &structural {
            eprintln!(
                "\n-- {id}\n   IN : {}\n   OUT: {}",
                indent(input),
                indent(out)
            );
        }
    }
    if !cosmetic.is_empty() {
        eprintln!("\n== cosmetic (same line structure) ==");
        for id in &cosmetic {
            eprintln!("   {id}");
        }
    }
    eprintln!();
}

/// Render a multi-line block inline for the triage log (one indented line each).
fn indent(s: &str) -> String {
    s.trim_end().lines().collect::<Vec<_>>().join("\n        ")
}
