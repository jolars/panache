//! Byte-exact Badness output oracle for Panache's experimental math formatter.
//!
//! Badness formats complete LaTeX documents, whereas Panache's math entry point
//! receives a delimiter-free body. These test-only adapters place the same body
//! in controlled inline, display, and environment contexts, then mechanically
//! remove the wrappers. They do not parse or normalize the resulting TeX.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use badness_formatter::{FormatStyle, LineEnding, MathWrap, formatter::format_with_style};
use panache_formatter::formatter::math::{MathContext, MathFormatOptions, format_math};
use panache_parser::parser::math::{MathParseOptions, parse_math_content};
use panache_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

#[path = "common/math_corpus.rs"]
mod math_corpus;
use math_corpus::{discover_cases, read_preamble, signature_scope};

const REPORT_REL: &str = "tests/math_badness/report.txt";
const START_SENTINEL: &str = "% panache-math-oracle-start";
const END_SENTINEL: &str = "% panache-math-oracle-end";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum OracleContext {
    Inline,
    Display,
    Environment,
}

impl OracleContext {
    const ALL: [Self; 3] = [Self::Inline, Self::Display, Self::Environment];

    fn wrapper(self, body: &str) -> (String, String, String) {
        match self {
            Self::Inline => (format!("${body}$\n"), "$".into(), "$\n".into()),
            Self::Display => {
                let suffix = if body.ends_with('\n') {
                    "\\]\n"
                } else {
                    "\n\\]\n"
                };
                (
                    format!("\\[\n{body}{suffix}"),
                    "\\[\n".into(),
                    suffix.into(),
                )
            }
            Self::Environment => {
                let suffix = if body.ends_with('\n') {
                    "\\end{aligned}\n"
                } else {
                    "\n\\end{aligned}\n"
                };
                (
                    format!("\\begin{{aligned}}\n{body}{suffix}"),
                    "\\begin{aligned}\n".into(),
                    suffix.into(),
                )
            }
        }
    }

    fn panache_context(self) -> MathContext {
        match self {
            Self::Inline => MathContext::Inline,
            Self::Display => MathContext::Display,
            Self::Environment => MathContext::EnvironmentBody,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Display => "display",
            Self::Environment => "environment",
        }
    }
}

fn badness_body(body: &str, context: OracleContext) -> Result<String, String> {
    badness_body_with_width(body, context, 80)
}

fn badness_body_with_width(
    body: &str,
    context: OracleContext,
    line_width: usize,
) -> Result<String, String> {
    badness_body_with_preamble_and_width(body, None, context, line_width)
}

fn badness_body_with_preamble(
    body: &str,
    preamble: Option<&str>,
    context: OracleContext,
) -> Result<String, String> {
    badness_body_with_preamble_and_width(body, preamble, context, 80)
}

fn badness_body_with_preamble_and_width(
    body: &str,
    preamble: Option<&str>,
    context: OracleContext,
    line_width: usize,
) -> Result<String, String> {
    let (wrapped, prefix, suffix) = context.wrapper(body);
    let controlled = if let Some(preamble) = preamble {
        let mut controlled = String::new();
        controlled.push_str(preamble);
        if !preamble.ends_with('\n') {
            controlled.push('\n');
        }
        writeln!(controlled, "{START_SENTINEL}").unwrap();
        controlled.push_str(&wrapped);
        writeln!(controlled, "{END_SENTINEL}").unwrap();
        controlled
    } else {
        wrapped
    };
    let formatted = format_with_style(
        &controlled,
        FormatStyle {
            line_width,
            indent_width: 2,
            math_wrap: MathWrap::Break,
            line_ending: LineEnding::Lf,
            ..FormatStyle::default()
        },
    )
    .map_err(|error| format!("Badness rejected {context:?} wrapper: {error}"))?;

    let formatted_wrapper = if preamble.is_some() {
        let start = format!("{START_SENTINEL}\n");
        let end = format!("{END_SENTINEL}\n");
        let (_, after_start) = formatted.split_once(&start).ok_or_else(|| {
            format!("Badness removed the controlled {context:?} start sentinel:\n{formatted:?}")
        })?;
        if after_start.contains(&start) {
            return Err(format!(
                "Badness duplicated the controlled {context:?} start sentinel:\n{formatted:?}"
            ));
        }
        let (formatted_wrapper, after_end) = after_start.split_once(&end).ok_or_else(|| {
            format!("Badness removed the controlled {context:?} end sentinel:\n{formatted:?}")
        })?;
        if after_end.contains(&end) {
            return Err(format!(
                "Badness duplicated the controlled {context:?} end sentinel:\n{formatted:?}"
            ));
        }
        formatted_wrapper
    } else {
        formatted.as_str()
    };

    let body = formatted_wrapper
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_suffix(&suffix))
        .ok_or_else(|| {
            format!(
                "Badness changed the controlled {context:?} wrapper shape:\n{formatted_wrapper:?}"
            )
        })?;
    Ok(body.to_owned())
}

