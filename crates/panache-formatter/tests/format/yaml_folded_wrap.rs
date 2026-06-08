//! Folded (`>`) block-scalar wrapping (STYLE.md rule 15). Overlong body
//! lines of a *folded* scalar reflow to `line-width`; literal (`|`) and
//! quoted scalars never wrap, and short folded lines are never joined.

use panache_formatter::config::WrapMode;
use panache_formatter::{Config, format};

const LONG: &str = "Suboptimality versus wall-clock time on the Rosenbrock function (200-iteration cap, backend, AMD Ryzen 9 7900). Lower and further left is better; Basin is drawn with a heavier line.";

fn reflow80() -> Config {
    Config {
        wrap: Some(WrapMode::Reflow),
        line_width: 80,
        ..Default::default()
    }
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
    let body_lines = out
        .lines()
        .skip_while(|l| !l.starts_with("abstract:"))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .count();
    assert!(body_lines >= 2, "expected wrapped body, got:\n{out}");
    // Folding is loss-free: the wrapped words rejoin to the original.
    assert_eq!(folded_body_words(&out, "abstract"), LONG);
    // Idempotent.
    assert_eq!(format(&out, Some(reflow80()), None), out, "not idempotent");
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
fn short_folded_lines_are_not_joined() {
    let input = "---\nmsg: >\n  line one\n  line two\n---\n\n# Test\n";
    let out = format(input, Some(reflow80()), None);
    assert!(
        out.contains("msg: >\n  line one\n  line two\n"),
        "short folded lines must not be reflowed:\n{out}"
    );
}

#[test]
fn folded_not_wrapped_under_preserve() {
    let input = format!("---\nabstract: >\n  {LONG}\n---\n\n# Test\n");
    let preserve = Config {
        wrap: Some(WrapMode::Preserve),
        line_width: 80,
        ..Default::default()
    };
    let out = format(&input, Some(preserve), None);
    assert!(
        out.contains(&format!("  {LONG}")),
        "preserve mode must leave folded scalar unwrapped:\n{out}"
    );
}
