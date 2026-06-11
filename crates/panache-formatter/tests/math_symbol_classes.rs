//! Tier 3 symbol → atom-class fixture for the experimental math formatter
//! (Phase 5b).
//!
//! Where Tier 2 (`math_cross_validation.rs`) asserts *render invariance* — the
//! formatter must not change the MathML a renderer produces — this tier pins the
//! **static interpretation table** itself: `formatter::math::operators`, the
//! hand-curated map from a TeX symbol to its [`AtomClass`] that drives operator
//! spacing (Phase 5) and the upcoming semantic line-breaking (Phase 6). Nothing
//! else guards that table; a typo'd class or a dropped command would pass every
//! other test today.
//!
//! The vendored fixture (`fixtures/math_symbol_classes/symbol-classes.tsv`) is an
//! *independent* enumeration of the table's surface, so it catches drift in both
//! directions: a changed class, and a deleted entry (the row's table lookup then
//! fails). Each row carries the expected [`AtomClass`] **and** the expected
//! `pulldown-latex` parser Event class, so the fixture is itself cross-validated
//! against a real LaTeX parser rather than asserting our own table against
//! itself.
//!
//! `pulldown-latex` is a **dev-only** oracle (see the `TEMPORARY` note in
//! `Cargo.toml`), never a runtime dependency.
//!
//! Two recorded divergences (see the fixture comments + README): `\lim` is `Op`
//! for us but a `Function` to pulldown (spacing-equivalent), and `\asymp` is the
//! AMS-correct `Rel` for us but a binary op to pulldown (an oracle quirk we
//! deliberately do *not* follow). They are recorded, not silently tolerated — the
//! oracle column records pulldown's view, the class column ours.

use std::fs;
use std::path::PathBuf;

use panache_formatter::formatter::math::operators::{self, AtomClass};
use panache_parser::parser::math::{MathParseOptions, parse_math_content};
use panache_parser::syntax::{SyntaxKind, SyntaxNode};
use pulldown_latex::event::{Content, DelimiterType, Event};
use pulldown_latex::{Parser, Storage};

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/math_symbol_classes/symbol-classes.tsv")
}

/// The `pulldown-latex` `Content` class we expect for a symbol's probe — a
/// coarse projection of [`Content`] onto the distinctions the table cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Oracle {
    BinOp,
    Relation,
    LargeOp,
    Function,
    Open,
    Close,
    Punct,
    Ordinary,
    /// The symbol emits no probeable standalone `Content` event (delimiter
    /// framing like `\left`/`\right`, or a multi-argument visual like `\frac`).
    Skip,
}

struct Row {
    token: String,
    atom_class: AtomClass,
    oracle: Oracle,
    line: usize,
}

fn parse_atom_class(s: &str, line: usize) -> AtomClass {
    match s {
        "Ord" => AtomClass::Ord,
        "Bin" => AtomClass::Bin,
        "Rel" => AtomClass::Rel,
        "Open" => AtomClass::Open,
        "Close" => AtomClass::Close,
        "Punct" => AtomClass::Punct,
        "Op" => AtomClass::Op,
        other => panic!("line {line}: unknown atom_class token {other:?}"),
    }
}

fn parse_oracle(s: &str, line: usize) -> Oracle {
    match s {
        "binop" => Oracle::BinOp,
        "relation" => Oracle::Relation,
        "largeop" => Oracle::LargeOp,
        "function" => Oracle::Function,
        "open" => Oracle::Open,
        "close" => Oracle::Close,
        "punct" => Oracle::Punct,
        "ordinary" => Oracle::Ordinary,
        "skip" => Oracle::Skip,
        other => panic!("line {line}: unknown oracle token {other:?}"),
    }
}

fn load_rows() -> Vec<Row> {
    let path = manifest_path();
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut rows = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = raw.split('\t').collect();
        assert!(
            cols.len() == 3,
            "line {line}: expected 3 tab-separated columns, got {}: {raw:?}",
            cols.len()
        );
        rows.push(Row {
            token: cols[0].to_string(),
            atom_class: parse_atom_class(cols[1], line),
            oracle: parse_oracle(cols[2], line),
            line,
        });
    }
    rows
}

/// The table's class for a char token. `+ - * = < >` go through
/// [`operators::classify_operator`]. Delimiters/punctuation (`( ) [ ] , ;`) take
/// the production path: the parser tokenizes the char into a dedicated `MATH_*`
/// kind, and [`operators::delimiter_class`] maps that kind to an [`AtomClass`].
/// This pins both halves at once — the parser's char→kind grouping and the
/// formatter's kind→class read — against the vendored intent and the oracle.
fn char_class(token: &str) -> AtomClass {
    match token {
        "+" | "-" | "*" | "=" | "<" | ">" => operators::classify_operator(token),
        _ => {
            let kind = sole_token_kind(token);
            operators::delimiter_class(kind)
                .unwrap_or_else(|| panic!("char {token:?} parsed as {kind:?}, not a delimiter"))
        }
    }
}

/// Tokenize a single math char and return its sole token kind (the parser owns
/// the char→kind grouping for delimiters/punctuation).
fn sole_token_kind(token: &str) -> SyntaxKind {
    let root = SyntaxNode::new_root(parse_math_content(token, MathParseOptions::default()));
    let kinds: Vec<SyntaxKind> = root
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .map(|t| t.kind())
        .collect();
    assert_eq!(
        kinds.len(),
        1,
        "expected a single math token for {token:?}, got {kinds:?}"
    );
    kinds[0]
}

