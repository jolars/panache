//! Tier 2 semantic-equivalence oracle for the experimental math formatter
//! (Phase 4).
//!
//! Unlike YAML — where `pretty_yaml` is an *output* oracle and byte-exact parity
//! is the right assertion — math has **no output oracle**: latexindent does
//! `&`/`\\` alignment but no operator spacing, and KaTeX-class renderers
//! *render* TeX rather than reformatting it. Panache's eventual spacing /
//! line-breaking policy is its own invention. So instead of matching an oracle's
//! output, this harness asserts **invariance**: formatting must not change the
//! *rendered meaning*. We render both `x` and `format_math(x)` to MathML via
//! `pulldown-latex` and compare the normalized result. Because spacing and line
//! breaks are presentation that a renderer collapses, this survives the future
//! operator-spacing (Phase 5) and line-breaking (Phase 6) work.
//!
//! `pulldown-latex` is a **dev-only** oracle (see the `TEMPORARY` note in
//! `Cargo.toml`), never a runtime dependency. MathML — not HTML — is the
//! comparison surface: it encodes semantic atom structure (`<mo>`/`<mi>`/`<mn>`,
//! grouping) and omits pixel geometry, so benign source reflow renders
//! identically while a meaning change (e.g. an atom-class flip) shows up.
//!
//! Per-case **four-way rule**:
//!
//! - oracle rejects the **input** → skip (outside oracle scope), counted.
//! - oracle accepts input but rejects `format(input)` → **fail** (the formatter
//!   broke parseability).
//! - both accepted, normalized MathML differs → **fail** (meaning drift).
//! - both accepted, equal → pass.
//!
//! `macro_dependent/` cases are excluded (they need document-level macros the
//! oracle can't see); Tier 1 still covers them. The harness also **fails if the
//! skipped fraction exceeds [`MAX_SKIP_FRACTION`]**, so silent oracle-coverage
//! erosion stays visible.

use std::fs;
use std::path::PathBuf;

use panache_formatter::formatter::math::{MathContext, MathFormatOptions, format_math};
use pulldown_latex::config::RenderConfig;
use pulldown_latex::{Parser, Storage, push_mathml};

#[path = "common/math_corpus.rs"]
mod math_corpus;
use math_corpus::{discover_cases, read_preamble, signature_scope};

const MAX_SKIP_FRACTION: f64 = 0.40;

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/math_corpus")
}

fn context_for(id: &str) -> MathContext {
    if id.starts_with("inline/") {
        MathContext::Inline
    } else {
        MathContext::Display
    }
}

fn format_opts(
    context: MathContext,
    signature_scope: panache_parser::semantic::math::SignatureScope,
) -> MathFormatOptions {
    MathFormatOptions {
        enabled: true,
        math_indent: 2,
        line_width: 80,
        bookdown_equation_labels: false,
        context,
        signature_scope,
    }
}

fn render_mathml(tex: &str) -> Option<String> {
    let storage = Storage::new();
    let events = Parser::new(tex, &storage)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let mut out = String::new();
    push_mathml(
        &mut out,
        events.into_iter().map(Ok::<_, pulldown_latex::ParserError>),
        RenderConfig::default(),
    )
    .expect("writing MathML into a String never performs failing IO");
    Some(normalize_mathml(&out))
}

fn normalize_mathml(mathml: &str) -> String {
    let mut out = String::with_capacity(mathml.len());
    let mut prev_space = false;
    for ch in mathml.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.replace("> ", ">").replace(" <", "<")
}

#[test]
fn corpus_cross_validates_against_pulldown_latex() {
    let root = corpus_root();
    let cases = discover_cases(&root);
    assert!(
        !cases.is_empty(),
        "no cases discovered under {}",
        root.display()
    );

    let mut failures: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut considered = 0usize;
    for case in &cases {
        let id = case
            .strip_prefix(&root)
            .unwrap_or(case)
            .display()
            .to_string();

        if id.starts_with("macro_dependent/") {
            continue;
        }
        considered += 1;

        let input = match fs::read_to_string(case) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("[{id}] read error: {e}"));
                continue;
            }
        };
        let preamble = match read_preamble(case) {
            Ok(preamble) => preamble,
            Err(e) => {
                failures.push(format!("[{id}] preamble read error: {e}"));
                continue;
            }
        };
        let context = context_for(&id);

        let Some(before) = render_mathml(&input) else {
            skipped.push(id);
            continue;
        };

        let formatted = format_math(
            &input,
            &format_opts(context, signature_scope(preamble.as_deref())),
        )
        .unwrap_or_else(|| input.clone());
        let Some(after) = render_mathml(&formatted) else {
            failures.push(format!(
                "[{id}] format produced oracle-unparseable output:\n  input:\n{}\n  formatted:\n{}",
                indent_block(&input),
                indent_block(&formatted),
            ));
            continue;
        };

        if before != after {
            failures.push(format!(
                "[{id}] meaning drift (MathML changed):\n  input:\n{}\n  formatted:\n{}\n  \
                 mathml(before):\n{}\n  mathml(after):\n{}",
                indent_block(&input),
                indent_block(&formatted),
                indent_block(&before),
                indent_block(&after),
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} of {} considered cases failed cross-validation:\n\n{}",
            failures.len(),
            considered,
            failures.join("\n\n"),
        );
    }

    let skip_fraction = skipped.len() as f64 / considered as f64;
    assert!(
        skip_fraction <= MAX_SKIP_FRACTION,
        "oracle skipped {}/{} cases ({:.0}%) > {:.0}% threshold — coverage eroded; \
         either the corpus drifted toward oracle-unparseable inputs or the oracle regressed.\n\
         skipped: {}",
        skipped.len(),
        considered,
        skip_fraction * 100.0,
        MAX_SKIP_FRACTION * 100.0,
        skipped.join(", "),
    );
}

fn indent_block(text: &str) -> String {
    text.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Guards the oracle against being *vacuously* correct: a `normalize_mathml`
/// that collapsed too much (or a renderer that emitted constant output) would
/// make every case compare equal and the cross-validation would prove nothing.
/// So pin both directions — benign spacing must be invisible, a real meaning
/// change must not be.
#[test]
fn oracle_discriminates_meaning_from_spacing() {
    let tight = render_mathml("a+b").expect("a+b renders");
    let spaced = render_mathml("a + b").expect("a + b renders");
    assert_eq!(
        tight, spaced,
        "benign spacing changed normalized MathML — the invariance check would \
         produce false positives"
    );

    let different = render_mathml("a-b").expect("a-b renders");
    assert_ne!(
        tight, different,
        "a meaning change rendered identically — the invariance check is blind \
         and would never catch drift"
    );
}
