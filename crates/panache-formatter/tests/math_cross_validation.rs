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

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use panache_formatter::formatter::math::{MathContext, MathFormatOptions, format_math};
use pulldown_latex::config::RenderConfig;
use pulldown_latex::{Parser, Storage, push_mathml};

/// Fail the run if more than this fraction of (non-excluded) cases skip — a
/// guard against the oracle silently covering less and less of the corpus.
const MAX_SKIP_FRACTION: f64 = 0.40;

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

fn context_for(id: &str) -> MathContext {
    if id.starts_with("inline/") {
        MathContext::Inline
    } else {
        MathContext::Display
    }
}

fn format_opts(context: MathContext) -> MathFormatOptions {
    MathFormatOptions {
        enabled: true,
        math_indent: 2,
        line_width: 80,
        bookdown_equation_labels: false,
        context,
    }
}

/// Render math content to a MathML string, or return `None` if `pulldown-latex`
/// rejects it (a parse error). `push_mathml` itself renders parse errors as
/// inline error nodes rather than failing, so we first collect the event stream
/// into a `Result` to detect rejection, then render the validated events.
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

/// Collapse insignificant inter-tag whitespace so benign source reflow doesn't
/// register as a MathML difference. Significant attributes and text content are
/// preserved; only run-length whitespace and newlines between/inside tags are
/// normalized. We deliberately avoid a full XML parse — `pulldown-latex`'s
/// output is regular enough that textual normalization is sufficient and keeps
/// the harness dependency-light.
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
    // Drop whitespace that hugs tag boundaries (`> <` → `><`, `> x` → `>x`),
    // which carries no MathML meaning.
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

        // Macro-dependent cases need document-level macros the oracle can't see.
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
        let context = context_for(&id);

        let Some(before) = render_mathml(&input) else {
            // Oracle rejects the input itself → outside its scope, skip.
            skipped.push(id);
            continue;
        };

        let formatted = format_math(&input, &format_opts(context));
        let Some(after) = render_mathml(&formatted) else {
            // The formatter turned oracle-parseable input into something the
            // oracle rejects — that is a real formatter bug.
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