fn panache_body(body: &str, context: OracleContext) -> Result<String, String> {
    panache_body_with_preamble(body, None, context)
}

fn panache_body_with_preamble(
    body: &str,
    preamble: Option<&str>,
    context: OracleContext,
) -> Result<String, String> {
    format_math(
        body,
        &MathFormatOptions {
            enabled: true,
            math_indent: 2,
            line_width: 80,
            bookdown_equation_labels: false,
            context: context.panache_context(),
            signature_scope: signature_scope(preamble),
        },
    )
    .ok_or_else(|| format!("Panache declined {context:?} body"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Classification {
    Parity,
    ControlledWrapperRejection {
        reason: String,
    },
    Divergence {
        badness: String,
        panache: Option<String>,
    },
}

fn classify_result(badness: Result<String, String>, panache: Option<String>) -> Classification {
    match badness {
        Err(reason) => Classification::ControlledWrapperRejection { reason },
        Ok(badness) if panache.as_deref() == Some(badness.as_str()) => Classification::Parity,
        Ok(badness) => Classification::Divergence { badness, panache },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BaselineRecord {
    id: String,
    context: OracleContext,
    input: String,
    classification: Classification,
    first_slice_candidate: bool,
}

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/math_corpus")
}

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn normalized_id(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("corpus case outside root")
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_flat_inline_candidate(body: &str) -> bool {
    let root = SyntaxNode::new_root(parse_math_content(body, MathParseOptions::default()));
    root.children_with_tokens().all(|element| match element {
        NodeOrToken::Token(token) => matches!(
            token.kind(),
            SyntaxKind::MATH_WORD | SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE
        ),
        NodeOrToken::Node(_) => false,
    })
}

fn collect_baseline_records() -> (usize, Vec<BaselineRecord>) {
    let root = corpus_root();
    let cases = discover_cases(&root);
    let mut records = Vec::with_capacity(cases.len() * OracleContext::ALL.len());
    for path in &cases {
        let id = normalized_id(&root, path);
        let input = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("failed to read `{id}`: {error}"));
        let preamble = read_preamble(path)
            .unwrap_or_else(|error| panic!("failed to read preamble for `{id}`: {error}"));
        let flat = is_flat_inline_candidate(&input);
        for context in OracleContext::ALL {
            let badness = badness_body_with_preamble(&input, preamble.as_deref(), context);
            let panache = panache_body_with_preamble(&input, preamble.as_deref(), context).ok();
            records.push(BaselineRecord {
                id: id.clone(),
                context,
                input: input.clone(),
                classification: classify_result(badness, panache),
                first_slice_candidate: context == OracleContext::Inline && flat,
            });
        }
    }
    (cases.len(), records)
}

fn render_report(mut records: Vec<BaselineRecord>, corpus_count: usize) -> String {
    records.sort_by(|left, right| (&left.id, left.context).cmp(&(&right.id, right.context)));
    let total = records.len();
    let parity = records
        .iter()
        .filter(|record| matches!(record.classification, Classification::Parity))
        .count();
    let rejected = records
        .iter()
        .filter(|record| {
            matches!(
                record.classification,
                Classification::ControlledWrapperRejection { .. }
            )
        })
        .count();
    let divergent = total - parity - rejected;
    let mut report = String::new();
    writeln!(report, "Badness math formatter parity baseline").unwrap();
    writeln!(report, "Oracle: badness-formatter =0.6.0").unwrap();
    writeln!(report, "Corpus: tests/fixtures/math_corpus").unwrap();
    writeln!(report, "Cases: {corpus_count}").unwrap();
    writeln!(report, "Context runs: {total}").unwrap();
    writeln!(report, "Parity: {parity} / {total}").unwrap();
    writeln!(report, "Controlled wrapper rejection: {rejected} / {total}").unwrap();
    writeln!(report, "Divergent: {divergent} / {total}\n").unwrap();
    writeln!(report, "Regenerate with:").unwrap();
    writeln!(
        report,
        "  cargo test -p panache-formatter --test math_badness_oracle math_badness_full_report -- --ignored --nocapture\n"
    )
    .unwrap();
    writeln!(report, "Selected first migration slice:").unwrap();
    writeln!(
        report,
        "  Inline bodies containing only MATH_WORD, MATH_SPACE, and MATH_NEWLINE."
    )
    .unwrap();
    writeln!(
        report,
        "  Every other shape remains on the conservative legacy fallback.\n"
    )
    .unwrap();
    writeln!(report, "=== Counts by context ===").unwrap();
    for context in OracleContext::ALL {
        let in_context = records.iter().filter(|record| record.context == context);
        let context_parity = in_context
            .clone()
            .filter(|record| matches!(record.classification, Classification::Parity))
            .count();
        let context_rejected = in_context
            .clone()
            .filter(|record| {
                matches!(
                    record.classification,
                    Classification::ControlledWrapperRejection { .. }
                )
            })
            .count();
        writeln!(
            report,
            "{}: parity {}, rejected {}, divergent {}",
            context.label(),
            context_parity,
            context_rejected,
            corpus_count - context_parity - context_rejected,
        )
        .unwrap();
    }
    writeln!(report, "\n=== First-slice parity candidates ===").unwrap();
    for record in records.iter().filter(|record| {
        record.first_slice_candidate && matches!(record.classification, Classification::Parity)
    }) {
        writeln!(report, "{} [{}]", record.id, record.context.label()).unwrap();
    }
    writeln!(report, "\n=== All parity candidates ===").unwrap();
    for record in records
        .iter()
        .filter(|record| matches!(record.classification, Classification::Parity))
    {
        writeln!(report, "{} [{}]", record.id, record.context.label()).unwrap();
    }
    writeln!(report, "\n=== Controlled wrapper rejections ===").unwrap();
    for record in &records {
        if let Classification::ControlledWrapperRejection { reason } = &record.classification {
            writeln!(
                report,
                "\n--- {} [{}] ---\nReason: {reason:?}",
                record.id,
                record.context.label()
            )
            .unwrap();
        }
    }
    writeln!(report, "\n=== Divergences ===").unwrap();
    for record in &records {
        if let Classification::Divergence { badness, panache } = &record.classification {
            writeln!(
                report,
                "\n--- {} [{}] ---\nInput: {:?}\nBadness: {:?}\nPanache: {}",
                record.id,
                record.context.label(),
                record.input,
                badness,
                panache
                    .as_ref()
                    .map_or_else(|| "<declined>".to_owned(), |output| format!("{output:?}")),
            )
            .unwrap();
        }
    }
    report
}

fn sample_report_records() -> Vec<BaselineRecord> {
    vec![
        BaselineRecord {
            id: "b.tex".to_owned(),
            context: OracleContext::Environment,
            input: "b".to_owned(),
            classification: Classification::Divergence {
                badness: "b".to_owned(),
                panache: None,
            },
            first_slice_candidate: false,
        },
        BaselineRecord {
            id: "a.tex".to_owned(),
            context: OracleContext::Inline,
            input: "a".to_owned(),
            classification: Classification::Parity,
            first_slice_candidate: true,
        },
        BaselineRecord {
            id: "a.tex".to_owned(),
            context: OracleContext::Display,
            input: "a".to_owned(),
            classification: Classification::ControlledWrapperRejection {
                reason: "wrapper".to_owned(),
            },
            first_slice_candidate: false,
        },
    ]
}

/// Whether `body` carries a TeX comment, which runs to end of line and so
/// pins the break that follows it.
fn has_tex_comment(body: &str) -> bool {
    let mut escaped = false;
    for character in body.chars() {
        match character {
            '\\' => escaped = !escaped,
            '%' if !escaped => return true,
            _ => escaped = false,
        }
    }
    false
}

/// Join the lines of a Badness inline body the way Panache prints them.
///
/// Badness formats LaTeX, where a newline inside `$...$` costs nothing. Panache
/// emits inline math into a Markdown line -- a paragraph, or a table cell whose
/// row ends at the newline -- so it prints inline bodies flat and drops the
/// layout indent of each joined line. Display and environment contexts compare
/// byte for byte, as does an inline body whose comment pins its breaks.
fn flatten_inline(body: &str) -> String {
    body.split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn assert_formatter_parity(body: &str, context: OracleContext) {
    let badness =
        badness_body(body, context).unwrap_or_else(|error| panic!("{error}; body: {body:?}"));
    let badness = if context == OracleContext::Inline && !has_tex_comment(body) {
        flatten_inline(&badness)
    } else {
        badness
    };
    let panache =
        panache_body(body, context).unwrap_or_else(|error| panic!("{error}; body: {body:?}"));
    assert_eq!(
        panache, badness,
        "formatter parity failed in {context:?} context"
    );
}

#[test]
fn oracle_extracts_bodies_from_all_controlled_contexts() {
    let expected = ["a + b", "  a + b", "  a + b"];
    for (context, expected) in OracleContext::ALL.into_iter().zip(expected) {
        assert_eq!(badness_body("a+b", context).as_deref(), Ok(expected));
    }
}

#[test]
fn oracle_compares_formatter_output_byte_for_byte() {
    for context in OracleContext::ALL {
        assert_formatter_parity("a+b", context);
    }
}

#[test]
fn flat_inline_migration_slice_matches_badness() {
    const CASES: &[&str] = &[
        "display/authored_newline.tex",
        "inline/simple_equality.tex",
        "inline/sum_expression.tex",
        "operators/double_minus.tex",
        "operators/plus.tex",
        "operators/plus_tight.tex",
        "operators/relation_chain.tex",
        "operators/unary_minus.tex",
    ];

    let root = corpus_root();
    for id in CASES {
        let input = fs::read_to_string(root.join(id))
            .unwrap_or_else(|error| panic!("failed to read `{id}`: {error}"));
        assert!(
            is_flat_inline_candidate(&input),
            "mandatory flat-inline case `{id}` left the selected CST slice",
        );
        assert_formatter_parity(&input, OracleContext::Inline);
    }
}

#[test]
fn flat_inline_edge_cases_match_badness() {
    for body in ["a/b", "a/ b", "a /b"] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn composite_relations_match_badness() {
    for (body, expected) in [
        ("x:=y", "x := y"),
        ("x:=-y", "x := -y"),
        ("a::=b", "a ::= b"),
        ("a:=_ib", "a :=_i b"),
        ("a::=_ib", "a ::=_i b"),
    ] {
        let panache = panache_body(body, OracleContext::Inline).expect("Panache formatter");
        let badness = badness_body(body, OracleContext::Inline).expect("Badness formatter");
        assert_eq!(panache, expected, "{body:?}");
        assert_eq!(panache, badness, "{body:?}");
    }
}

/// Badness keeps whatever space the author wrote around a coerced unary sign.
/// Panache strips it, so a unary sign always binds to its operand -- the
/// behavior `docs/guide/formatting.qmd` documents (`x = -y`, `f(-x)`).
#[test]
fn panache_tightens_unary_signs_where_badness_keeps_author_space() {
    for (body, expected) in [
        ("- x", "-x"),
        ("x = - y", "x = -y"),
        ("f( - x)", "f(-x)"),
        ("a {- b}", "a {-b}"),
        ("e^{- t}", "e^{-t}"),
    ] {
        let panache = panache_body(body, OracleContext::Inline).expect("Panache formatter");
        let badness = badness_body(body, OracleContext::Inline).expect("Badness formatter");
        assert_eq!(panache, expected, "{body:?}");
        assert_ne!(panache, badness, "{body:?}");
    }
}

#[test]
fn panache_preserves_scripted_composite_relations_where_badness_splits_them() {
    for (body, expected) in [
        ("x<=_iy", "x <=_i y"),
        ("x>=_iy", "x >=_i y"),
        ("a==_kb", "a ==_k b"),
    ] {
        let panache = panache_body(body, OracleContext::Inline).expect("Panache formatter");
        let badness = badness_body(body, OracleContext::Inline).expect("Badness formatter");
        assert_eq!(panache, expected, "{body:?}");
        assert_ne!(panache, badness, "known Badness defect: {body:?}");
    }
}

#[test]
fn ordinary_group_migration_slice_matches_badness() {
    for body in ["{ a+b }", "a+{b-c}", "{{ α<=β }}", "{   }"] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn signature_proven_command_migration_slice_matches_badness() {
    for body in [
        r"\frac{ a+b }{ c-d }",
        r"\sqrt{ a+b }",
        r"\sqrt[ a+b ]{ c-d }",
        r"\frac { a+b } { c-d }",
        r"x+\frac{{ a+b }}{c}",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn signature_proven_command_comment_migration_slice_matches_badness() {
    for body in [
        "\\frac{% numerator\n a+b}{c}",
        "\\frac{a+b % numerator\n}{c}",
        "\\sqrt[% index\n n+1]{x}",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn bare_command_migration_slice_matches_badness() {
    for body in [
        r"\alpha+\beta",
        r"a\cdot b",
        r"x\leq-y",
        r"\sin x",
        r"\unknown x",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn script_migration_slice_matches_badness() {
    for body in [
        "x^2",
        "x _ i ^ { a+b }",
        r"\alpha_i+\beta^2",
        r"\frac{ a+b }{c}^2",
        "{ a+b }^2",
        "e^{x_i^2}",
        r"\sum_{i=1}^{n} i",
        r"x^\alpha+y_\beta",
        r"x^{a\in A}",
        r"x^{\alpha b}",
        "x^{( a )}",
        "x^{a/ b}",
        r"x^{\frac{a+b}{c-d}}",
        r"a\leq_i-b",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn paired_delimiter_migration_slice_matches_badness() {
    for body in [
        r"\left (  a+b  \right )",
        r"x+\left[ \frac{ a+b }{c} \right]",
        r"\left.   \alpha   \right|",
        r"\left\langle x \right\rangle",
        r"\left( a, b \right]",
        r"\left(   \right)",
        r"\left x \right)",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn paired_delimiter_script_migration_slice_matches_badness() {
    for body in [
        r"\left( x _ i + y ^ { a+b } \right)",
        r"\left[ \frac{ a+b }{c}^2 \right]",
        r"x ^ { \left( a+b \right) }",
        r"\left( x \right) ^ 2",
        r"a + \left[ b+c \right] _ { i+j }",
        r"\left. x_i \right| _ 0 ^ 1",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn nested_paired_delimiter_migration_slice_matches_badness() {
    for body in [
        r"\left[ \left( a+b \right) + c \right]",
        r"x ^ { \left[ \left( a+b \right) \right] }",
        r"\left( \left[ x \right] ^ 2 \right) _ i",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn top_level_edge_comment_migration_slice_matches_badness() {
    for body in [
        "% leading comment\nx = 1\n",
        "% base comment\nx^2",
        "a + b % this is a comment\n",
        "a + b % final comment",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn mid_expression_comment_migration_slice_matches_badness() {
    for body in [
        "a% operand before comment\n+b",
        "a+% binary before comment\n-b",
        "a=% relation before comment\n-b",
        "\\frac{a % keep this comment\n+b}{c}",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn bracketed_body_comment_migration_slice_matches_badness() {
    for body in [
        "{a+b % inner\n}",
        "{% inner\n a+b}",
        "{a % inner\n+b}",
        "{ % only\n }",
        "{{a % inner\n}}",
        "{a+b % inner\n} + c",
        "\\frac{{a % inner\n}}{c}",
        "\\sqrt[{a % inner\n}]{b}",
        "\\left( {a % inner\n} \\right)",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn script_argument_comment_migration_slice_matches_badness() {
    for body in [
        "x^{a % inner\n+b}",
        "x^{% inner\n a}",
        "x^{a+b % inner\n}",
        "x_{a % inner\n}^2",
        "x^{{a % inner\n}}",
        "x^{a % inner\n}_{b % other\n}",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn paired_delimiter_body_comment_migration_slice_matches_badness() {
    for body in [
        "\\left( a % inner\n+ b \\right)",
        "\\left( % lead\n a+b \\right)",
        "\\left( a+b % trail\n \\right)",
        "\\left(a % inner\n\\right)",
        "\\left( % only\n \\right)",
        "\\left\\langle a % inner\n+b \\right\\rangle",
        "\\left( \\left[ a % inner\n \\right] \\right)",
        "\\left( a % inner\n \\right)^2",
        "\\frac{\\left( a % inner\n \\right)}{b}",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
    }
}

#[test]
fn free_display_comment_migration_slice_matches_badness() {
    let root = corpus_root();
    for id in [
        "comments/argument_leading.tex",
        "comments/argument_trailing.tex",
        "comments/comment_line.tex",
        "comments/delimiter_body_mid.tex",
        "comments/delimiter_body_trailing.tex",
        "comments/group_leading.tex",
        "comments/group_nested.tex",
        "comments/group_trailing.tex",
        "comments/inside_math_argument.tex",
        "comments/mid_expression_after_binary.tex",
        "comments/mid_expression_after_operand.tex",
        "comments/mid_expression_after_relation.tex",
        "comments/optional_argument_leading.tex",
        "comments/script_argument_mid.tex",
        "comments/script_argument_trailing.tex",
        "comments/trailing_comment.tex",
    ] {
        let input = fs::read_to_string(root.join(id))
            .unwrap_or_else(|error| panic!("failed to read `{id}`: {error}"));
        let body = input.trim_end_matches('\n');
        assert_formatter_parity(body, OracleContext::Display);
        let once = panache_body(body, OracleContext::Display).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Display).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "display comment case is not idempotent: `{id}`"
        );
    }
}

#[test]
fn free_environment_comment_migration_slice_matches_badness() {
    let root = corpus_root();
    for id in [
        "comments/argument_leading.tex",
        "comments/argument_trailing.tex",
        "comments/comment_line.tex",
        "comments/delimiter_body_mid.tex",
        "comments/delimiter_body_trailing.tex",
        "comments/group_leading.tex",
        "comments/group_nested.tex",
        "comments/group_trailing.tex",
        "comments/inside_math_argument.tex",
        "comments/mid_expression_after_binary.tex",
        "comments/mid_expression_after_operand.tex",
        "comments/mid_expression_after_relation.tex",
        "comments/optional_argument_leading.tex",
        "comments/script_argument_mid.tex",
        "comments/script_argument_trailing.tex",
        "comments/trailing_comment.tex",
    ] {
        let input = fs::read_to_string(root.join(id))
            .unwrap_or_else(|error| panic!("failed to read `{id}`: {error}"));
        let body = input.trim_end_matches('\n');
        assert_formatter_parity(body, OracleContext::Environment);
        let once = panache_body(body, OracleContext::Environment).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Environment).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "environment comment case is not idempotent: `{id}`"
        );
    }
}

#[test]
fn environment_grid_final_cell_comment_migration_slice_matches_badness() {
    for body in [
        "a&={b % inner\n+c}\\\\\nd&=e",
        "α&={β % inner\n+γ}\\\\\nδ&=ε",
        "a&=\\frac{b % numerator\n+c}{d}\\\\\ne&=f",
        "a&=x^{b % exponent\n+c}\\\\\nd&=e",
        "a&=\\left( b % inner\n+c \\right)\\\\\nd&=e",
    ] {
        assert_formatter_parity(body, OracleContext::Environment);
        let once = panache_body(body, OracleContext::Environment).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Environment).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "environment grid cell is not idempotent: {body:?}"
        );
    }
}

#[test]
fn environment_grid_nonfinal_cell_comment_migration_slice_matches_badness() {
    for body in [
        "{a % left cell\n+b}&=c\\\\\nd&=e",
        "{α % left cell\n+β}&=γ\\\\\nδ&=ε",
        "a&{b % middle cell\n+c}&=d\\\\\ne&f&=g",
        "\\frac{a % numerator\n+b}{c}&=d\\\\\ne&=f",
        "x^{a % exponent\n+b}&=c\\\\\nd&=e",
        "\\left( a % inner\n+b \\right)&=c\\\\\nd&=e",
        "a&\\frac{b % numerator\n+c}{d}&=e\\\\\nf&g&=h",
        "{a % left cell\n+b}&=c",
    ] {
        assert_formatter_parity(body, OracleContext::Environment);
        let once = panache_body(body, OracleContext::Environment).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Environment).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "environment non-final grid cell is not idempotent: {body:?}"
        );
    }
}

#[test]
fn nested_environment_comment_migration_slice_matches_badness() {
    for body in [
        "\\begin{gathered}\n{a % inner\n+b}\n\\end{gathered}",
        "\\begin{aligned}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{aligned}",
        "\\begin{aligned}\n{a % left cell\n+b}&=c\\\\\nd&=e\n\\end{aligned}",
        "\\begin{gathered}\n\\begin{aligned}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{aligned}\n\\end{gathered}",
    ] {
        assert_formatter_parity(body, OracleContext::Environment);
        let once = panache_body(body, OracleContext::Environment).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Environment).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "nested environment comment case is not idempotent: {body:?}"
        );
    }
}

#[test]
fn embedded_environment_migration_slice_matches_badness() {
    let root = corpus_root();
    for id in [
        "environments/aligned/multi_ampersand.tex",
        "environments/aligned/ragged_columns.tex",
        "environments/aligned/single_row.tex",
        "environments/aligned/three_rows.tex",
        "environments/aligned/two_rows.tex",
        "environments/aligned/with_frac.tex",
        "environments/cases/piecewise.tex",
        "environments/cases/sign.tex",
        "environments/cases/single.tex",
        "environments/matrix/bmatrix.tex",
        "environments/matrix/plain.tex",
        "environments/matrix/pmatrix.tex",
        "environments/matrix/three_by_three.tex",
        "environments/recovery/nested.tex",
        "escapes/literal_backslash_in_env.tex",
    ] {
        let body = fs::read_to_string(root.join(id))
            .unwrap_or_else(|error| panic!("failed to read `{id}`: {error}"));
        assert_formatter_parity(&body, OracleContext::Inline);
        let once = panache_body(&body, OracleContext::Inline).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Inline).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "embedded environment is not idempotent: `{id}`"
        );
    }

    for body in [
        "\\begin{matrix}\na&b\\\\\nc&d\n\\end{matrix}",
        "x+\\begin{matrix}\na&b\\\\\nc&d\n\\end{matrix}",
        "(x,\\begin{matrix}\na&b\\\\\nc&d\n\\end{matrix},y)",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
        let once = panache_body(body, OracleContext::Inline).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Inline).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "embedded environment is not idempotent: {body:?}"
        );
    }
}

#[test]
fn delimited_environment_migration_slice_matches_badness() {
    let id = "environments/nested/delimited_matrix.tex";
    let body = fs::read_to_string(corpus_root().join(id))
        .unwrap_or_else(|error| panic!("failed to read `{id}`: {error}"));

    for context in OracleContext::ALL {
        assert_formatter_parity(&body, context);
        let once = panache_body(&body, context).expect("first Panache pass");
        let twice = panache_body(&once, context).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "delimited environment is not idempotent in {context:?}"
        );
    }
}

#[test]
fn malformed_embedded_environment_stays_on_compatibility_path() {
    let body = r"\begin {aligned}x\end {aligned}";
    assert_eq!(
        panache_body(body, OracleContext::Inline).as_deref(),
        Ok(body)
    );
}

#[test]
fn authored_line_break_migration_slice_matches_badness() {
    for body in [
        "a\\\\*[2ex]\nb",
        "a\\\\b",
        "a \\\\*[2ex]\n-b",
        "a+b\\\\c-d",
        "a&=b\\\\\nc&=d",
        "x&=a\\\\\n&=b",
        "a&&=b\\\\\nc&&=d",
        "a&=bb\\\\\nccc&=d",
        "a&={b % inner\n+c}\\\\\nd&=e",
        "{a+b\\\\c-d}",
        "\\frac{a+b\\\\c-d}{e}",
        "\\left( a+b\\\\c-d \\right)",
        "a \\\\ % first row\nb",
    ] {
        for context in OracleContext::ALL {
            assert_formatter_parity(body, context);
            let once = panache_body(body, context).expect("first Panache pass");
            let twice = panache_body(&once, context).expect("second Panache pass");
            assert_eq!(
                once, twice,
                "authored line break is not idempotent in {context:?}: {body:?}"
            );
        }
    }
}

/// The hanging indent this slice emits re-enters the parser as `MATH_SPACE` on
/// the next pass, so guard the round trip explicitly.
#[test]
fn bracketed_body_comment_lowering_is_idempotent() {
    for body in [
        "{a+b % inner\n}",
        "{% inner\n a+b}",
        "{a % inner\n+b}",
        "{ % only\n }",
        "{{a % inner\n}}",
        "{a+b % inner\n} + c",
        "\\frac{{a % inner\n}}{c}",
        "\\sqrt[{a % inner\n}]{b}",
        "\\left( {a % inner\n} \\right)",
        "x^{a % inner\n+b}",
        "x^{% inner\n a}",
        "x^{a+b % inner\n}",
        "x_{a % inner\n}^2",
        "x^{{a % inner\n}}",
        "x^{a % inner\n}_{b % other\n}",
        "\\left( a % inner\n+ b \\right)",
        "\\left( % lead\n a+b \\right)",
        "\\left( a+b % trail\n \\right)",
        "\\left( % only\n \\right)",
        "\\left( \\left[ a % inner\n \\right] \\right)",
        "\\left( a % inner\n \\right)^2",
    ] {
        let once = panache_body(body, OracleContext::Inline).expect("first pass");
        let twice = panache_body(&once, OracleContext::Inline).expect("second pass");
        assert_eq!(once, twice, "not idempotent: {body:?}");
    }
}

#[test]
fn argument_recursion_contract_matches_badness() {
    let cases = [
        r"\frac{ a   +   b }{ c   +   d }",
        r"\text{ a   +   b }",
        r"\unknown{ a   +   b }",
        r"\sqrt{ a   +   b }{ c   +   d }",
    ];
    for body in cases {
        for context in OracleContext::ALL {
            assert_formatter_parity(body, context);
        }
    }
}

#[test]
fn oracle_ranks_relations_above_binaries_and_never_breaks_at_unary_signs() {
    let formatted = badness_body_with_width(
        "aaaaaaaa = -bbbbbbbb + cccccccc = dddddddd",
        OracleContext::Display,
        24,
    )
    .expect("Badness display-math oracle");
    let indented_operators = formatted
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let operator = trimmed.chars().next()?;
            matches!(operator, '=' | '+' | '-').then_some((line.len() - trimmed.len(), operator))
        })
        .collect::<Vec<_>>();

    assert!(
        indented_operators
            .iter()
            .any(|(_, operator)| *operator == '='),
        "expected a relation break in {formatted:?}",
    );
    assert!(
        indented_operators
            .iter()
            .any(|(_, operator)| *operator == '+'),
        "expected a binary break in {formatted:?}",
    );
    assert!(
        indented_operators
            .iter()
            .all(|(_, operator)| *operator != '-'),
        "unary sign became a break site in {formatted:?}",
    );
    let relation_indent = indented_operators
        .iter()
        .find_map(|(indent, operator)| (*operator == '=').then_some(*indent))
        .unwrap();
    let binary_indent = indented_operators
        .iter()
        .find_map(|(indent, operator)| (*operator == '+').then_some(*indent))
        .unwrap();
    assert!(relation_indent < binary_indent, "{formatted:?}");
}

#[test]
fn result_classification_distinguishes_all_baseline_outcomes() {
    assert!(matches!(
        classify_result(Ok("same".to_owned()), Some("same".to_owned())),
        Classification::Parity
    ));
    assert!(matches!(
        classify_result(Err("wrapper".to_owned()), Some("same".to_owned())),
        Classification::ControlledWrapperRejection { .. }
    ));
    assert!(matches!(
        classify_result(Ok("badness".to_owned()), Some("panache".to_owned())),
        Classification::Divergence { .. }
    ));
    assert!(matches!(
        classify_result(Ok("badness".to_owned()), None),
        Classification::Divergence { panache: None, .. }
    ));
}

#[test]
fn report_rendering_is_deterministic() {
    let records = sample_report_records();
    let forward = render_report(records.clone(), 2);
    let reverse = render_report(records.into_iter().rev().collect(), 2);
    assert_eq!(forward, reverse);
    assert!(forward.contains("Parity: 1 / 3"));
    assert!(forward.contains("Controlled wrapper rejection: 1 / 3"));
    assert!(forward.contains("Divergent: 1 / 3"));
}

#[test]
fn document_preamble_shadows_builtin_signatures_in_both_formatters() {
    let preamble = r"\renewcommand{\frac}[2]{#1/#2}";
    let body = r"\frac{ a   +   b }{ c   +   d }";
    let badness = badness_body_with_preamble(body, Some(preamble), OracleContext::Inline)
        .expect("Badness controlled wrapper");
    let panache = panache_body_with_preamble(body, Some(preamble), OracleContext::Inline)
        .expect("Panache formatter");
    assert_eq!(panache, badness);
    assert_eq!(panache, body);
}

#[test]
#[ignore = "manual: regenerate the committed Badness formatter baseline"]
fn math_badness_full_report() {
    let (corpus_count, records) = collect_baseline_records();
    let report = render_report(records, corpus_count);
    let path = manifest_path(REPORT_REL);
    fs::create_dir_all(path.parent().expect("report path has a parent"))
        .unwrap_or_else(|error| panic!("failed to create report directory: {error}"));
    fs::write(&path, report)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}
