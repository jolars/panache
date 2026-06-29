//! Folding long double-quoted scalars into `>-` block scalars (STYLE.md
//! rule 17). When a wrap mode is active and a double-quoted scalar
//! overflows `line-width`, the formatter rewrites it as a folded block
//! scalar so its prose reflows — but only when the conversion is
//! completely value-preserving. Content that folding can't round-trip
//! (escapes, leading/trailing whitespace, control chars, multi-line
//! quoted scalars) is left quoted. Rule 17 keys off the `"` prefix, so a
//! single-quoted scalar only folds when rule 3 first normalizes it to
//! double-quoted (simple content); one that keeps its single quotes (it
//! contains an apostrophe) is left untouched.

use panache_formatter::config::WrapMode;
use panache_formatter::{Config, format};

const LONG: &str = "This is a very long description that happens to exceed the line-length cap by a fair bit, and should be wrapped.";

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

/// Body lines (2-space indented) immediately following the `key:` line.
fn body_lines(out: &str, key: &str) -> Vec<String> {
    out.lines()
        .skip_while(|l| !l.starts_with(&format!("{key}:")))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .map(str::to_string)
        .collect()
}

/// Reconstruct a folded scalar's value: strip the base indent from each
/// body line and join with a single space (the fold). Interior spaces
/// are preserved verbatim, so this round-trips a single-paragraph folded
/// scalar back to its original value.
fn fold_body(out: &str, key: &str) -> String {
    body_lines(out, key)
        .iter()
        .map(|l| l.strip_prefix("  ").unwrap_or(l).to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn long_double_quoted_becomes_folded() {
    let input = format!("---\ndescription: \"{LONG}\"\n---\n\n# Test\n");
    let out = format(&input, Some(reflow80()), None);

    assert!(
        out.contains("\ndescription: >-\n"),
        "expected `>-` header:\n{out}"
    );
    for line in out.lines() {
        assert!(
            line.chars().count() <= 80,
            "line over width: {line:?}\n{out}"
        );
    }
    assert!(
        body_lines(&out, "description").len() >= 2,
        "expected wrapped body:\n{out}"
    );
    // The conversion is loss-free: the folded body rejoins to the value.
    assert_eq!(
        fold_body(&out, "description"),
        LONG,
        "value changed:\n{out}"
    );
    assert_eq!(format(&out, Some(reflow80()), None), out, "not idempotent");
}

#[test]
fn next_line_double_quoted_folds_onto_key_line() {
    // A double-quoted value written on its *own* line (indented under the
    // key) must fold to the same shape as the same-line case: the `>-`
    // indicator hoisted onto the key line. Emitting `>-` on its own
    // indented line is not a fixpoint of the indent pass (it relocates the
    // indicator on a second format), so it breaks idempotency (issue
    // #400).
    let input = format!("---\ndescription:\n  \"{LONG}\"\n---\n\n# Test\n");
    let out = format(&input, Some(reflow80()), None);

    assert!(
        out.contains("\ndescription: >-\n"),
        "expected `>-` hoisted onto the key line:\n{out}"
    );
    assert!(
        !out.contains("description:\n  >-") && !out.contains("description:\n>-"),
        "indicator must not stay on its own line:\n{out}"
    );
    assert_eq!(
        fold_body(&out, "description"),
        LONG,
        "value changed:\n{out}"
    );
    assert_eq!(format(&out, Some(reflow80()), None), out, "not idempotent");
}

#[test]
fn multi_space_run_is_preserved_when_folded() {
    // A run of >=2 spaces must never sit at a line break (a fold would
    // collapse it to one space). It stays verbatim mid-line.
    const V: &str = "Column   alignment is preserved here and this caption is certainly long enough to overflow.";
    let input = format!("---\ntbl-cap: \"{V}\"\n---\n\n# Test\n");
    let out = format(&input, Some(reflow80()), None);

    assert!(
        out.contains("\ntbl-cap: >-\n"),
        "expected `>-` header:\n{out}"
    );
    assert_eq!(
        fold_body(&out, "tbl-cap"),
        V,
        "3-space run must survive folding:\n{out}"
    );
    assert_eq!(format(&out, Some(reflow80()), None), out, "not idempotent");
}

#[test]
fn short_double_quoted_stays_quoted() {
    let input = "---\ntitle: \"A Document\"\n---\n\n# Test\n";
    let out = format(input, Some(reflow80()), None);
    assert!(
        out.contains("title: \"A Document\""),
        "short quoted value should be untouched:\n{out}"
    );
}

#[test]
fn escapes_keep_scalar_quoted() {
    // A `\n` escape resolves to a newline that folding can't represent;
    // leave the scalar quoted even though it overflows.
    let v = "This description deliberately embeds a newline escape \\n right here and is also long enough to overflow eighty columns.";
    let input = format!("---\ndescription: \"{v}\"\n---\n\n# Test\n");
    let out = format(&input, Some(reflow80()), None);
    assert!(
        out.contains("description: \""),
        "escaped value must stay quoted:\n{out}"
    );
    assert!(
        !out.contains(">-"),
        "must not fold an escaped value:\n{out}"
    );
}

#[test]
fn leading_space_keeps_scalar_quoted() {
    // A folded scalar strips leading whitespace (and a leading space
    // makes the line "more-indented" = literal), so this can't fold.
    let v = " this description begins with a space that folding would strip, and it is long enough to overflow the cap.";
    let input = format!("---\ndescription: \"{v}\"\n---\n\n# Test\n");
    let out = format(&input, Some(reflow80()), None);
    assert!(
        out.contains("description: \""),
        "leading-space value must stay quoted:\n{out}"
    );
}

#[test]
fn simple_single_quoted_folds_via_double_quoted() {
    // Rule 3 rewrites a simple single-quoted scalar to double-quoted,
    // which rule 17 then folds.
    let v = "This single quoted caption has no apostrophe so it normalizes to double quotes and folds when it overflows.";
    let input = format!("---\ndescription: '{v}'\n---\n\n# Test\n");
    let out = format(&input, Some(reflow80()), None);
    assert!(
        out.contains("\ndescription: >-\n"),
        "simple single-quoted should fold:\n{out}"
    );
    assert_eq!(fold_body(&out, "description"), v, "value changed:\n{out}");
    assert_eq!(format(&out, Some(reflow80()), None), out, "not idempotent");
}

#[test]
fn single_quoted_with_apostrophe_stays_quoted() {
    // Rule 3 preserves a single-quoted scalar whose content holds a `'`,
    // so rule 17 (which only matches `"`) never sees it.
    let input = "---\ndescription: 'A long single-quoted caption that won''t fold because it keeps its quotes for the apostrophe and is long'\n---\n\n# Test\n";
    let out = format(input, Some(reflow80()), None);
    assert!(
        out.contains("description: 'A long single-quoted caption that won''t fold"),
        "single-quoted-with-apostrophe should stay quoted:\n{out}"
    );
    assert!(!out.contains(">-"), "must not fold:\n{out}");
}

#[test]
fn preserve_mode_keeps_scalar_quoted() {
    let input = format!("---\ndescription: \"{LONG}\"\n---\n\n# Test\n");
    let out = format(&input, Some(cfg(WrapMode::Preserve)), None);
    assert!(
        out.contains("description: \""),
        "preserve mode must leave the scalar quoted:\n{out}"
    );
}

#[test]
fn sentence_mode_folds_one_sentence_per_line() {
    let v = "First sentence is reasonably long here. Second sentence follows along. Third one closes it out nicely indeed.";
    let input = format!("---\ndescription: \"{v}\"\n---\n\n# Test\n");
    let out = format(&input, Some(cfg(WrapMode::Sentence)), None);

    assert!(
        out.contains("\ndescription: >-\n"),
        "expected fold in sentence mode:\n{out}"
    );
    assert_eq!(
        body_lines(&out, "description"),
        vec![
            "  First sentence is reasonably long here.".to_string(),
            "  Second sentence follows along.".to_string(),
            "  Third one closes it out nicely indeed.".to_string(),
        ],
        "one sentence per line:\n{out}"
    );
    assert_eq!(
        format(&out, Some(cfg(WrapMode::Sentence)), None),
        out,
        "not idempotent"
    );
}
