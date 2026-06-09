//! Folded (`>`) block-scalar wrapping (STYLE.md rule 15). A folded
//! scalar's prose reflows per the active wrap mode — short lines are
//! joined and the paragraph is re-laid-out, which folding makes
//! loss-free. Literal (`|`) and quoted scalars never wrap; `preserve`
//! leaves the author's line breaks untouched.

use panache_formatter::config::WrapMode;
use panache_formatter::{Config, format};

const LONG: &str = "Suboptimality versus wall-clock time on the Rosenbrock function (200-iteration cap, backend, AMD Ryzen 9 7900). Lower and further left is better; Basin is drawn with a heavier line.";

fn cfg(wrap: WrapMode) -> Config {
    Config {
        wrap: Some(wrap),
        line_width: 80,
        ..Default::default()
    }
}

fn reflow80() -> Config {
    cfg(WrapMode::Reflow)
}

/// Body lines (2-space indented) immediately following the `key: >` line.
fn folded_body_words(out: &str, key: &str) -> String {
    out.lines()
        .skip_while(|l| !l.starts_with(&format!("{key}:")))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .flat_map(|l| l.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}

fn body_lines(out: &str, key: &str) -> Vec<String> {
    out.lines()
        .skip_while(|l| !l.starts_with(&format!("{key}:")))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .map(str::to_string)
        .collect()
}

#[test]
fn folded_scalar_wraps_long_line() {
    let input = format!("---\nabstract: >\n  {LONG}\n---\n\n# Test\n");
    let out = format(&input, Some(reflow80()), None);

    assert!(out.contains("\nabstract: >\n"), "header preserved:\n{out}");
    for line in out.lines() {
        assert!(
            line.chars().count() <= 80,
            "line over width: {line:?}\n{out}"
        );
    }
    assert!(
        body_lines(&out, "abstract").len() >= 2,
        "expected wrapped body:\n{out}"
    );
    // Folding is loss-free: the wrapped words rejoin to the original.
    assert_eq!(folded_body_words(&out, "abstract"), LONG);
    // Idempotent.
    assert_eq!(format(&out, Some(reflow80()), None), out, "not idempotent");
}

#[test]
fn reflow_joins_short_lines_and_refills() {
    // The motivating case: a folded paragraph hand-wrapped at uneven
    // widths is rejoined and greedily refilled to the line width.
    let input = "---\ndescription: >\n  This page presents performance benchmarks for Panache, comparing its formatting\n  and linting speed against popular alternatives like Prettier, Pandoc, rumdl.\n---\n\n# Test\n";
    let out = format(input, Some(reflow80()), None);

    for line in out.lines() {
        assert!(
            line.chars().count() <= 80,
            "line over width: {line:?}\n{out}"
        );
    }
    // The orphaned word `formatting` is rejoined, not stranded on its own line.
    assert!(
        !out.lines().any(|l| l.trim() == "formatting"),
        "short line should be rejoined, not stranded:\n{out}"
    );
    let words = "This page presents performance benchmarks for Panache, comparing its formatting and linting speed against popular alternatives like Prettier, Pandoc, rumdl.";
    assert_eq!(folded_body_words(&out, "description"), words);
    assert_eq!(format(&out, Some(reflow80()), None), out, "not idempotent");
}

#[test]
fn short_folded_lines_are_joined_under_reflow() {
    // Rule 15 (revised): short folded lines now join when they fit.
    let input = "---\nmsg: >\n  line one\n  line two\n---\n\n# Test\n";
    let out = format(input, Some(reflow80()), None);
    assert!(
        out.contains("msg: >\n  line one line two\n"),
        "short folded lines should join under reflow:\n{out}"
    );
    assert_eq!(format(&out, Some(reflow80()), None), out, "not idempotent");
}

#[test]
fn sentence_mode_breaks_one_sentence_per_line() {
    let input = "---\nmsg: >\n  First sentence here. Second sentence follows. Third one too.\n---\n\n# Test\n";
    let out = format(input, Some(cfg(WrapMode::Sentence)), None);
    assert_eq!(
        body_lines(&out, "msg"),
        vec![
            "  First sentence here.".to_string(),
            "  Second sentence follows.".to_string(),
            "  Third one too.".to_string(),
        ],
        "one sentence per line:\n{out}"
    );
    // Folding rejoins to the original prose.
    assert_eq!(
        folded_body_words(&out, "msg"),
        "First sentence here. Second sentence follows. Third one too."
    );
    assert_eq!(
        format(&out, Some(cfg(WrapMode::Sentence)), None),
        out,
        "not idempotent"
    );
}

#[test]
fn semantic_mode_preserves_breaks_and_splits_sentences() {
    // Two author lines; the first carries two sentences. Semantic keeps
    // the author break AND splits the first line's two sentences.
    let input = "---\nmsg: >\n  First sentence. Second sentence.\n  A third on its own line.\n---\n\n# Test\n";
    let out = format(input, Some(cfg(WrapMode::Semantic)), None);
    assert_eq!(
        body_lines(&out, "msg"),
        vec![
            "  First sentence.".to_string(),
            "  Second sentence.".to_string(),
            "  A third on its own line.".to_string(),
        ],
        "semantic line breaks:\n{out}"
    );
    assert_eq!(
        format(&out, Some(cfg(WrapMode::Semantic)), None),
        out,
        "not idempotent"
    );
}

#[test]
fn literal_scalar_never_wraps() {
    let input = format!("---\nscript: |\n  {LONG}\n---\n\n# Test\n");
    let out = format(&input, Some(reflow80()), None);
    assert!(
        out.contains(&format!("  {LONG}")),
        "literal body must stay verbatim:\n{out}"
    );
}

#[test]
fn folded_strip_and_keep_preserve_header_and_wrap() {
    for header in [">-", ">+"] {
        let input = format!("---\nabstract: {header}\n  {LONG}\n---\n\n# Test\n");
        let out = format(&input, Some(reflow80()), None);
        assert!(
            out.contains(&format!("abstract: {header}\n")),
            "header {header} preserved:\n{out}"
        );
        for line in out.lines() {
            assert!(
                line.chars().count() <= 80,
                "line over width: {line:?}\n{out}"
            );
        }
        assert_eq!(folded_body_words(&out, "abstract"), LONG);
    }
}

#[test]
fn folded_with_indentation_indicator_is_left_alone() {
    // Explicit indentation indicator (`>2`) is out of scope for rule 15.
    let input = format!("---\nabstract: >2\n  {LONG}\n---\n\n# Test\n");
    let out = format(&input, Some(reflow80()), None);
    assert!(
        out.contains(&format!("  {LONG}")),
        "indentation-indicator scalar must stay verbatim:\n{out}"
    );
}

#[test]
fn blank_line_separates_paragraphs() {
    // A blank line inside a folded scalar is folding-significant (it
    // folds to a newline); reflow must not merge across it.
    let input = "---\nmsg: >\n  one two\n  three four\n\n  five six\n---\n\n# Test\n";
    let out = format(input, Some(reflow80()), None);
    assert!(
        out.contains("msg: >\n  one two three four\n\n  five six\n"),
        "blank line must separate paragraphs:\n{out}"
    );
    assert_eq!(format(&out, Some(reflow80()), None), out, "not idempotent");
}

#[test]
fn folded_not_wrapped_under_preserve() {
    let input = "---\nmsg: >\n  line one\n  line two\n---\n\n# Test\n";
    let out = format(input, Some(cfg(WrapMode::Preserve)), None);
    assert!(
        out.contains("msg: >\n  line one\n  line two\n"),
        "preserve mode must leave folded scalar line breaks untouched:\n{out}"
    );
}
