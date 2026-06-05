//! Tier 1 property harness for the experimental math formatter (Phase 4).
//!
//! For each `*.tex` case under
//! `crates/panache-formatter/tests/fixtures/math_corpus/`, asserts three
//! properties that need **no external oracle**:
//!
//! 1. **Idempotency.** `format_math(format_math(x)) == format_math(x)`.
//! 2. **Parser losslessness.** The structural math CST reconstructs the input
//!    byte-for-byte: `parse_math_report(x).green.text() == x`. (The corpus holds
//!    bare content with no host container prefixes, so `tree.text()` is the right
//!    surface — same shape as `debug format --checks losslessness`.)
//! 3. **Gate-off verbatim.** `format_math(x, { enabled: false, .. }) == x`, so a
//!    mis-wired call site can never change bytes when the experimental gate is
//!    off.
//!
//! This is the load-bearing correctness signal; the `pulldown-latex`
//! cross-validation in `math_cross_validation.rs` (Tier 2) is a secondary
//! meaning-drift alarm layered on top.
//!
//! The `MathContext` is chosen by subdirectory (see `fixtures/math_corpus/
//! README.md`): `inline/` → `Inline`; everything else → `Display`.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use panache_formatter::formatter::math::{MathContext, MathFormatOptions, format_math};
use panache_parser::parser::math::{MathParseOptions, parse_math_report};
use panache_parser::syntax::SyntaxNode;

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/math_corpus")
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
        } else if path.extension() == Some(OsStr::new("tex")) {
            out.push(path);
        }
    }
}

/// Subdirectory → layout context. `inline/` collapses whitespace on one line;
/// everything else gets the multi-line display treatment.
fn context_for(id: &str) -> MathContext {
    if id.starts_with("inline/") {
        MathContext::Inline
    } else {
        MathContext::Display
    }
}

fn opts(enabled: bool, context: MathContext) -> MathFormatOptions {
    MathFormatOptions {
        enabled,
        math_indent: 2,
        bookdown_equation_labels: false,
        context,
    }
}

#[test]
fn corpus_satisfies_math_formatter_properties() {
    let root = corpus_root();
    let cases = discover_cases(&root);
    assert!(
        !cases.is_empty(),
        "no cases discovered under {}",
        root.display()
    );

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
        let context = context_for(&id);

        // (2) Parser losslessness: the structural CST round-trips the input.
        let report = parse_math_report(
            &input,
            MathParseOptions {
                bookdown_equation_labels: false,
            },
        );
        let tree_text = SyntaxNode::new_root(report.green).text().to_string();
        if tree_text != input {
            failures.push(format!(
                "[{id}] losslessness break ({:+} bytes):\n  input:\n{}\n  tree:\n{}",
                tree_text.len() as i64 - input.len() as i64,
                indent_block(&input),
                indent_block(&tree_text),
            ));
            continue;
        }

        // (3) Gate-off verbatim: the off path must never change bytes.
        let off = format_math(&input, &opts(false, context));
        if off != input {
            failures.push(format!(
                "[{id}] gate-off changed bytes:\n  input:\n{}\n  off:\n{}",
                indent_block(&input),
                indent_block(&off),
            ));
            continue;
        }

        // (1) Idempotency: a second pass is a no-op.
        let once = format_math(&input, &opts(true, context));
        let twice = format_math(&once, &opts(true, context));
        if once != twice {
            failures.push(format!(
                "[{id}] idempotency break:\n  pass1:\n{}\n  pass2:\n{}",
                indent_block(&once),
                indent_block(&twice),
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