/// Project a `pulldown-latex` `Content` event onto an [`Oracle`] class.
fn classify_content(content: Content<'_>) -> Oracle {
    match content {
        Content::BinaryOp { .. } => Oracle::BinOp,
        Content::Relation { .. } => Oracle::Relation,
        Content::LargeOp { .. } => Oracle::LargeOp,
        Content::Function(_) => Oracle::Function,
        Content::Delimiter {
            ty: DelimiterType::Open,
            ..
        } => Oracle::Open,
        Content::Delimiter {
            ty: DelimiterType::Close,
            ..
        } => Oracle::Close,
        // `\middle` fences — no fixture row probes one; surface it as Skip rather
        // than mislabel it Open/Close.
        Content::Delimiter {
            ty: DelimiterType::Fence,
            ..
        } => Oracle::Skip,
        Content::Punctuation(_) => Oracle::Punct,
        Content::Ordinary { .. } | Content::Number(_) | Content::Text(_) => Oracle::Ordinary,
    }
}

/// Parse the probe `a <token> b` with `pulldown-latex` and return the class of
/// the single operator event between the two `a`/`b` operands.
fn oracle_class(token: &str) -> Result<Oracle, String> {
    let probe = format!("a {token} b");
    let storage = Storage::new();
    let events = Parser::new(&probe, &storage)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("pulldown rejected probe {probe:?}: {e:?}"))?;

    let mut ops: Vec<Oracle> = Vec::new();
    for ev in events {
        let Event::Content(content) = ev else {
            continue;
        };
        if let Content::Ordinary {
            content: 'a' | 'b', ..
        } = content
        {
            continue; // the probe operands
        }
        ops.push(classify_content(content));
    }

    match ops.as_slice() {
        [one] => Ok(*one),
        _ => Err(format!(
            "probe {probe:?} produced {} operator events, expected exactly 1: {ops:?}",
            ops.len()
        )),
    }
}

/// Assertion 1: every fixture row's class matches the live interpretation table.
/// Catches a retyped class and (via the `Ord`/`None` controls and the lookup
/// itself) a deleted command.
#[test]
fn table_matches_vendored_fixture() {
    let rows = load_rows();
    let mut failures: Vec<String> = Vec::new();

    for row in &rows {
        if let Some(name) = row.token.strip_prefix('\\') {
            // `command_class` never returns `Some(Ord)`; an `Ord` fixture row is
            // a control asserting the command is absent (defaults to Ord).
            let want = if row.atom_class == AtomClass::Ord {
                None
            } else {
                Some(row.atom_class)
            };
            let got = operators::command_class(name);
            if got != want {
                failures.push(format!(
                    "line {}: `\\{name}`: command_class = {got:?}, fixture expects {want:?}",
                    row.line
                ));
            }
        } else {
            let got = char_class(&row.token);
            if got != row.atom_class {
                failures.push(format!(
                    "line {}: char {:?}: class = {got:?}, fixture expects {:?}",
                    row.line, row.token, row.atom_class
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} fixture row(s) disagree with `operators` — the table drifted from the \
         vendored intent:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

/// Assertion 2: the class each row *records* is what `pulldown-latex` actually
/// emits. Grounds the vendored intent in a real LaTeX parser; a non-divergence
/// mismatch here is a real table bug, not a fixture nit.
#[test]
fn oracle_agrees_with_fixture() {
    let rows = load_rows();
    let mut failures: Vec<String> = Vec::new();

    for row in &rows {
        if row.oracle == Oracle::Skip {
            continue;
        }
        match oracle_class(&row.token) {
            Ok(got) if got == row.oracle => {}
            Ok(got) => failures.push(format!(
                "line {}: {:?}: pulldown emitted {got:?}, fixture records {:?}",
                row.line, row.token, row.oracle
            )),
            Err(e) => failures.push(format!("line {}: {:?}: {e}", row.line, row.token)),
        }
    }

    assert!(
        failures.is_empty(),
        "{} symbol(s) disagree with the pulldown-latex oracle (a real table bug \
         unless recorded as a divergence in the fixture):\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

/// Guards against a *vacuously* passing oracle: if `classify_content` collapsed
/// every symbol to one class, [`oracle_agrees_with_fixture`] would prove nothing.
/// Pin that binop and relation stay distinguishable.
#[test]
fn oracle_distinguishes_atom_classes() {
    let plus = oracle_class("+").expect("`+` probes");
    let eq = oracle_class("=").expect("`=` probes");
    assert_eq!(plus, Oracle::BinOp);
    assert_eq!(eq, Oracle::Relation);
    assert_ne!(
        plus, eq,
        "oracle collapsed binop and relation to one class — the cross-check is blind"
    );
}

/// Guards against the vendored set being silently gutted: pin a coverage floor
/// and require every spacing-relevant class to appear.
#[test]
fn fixture_pins_table_coverage() {
    let rows = load_rows();

    let command_rows = rows.iter().filter(|r| r.token.starts_with('\\')).count();
    assert!(
        command_rows >= 65,
        "expected at least 65 command rows pinning `command_class`, found {command_rows}"
    );

    for class in [
        AtomClass::Ord,
        AtomClass::Bin,
        AtomClass::Rel,
        AtomClass::Open,
        AtomClass::Close,
        AtomClass::Punct,
        AtomClass::Op,
    ] {
        assert!(
            rows.iter().any(|r| r.atom_class == class),
            "no fixture row covers {class:?}"
        );
    }
}
