//! Cross-validation harness for the in-tree YAML formatter (Phase 1.3).
//!
//! For each `*.yaml` case under
//! `crates/panache-formatter/tests/fixtures/yaml_corpus/`, asserts:
//!
//! 1. **Parity.** `format_yaml(input)` equals `pretty_yaml::format_text(input)`
//!    with options bridged from `YamlFormatOptions` (mirroring the live
//!    `yaml_engine.rs` bridge — `print_width` ← `line_width`,
//!    `prose_wrap` ← `wrap`, everything else left at pretty_yaml defaults).
//! 2. **Idempotency.** `format_yaml(format_yaml(input)) == format_yaml(input)`.
//!
//! Disagreements are bugs to fix, not divergences to enumerate — see
//! `.claude/rules/yaml-formatter.md` and the cutover skill's `plan.md`
//! (Phase 1, "Cross-validation harness"). The diagnostic order is
//! in-tree formatter → in-tree parser CST shape → pretty_yaml.
//!
//! New cases land alongside the rule implementation (or parser CST fix)
//! that makes them pass. The starter corpus here is deliberately
//! narrow: trivially-canonical inputs that already round-trip through
//! the Phase 1.1 byte-passthrough stub. As the per-container
//! renderers land, more representative frontmatter (real
//! `tests/fixtures/cases/*/input.{md,qmd,Rmd}` extracts, hand-picked
//! stressors for comments / multi-line scalars / anchors / flow
//! overflow) graduates into the corpus.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use panache_formatter::formatter::yaml::{WrapMode, YamlFormatOptions, format_yaml};
use pretty_yaml::config::{FormatOptions, ProseWrap};

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/yaml_corpus")
}

fn discover_cases(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension() == Some(OsStr::new("yaml")) {
            out.push(path);
        }
    }
}

fn pretty_yaml_opts(in_tree: &YamlFormatOptions) -> FormatOptions {
    let mut opts = FormatOptions::default();
    opts.layout.print_width = in_tree.line_width;
    opts.language.prose_wrap = match in_tree.wrap {
        // pretty_yaml has no sentence/semantic prose modes; the reflow
        // family maps to `Always`, preserve to `Preserve`. The harness
        // only ever runs the default (`Reflow`) mode.
        WrapMode::Reflow | WrapMode::Sentence | WrapMode::Semantic => ProseWrap::Always,
        WrapMode::Preserve => ProseWrap::Preserve,
    };
    opts
}

/// Folded (`>`) block scalars are a deliberate divergence from
/// pretty_yaml: under STYLE.md rule 15 panache reflows their prose,
/// while pretty_yaml preserves every block scalar verbatim. Such cases
/// are exempt from the pretty_yaml *parity* check (idempotency is still
/// asserted). Detected by a value whose block-scalar header is a folded
/// indicator (`>`, `>-`, `>+`).
fn contains_folded_scalar(input: &str) -> bool {
    input.lines().any(|line| {
        let trimmed = line.trim_end();
        trimmed.ends_with('>') || trimmed.ends_with(">-") || trimmed.ends_with(">+")
    })
}

#[test]
fn corpus_cross_validates_against_pretty_yaml() {
    let root = corpus_root();
    let cases = discover_cases(&root);
    assert!(
        !cases.is_empty(),
        "no cases discovered under {}",
        root.display()
    );

    let opts = YamlFormatOptions::default();
    let pretty_opts = pretty_yaml_opts(&opts);

    let mut failures: Vec<String> = Vec::new();
    for case in &cases {
        let id = case
            .strip_prefix(&root)
            .unwrap_or(case)
            .display()
            .to_string();
        let input = match fs::read_to_string(case) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("[{id}] read error: {e}"));
                continue;
            }
        };

        let in_tree = format_yaml(&input, &opts);

        // Folded scalars deliberately diverge (rule 15); skip parity but
        // still assert idempotency below.
        if !contains_folded_scalar(&input) {
            let pretty = match pretty_yaml::format_text(&input, &pretty_opts) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("[{id}] pretty_yaml rejected reference input: {e}"));
                    continue;
                }
            };
            if in_tree != pretty {
                failures.push(format!(
                    "[{id}] parity break:\n  in_tree:\n{}\n  pretty:\n{}",
                    indent_block(&in_tree),
                    indent_block(&pretty),
                ));
                continue;
            }
        }

        let pass2 = format_yaml(&in_tree, &opts);
        if pass2 != in_tree {
            failures.push(format!(
                "[{id}] idempotency break:\n  pass1:\n{}\n  pass2:\n{}",
                indent_block(&in_tree),
                indent_block(&pass2),
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} of {} corpus cases failed:\n\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n\n"),
        );
    }
}

fn indent_block(text: &str) -> String {
    text.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
