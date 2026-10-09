//! Byte-exact Badness output oracle for Panache's math formatter.
//!
//! Badness formats complete LaTeX documents, whereas Panache's math entry point
//! receives a delimiter-free body. These test-only adapters place the same body
//! in controlled inline, display, and environment contexts, then mechanically
//! remove the wrappers. They do not parse or normalize the resulting TeX.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use badness_formatter::{FormatStyle, LineEnding, MathWrap, formatter::format_with_style};
use panache_formatter::MathMode;
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
            Self::Inline => {
                let suffix = if final_line_has_tex_comment(body) {
                    "\n$\n"
                } else {
                    "$\n"
                };
                (format!("${body}{suffix}"), "$".into(), suffix.into())
            }
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
    panache_body_with_preamble_and_width(body, None, context, 80)
}

fn panache_body_with_preamble(
    body: &str,
    preamble: Option<&str>,
    context: OracleContext,
) -> Result<String, String> {
    panache_body_with_preamble_and_width(body, preamble, context, 80)
}

fn panache_body_with_preamble_and_width(
    body: &str,
    preamble: Option<&str>,
    context: OracleContext,
    line_width: usize,
) -> Result<String, String> {
    format_math(
        body,
        &MathFormatOptions {
            mode: MathMode::Reflow,
            math_indent: 2,
            line_width,
            bookdown_equation_labels: false,
            context: context.panache_context(),
            signature_scope: signature_scope(preamble),
        },
    )
    .ok_or_else(|| format!("Panache declined {context:?} body"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Classification {
    MandatoryByteParity,
    IntentionalDifference {
        reason: IntentionalDifference,
        badness: String,
        panache: String,
    },
    Preserved {
        reason: PreservationReason,
        badness: Result<String, String>,
    },
    Unclassified {
        badness: Result<String, String>,
        panache: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntentionalDifference {
    InlineHostFlattening,
    SoftNewlineBeforeArguments,
}

impl IntentionalDifference {
    fn label(self) -> &'static str {
        match self {
            Self::InlineHostFlattening => "inline-host-flattening",
            Self::SoftNewlineBeforeArguments => "soft-newline-before-arguments",
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            Self::InlineHostFlattening => {
                "Markdown inline math joins Badness layout lines unless a TeX comment pins them."
            }
            Self::SoftNewlineBeforeArguments => {
                "Panache collapses an authored soft newline before proven command arguments; Badness preserves it."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreservationReason {
    MalformedMath,
    MissingNestedCommentLowering,
}

const PRESERVATION_BOUNDARY: [(&str, PreservationReason); 10] = [
    (
        "commands/argument_after_comment.tex",
        PreservationReason::MissingNestedCommentLowering,
    ),
    (
        "commands/math_argument_whitespace.tex",
        PreservationReason::MissingNestedCommentLowering,
    ),
    (
        "environments/recovery/comment_before_name.tex",
        PreservationReason::MalformedMath,
    ),
    (
        "environments/recovery/mismatched.tex",
        PreservationReason::MalformedMath,
    ),
    (
        "environments/recovery/trivia_before_name.tex",
        PreservationReason::MalformedMath,
    ),
    (
        "groups/left_right_group_boundary.tex",
        PreservationReason::MalformedMath,
    ),
    (
        "groups/left_right_stray.tex",
        PreservationReason::MalformedMath,
    ),
    (
        "groups/left_right_unclosed.tex",
        PreservationReason::MalformedMath,
    ),
    ("groups/unclosed.tex", PreservationReason::MalformedMath),
    (
        "scripts/missing_argument.tex",
        PreservationReason::MalformedMath,
    ),
];

impl PreservationReason {
    fn label(self) -> &'static str {
        match self {
            Self::MalformedMath => "malformed-math",
            Self::MissingNestedCommentLowering => {
                "missing-supported-shape: nested-comment-lowering"
            }
        }
    }
}

fn preservation_reason(id: &str) -> Option<PreservationReason> {
    PRESERVATION_BOUNDARY
        .iter()
        .find_map(|(candidate, reason)| (*candidate == id).then_some(*reason))
}

fn classify_result(
    id: &str,
    input: &str,
    context: OracleContext,
    badness: Result<String, String>,
    panache: Option<String>,
) -> Classification {
    if let Some(reason) = preservation_reason(id) {
        return if panache.is_none() {
            Classification::Preserved { reason, badness }
        } else {
            Classification::Unclassified { badness, panache }
        };
    }

    let Ok(badness_output) = badness else {
        return Classification::Unclassified { badness, panache };
    };
    let Some(panache_output) = panache else {
        return Classification::Unclassified {
            badness: Ok(badness_output),
            panache: None,
        };
    };

    if panache_output == badness_output {
        return Classification::MandatoryByteParity;
    }

    let reason = if id == "commands/argument_after_newline.tex"
        && badness_output.replacen('\n', " ", 1) == panache_output
    {
        Some(IntentionalDifference::SoftNewlineBeforeArguments)
    } else if context == OracleContext::Inline
        && !has_tex_comment(input)
        && panache_output == flatten_inline(&badness_output)
    {
        Some(IntentionalDifference::InlineHostFlattening)
    } else {
        None
    };

    match reason {
        Some(reason) => Classification::IntentionalDifference {
            reason,
            badness: badness_output,
            panache: panache_output,
        },
        None => Classification::Unclassified {
            badness: Ok(badness_output),
            panache: Some(panache_output),
        },
    }
}

fn classification_label(classification: &Classification) -> &'static str {
    match classification {
        Classification::MandatoryByteParity => "parity",
        Classification::IntentionalDifference { .. } => "intentional",
        Classification::Preserved { .. } => "preserved",
        Classification::Unclassified { .. } => "unclassified",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuditRecord {
    id: String,
    context: OracleContext,
    input: String,
    classification: Classification,
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

fn collect_audit_records() -> (usize, Vec<AuditRecord>) {
    let root = corpus_root();
    let cases = discover_cases(&root);
    let mut records = Vec::with_capacity(cases.len() * OracleContext::ALL.len());
    for path in &cases {
        let id = normalized_id(&root, path);
        let source = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("failed to read `{id}`: {error}"));
        let input = source.strip_suffix('\n').unwrap_or(&source).to_owned();
        let preamble = read_preamble(path)
            .unwrap_or_else(|error| panic!("failed to read preamble for `{id}`: {error}"));
        for context in OracleContext::ALL {
            let badness = badness_body_with_preamble(&input, preamble.as_deref(), context);
            let panache = panache_body_with_preamble(&input, preamble.as_deref(), context).ok();
            records.push(AuditRecord {
                id: id.clone(),
                context,
                input: input.clone(),
                classification: classify_result(&id, &input, context, badness, panache),
            });
        }
    }
    (cases.len(), records)
}

fn render_report(mut records: Vec<AuditRecord>, corpus_count: usize) -> String {
    records.sort_by(|left, right| (&left.id, left.context).cmp(&(&right.id, right.context)));
    let total = records.len();
    let parity = records
        .iter()
        .filter(|record| matches!(record.classification, Classification::MandatoryByteParity))
        .count();
    let intentional = records
        .iter()
        .filter(|record| {
            matches!(
                record.classification,
                Classification::IntentionalDifference { .. }
            )
        })
        .count();
    let preserved = records
        .iter()
        .filter(|record| matches!(record.classification, Classification::Preserved { .. }))
        .count();
    let unclassified = total - parity - intentional - preserved;
    let mut report = String::new();
    writeln!(report, "Panache/Badness math formatter audit").unwrap();
    writeln!(report, "Oracle: badness-formatter =0.7.0").unwrap();
    writeln!(report, "Corpus: tests/fixtures/math_corpus").unwrap();
    writeln!(report, "Cases: {corpus_count}").unwrap();
    writeln!(report, "Context runs: {total}").unwrap();
    writeln!(report, "Mandatory byte parity: {parity} / {total}").unwrap();
    writeln!(
        report,
        "Named intentional difference: {intentional} / {total}"
    )
    .unwrap();
    writeln!(report, "Preserved at named boundary: {preserved} / {total}").unwrap();
    writeln!(report, "Unclassified: {unclassified} / {total}\n").unwrap();
    writeln!(report, "Regenerate with:").unwrap();
    writeln!(
        report,
        "  cargo test -p panache-formatter --test math_badness_oracle math_badness_full_report -- --ignored --nocapture\n"
    )
    .unwrap();
    writeln!(report, "=== Counts by context ===").unwrap();
    for context in OracleContext::ALL {
        let counts = ["parity", "intentional", "preserved", "unclassified"].map(|label| {
            records
                .iter()
                .filter(|record| {
                    record.context == context
                        && classification_label(&record.classification) == label
                })
                .count()
        });
        writeln!(
            report,
            "{}: parity {}, intentional {}, preserved {}, unclassified {}",
            context.label(),
            counts[0],
            counts[1],
            counts[2],
            counts[3],
        )
        .unwrap();
    }
    writeln!(report, "\n=== Mandatory byte parity ===").unwrap();
    for record in records
        .iter()
        .filter(|record| matches!(record.classification, Classification::MandatoryByteParity))
    {
        writeln!(report, "{} [{}]", record.id, record.context.label()).unwrap();
    }

    writeln!(report, "\n=== Named intentional differences ===").unwrap();
    for record in &records {
        if let Classification::IntentionalDifference {
            reason,
            badness,
            panache,
        } = &record.classification
        {
            writeln!(
                report,
                "\n--- {} [{}] ---\nReason: {}\nPolicy: {}\nInput: {:?}\nBadness: {:?}\nPanache: {:?}",
                record.id,
                record.context.label(),
                reason.label(),
                reason.explanation(),
                record.input,
                badness,
                panache,
            )
            .unwrap();
        }
    }

    writeln!(report, "\n=== Preserved at the named boundary ===").unwrap();
    for record in &records {
        if let Classification::Preserved { reason, badness } = &record.classification {
            writeln!(
                report,
                "{} [{}]: {} (Badness: {})",
                record.id,
                record.context.label(),
                reason.label(),
                if badness.is_ok() {
                    "formatted"
                } else {
                    "rejected"
                },
            )
            .unwrap();
        }
    }

    writeln!(report, "\n=== Unclassified (must stay empty) ===").unwrap();
    for record in &records {
        if let Classification::Unclassified { badness, panache } = &record.classification {
            writeln!(
                report,
                "\n--- {} [{}] ---\nInput: {:?}\nBadness: {}\nPanache: {}",
                record.id,
                record.context.label(),
                record.input,
                badness.as_ref().map_or_else(
                    |error| format!("<rejected: {error}>"),
                    |output| format!("{output:?}")
                ),
                panache
                    .as_ref()
                    .map_or_else(|| "<declined>".to_owned(), |output| format!("{output:?}")),
            )
            .unwrap();
        }
    }
    report
}

fn sample_report_records() -> Vec<AuditRecord> {
    vec![
        AuditRecord {
            id: "b.tex".to_owned(),
            context: OracleContext::Environment,
            input: "b".to_owned(),
            classification: Classification::Unclassified {
                badness: Ok("b".to_owned()),
                panache: None,
            },
        },
        AuditRecord {
            id: "a.tex".to_owned(),
            context: OracleContext::Inline,
            input: "a".to_owned(),
            classification: Classification::MandatoryByteParity,
        },
        AuditRecord {
            id: "a.tex".to_owned(),
            context: OracleContext::Display,
            input: "a".to_owned(),
            classification: Classification::Preserved {
                reason: PreservationReason::MalformedMath,
                badness: Err("wrapper".to_owned()),
            },
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

fn final_line_has_tex_comment(body: &str) -> bool {
    has_tex_comment(body.rsplit_once('\n').map_or(body, |(_, line)| line))
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
    for body in ["a/b", "a/ b", "a /b", r"\int x \ \mathrm{d}x"] {
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

/// Badness 0.10 now keeps postfix signs tight, matching Panache's behavior.
#[test]
fn postfix_signs_match_badness() {
    for (body, expected) in [("N(t-)", "N(t-)"), ("S(T_i - )", "S(T_i-)")] {
        let panache = panache_body(body, OracleContext::Inline).expect("Panache formatter");
        let badness = badness_body(body, OracleContext::Inline).expect("Badness formatter");
        assert_eq!(panache, expected, "{body:?}");
        assert_eq!(panache, badness, "{body:?}");
    }
}

/// Badness treats a sign scanned as part of an unbraced TeX dimension as a
/// binary math operator. Panache keeps the command gap but attaches the sign
/// to the numeric dimension.
#[test]
fn panache_attaches_dimension_signs_where_badness_treats_them_as_binary() {
    for (body, expected) in [
        (r"\hskip -1cm", r"\hskip -1cm"),
        (r"\hskip -30mm", r"\hskip -30mm"),
        (r"\vskip -2pt", r"\vskip -2pt"),
        (r"\kern -3em", r"\kern -3em"),
        (r"\mkern -4mu", r"\mkern -4mu"),
        (r"\mskip -5mu", r"\mskip -5mu"),
    ] {
        let panache = panache_body(body, OracleContext::Inline).expect("Panache formatter");
        let badness = badness_body(body, OracleContext::Inline).expect("Badness formatter");
        assert_eq!(panache, expected, "{body:?}");
        assert_ne!(panache, badness, "known Badness defect: {body:?}");
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
fn environment_grid_multiple_multiline_cells_match_badness() {
    for body in [
        "{a % left cell\n+b}&={c % right cell\n+d}\\\\\ne&=f",
        "\\frac{a % numerator\n+b}{c}&=x^{d % exponent\n+e}\\\\\nf&=g",
        "\\left( a % left delimiter\n+b \\right)&=\\left( c % right delimiter\n+d \\right)\\\\\ne&=f",
        "x+{a % left group\n+b}&=y+{c % right group\n+d}\\\\\ne&=f",
    ] {
        assert_formatter_parity(body, OracleContext::Environment);
        let once = panache_body(body, OracleContext::Environment).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Environment).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "environment row with multiple multiline cells is not idempotent: {body:?}"
        );
    }
}

#[test]
fn environment_grid_multiline_cells_across_rows_match_badness() {
    for body in [
        "{a % first row left cell\n+b}&=c\\\\\nd&={e % second row right cell\n+f}",
        "a&{b % first row middle cell\n+c}&=d\\\\\n{e % second row left cell\n+f}&g&=h",
        "a&=x^{b % first row exponent\n+c}\\\\\n\\frac{d % second row numerator\n+e}{f}&=g",
        "\\left( a % first row delimiter\n+b \\right)&=c\\\\\nd&=y+{e % second row group\n+f}",
    ] {
        assert_formatter_parity(body, OracleContext::Environment);
        let once = panache_body(body, OracleContext::Environment).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Environment).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "environment rows with multiline cells are not idempotent: {body:?}"
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
fn array_environment_argument_migration_slice_matches_badness() {
    for body in [
        "\\begin{array}{cc}a&b\\\\c&d\\end{array}",
        "\\begin{array}[t]{cc}a&b\\\\c&d\\end{array}",
        "\\begin{array}\n{cc}a&b\\\\c&d\\end{array}",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
        let once = panache_body(body, OracleContext::Inline).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Inline).expect("second Panache pass");
        assert_eq!(once, twice, "array environment is not idempotent: {body:?}");
    }
}

#[test]
fn comment_bearing_embedded_environment_matches_badness() {
    for body in [
        "x+\\begin{matrix}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{matrix},y",
        "(x,\\begin{matrix}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{matrix},y)",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
        let once = panache_body(body, OracleContext::Inline).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Inline).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "comment-bearing embedded inline environment is not idempotent: {body:?}"
        );
    }
}

#[test]
fn comment_bearing_embedded_display_environment_matches_badness() {
    let body = "x+\\begin{matrix}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{matrix}";
    let badness = badness_body(body, OracleContext::Display).expect("Badness formatter");

    assert_eq!(
        badness,
        "  x\n  + \\begin{matrix}\n      a & = {b % inner\n             + c} \\\\\n      d & = e\n    \\end{matrix}"
    );

    assert_formatter_parity(body, OracleContext::Display);
    let once = panache_body(body, OracleContext::Display).expect("first Panache pass");
    let twice = panache_body(&once, OracleContext::Display).expect("second Panache pass");
    assert_eq!(
        once, twice,
        "comment-bearing embedded display environment is not idempotent: {body:?}"
    );

    let unary = "x=+\\begin{matrix}\na&={b % inner\n+c}\n\\end{matrix}";
    let badness = badness_body(unary, OracleContext::Display).expect("Badness formatter");

    assert_eq!(
        badness,
        "  x = +\\begin{matrix}\n         a & = {b % inner\n                + c}\n       \\end{matrix}"
    );

    for unary in [
        unary,
        "x=-\\begin{matrix}\na&={b % inner\n+c}\n\\end{matrix}",
        "x++\\begin{matrix}\na&={b % inner\n+c}\n\\end{matrix}",
        "x+-\\begin{matrix}\na&={b % inner\n+c}\n\\end{matrix}",
    ] {
        assert_formatter_parity(unary, OracleContext::Display);
        let once = panache_body(unary, OracleContext::Display).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Display).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "unary-prefixed embedded display environment is not idempotent: {unary:?}"
        );
    }
}

#[test]
fn comment_bearing_embedded_display_environment_after_relation_matches_badness() {
    let body = "x=\\begin{matrix}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{matrix}";
    let badness = badness_body(body, OracleContext::Display).expect("Badness formatter");

    assert_eq!(
        badness,
        "  x = \\begin{matrix}\n        a & = {b % inner\n               + c} \\\\\n        d & = e\n      \\end{matrix}"
    );

    assert_formatter_parity(body, OracleContext::Display);
    let once = panache_body(body, OracleContext::Display).expect("first Panache pass");
    let twice = panache_body(&once, OracleContext::Display).expect("second Panache pass");
    assert_eq!(
        once, twice,
        "relation-led embedded display environment is not idempotent: {body:?}"
    );

    let environment = "\\begin{matrix}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{matrix}";
    for head in ["x\\leq", "x:=", "x\\gets"] {
        let body = format!("{head}{environment}");
        assert_formatter_parity(&body, OracleContext::Display);
        let once = panache_body(&body, OracleContext::Display).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Display).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "relation-led embedded display environment is not idempotent: {body:?}"
        );
    }
}

#[test]
fn operand_prefixed_comment_bearing_display_environment_matches_badness() {
    let body = "x\\begin{matrix}\na&={b % inner\n+c}\n\\end{matrix}";
    let badness = badness_body(body, OracleContext::Display).expect("Badness formatter");

    assert_eq!(
        badness,
        "  x\\begin{matrix}\n     a & = {b % inner\n            + c}\n   \\end{matrix}"
    );

    for body in [
        body,
        "x_i\\begin{matrix}\na&={b % inner\n+c}\n\\end{matrix}",
    ] {
        assert_formatter_parity(body, OracleContext::Display);
        let once = panache_body(body, OracleContext::Display).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Display).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "operand-prefixed display environment is not idempotent: {body:?}"
        );
    }
}

#[test]
fn delimited_operand_prefixed_comment_bearing_display_environment_matches_badness() {
    let environment = "\\begin{matrix}\na&={b % inner\n+c}\n\\end{matrix}";
    let representative = format!(r"\left(x\right){environment}");
    assert_eq!(
        badness_body(&representative, OracleContext::Display).expect("Badness formatter"),
        "  \\left( x \\right)\\begin{matrix}\n                    a & = {b % inner\n                           + c}\n                  \\end{matrix}"
    );

    for prefix in [r"\left(x\right)", r"\left(x\right)^2"] {
        let body = format!("{prefix}{environment}");
        assert_formatter_parity(&body, OracleContext::Display);
        let once = panache_body(&body, OracleContext::Display).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Display).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "delimited-operand-prefixed display environment is not idempotent: {body:?}"
        );
    }
}

#[test]
fn grouped_operand_prefixed_comment_bearing_display_environment_matches_badness() {
    let environment = "\\begin{matrix}\na&={b % inner\n+c}\n\\end{matrix}";
    let representative = format!("{{x}}{environment}");
    assert_eq!(
        badness_body(&representative, OracleContext::Display).expect("Badness formatter"),
        "  {x}\\begin{matrix}\n       a & = {b % inner\n              + c}\n     \\end{matrix}"
    );

    for prefix in ["{x}", "{x}_i"] {
        let body = format!("{prefix}{environment}");
        assert_formatter_parity(&body, OracleContext::Display);
        let once = panache_body(&body, OracleContext::Display).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Display).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "grouped-operand-prefixed display environment is not idempotent: {body:?}"
        );
    }

    let unclosed = format!("{{x{environment}");
    assert!(panache_body(&unclosed, OracleContext::Display).is_err());
}

#[test]
fn comment_bearing_embedded_display_environment_with_trailing_content_matches_badness() {
    let environment = "\\begin{matrix}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{matrix}";
    let punctuation = format!("x+{environment},y");
    let badness = badness_body(&punctuation, OracleContext::Display).expect("Badness formatter");

    assert_eq!(
        badness,
        "  x\n  + \\begin{matrix}\n      a & = {b % inner\n             + c} \\\\\n      d & = e\n    \\end{matrix},y"
    );

    assert_formatter_parity(&punctuation, OracleContext::Display);
    let once = panache_body(&punctuation, OracleContext::Display).expect("first Panache pass");
    let twice = panache_body(&once, OracleContext::Display).expect("second Panache pass");
    assert_eq!(
        once, twice,
        "display environment with trailing content is not idempotent: {punctuation:?}"
    );
    let delimiter_framed = format!("\n{punctuation}\n");
    assert_eq!(
        panache_body(&delimiter_framed, OracleContext::Display).as_deref(),
        Ok(badness.as_str()),
        "delimiter-owned edge newlines must remain on the typed path"
    );

    let operator_suffixes = [format!("x+{environment}+y"), format!("x={environment}=y")];
    for body in operator_suffixes {
        assert_formatter_parity(&body, OracleContext::Display);
        let once = panache_body(&body, OracleContext::Display).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Display).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "operator-bearing display suffix is not idempotent: {body:?}"
        );
    }
}

#[test]
fn scripted_comment_bearing_display_environment_matches_badness() {
    let environment = "\\begin{matrix}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{matrix}";
    for body in [
        format!("x+{environment}^T,y"),
        format!("x+{environment}^T+y"),
        format!("x={environment}_i=y"),
    ] {
        assert_formatter_parity(&body, OracleContext::Display);
        let once = panache_body(&body, OracleContext::Display).expect("first Panache pass");
        let twice = panache_body(&once, OracleContext::Display).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "scripted display environment is not idempotent: {body:?}"
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
fn mixed_delimited_environment_migration_slice_matches_badness() {
    let body = "\\left(x+\\begin{matrix}\na&b\\\\\nc&d\n\\end{matrix},y\\right)";

    for context in OracleContext::ALL {
        assert_formatter_parity(body, context);
        let once = panache_body(body, context).expect("first Panache pass");
        let twice = panache_body(&once, context).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "mixed delimited environment is not idempotent in {context:?}"
        );
    }
}

#[test]
fn multiple_delimited_environments_match_badness() {
    let authored_rows = "\\left(\\begin{matrix}\na&b\\\\\nc&d\n\\end{matrix},\\begin{matrix}\ne&f\\\\\ng&h\n\\end{matrix}\\right)";
    for context in OracleContext::ALL {
        assert_formatter_parity(authored_rows, context);
        let once = panache_body(authored_rows, context).expect("first Panache pass");
        let twice = panache_body(&once, context).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "multiple delimited environments are not idempotent in {context:?}"
        );
    }

    let comment = "\\left(\\begin{matrix}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{matrix},\\begin{matrix}\nf&g\\\\\nh&i\n\\end{matrix}\\right)";
    for context in OracleContext::ALL {
        assert_formatter_parity(comment, context);
        let once = panache_body(comment, context).expect("first Panache pass");
        let twice = panache_body(&once, context).expect("second Panache pass");
        assert_eq!(
            once, twice,
            "commented delimited environments are not idempotent in {context:?}"
        );
    }

    let unpunctuated_comment = "\\left(\\begin{matrix}\na&={b % inner\n+c}\n\\end{matrix}\\begin{matrix}\nd&e\n\\end{matrix}\\right)";
    assert!(panache_body(unpunctuated_comment, OracleContext::Display).is_err());
}

#[test]
fn malformed_embedded_environment_crosses_the_preservation_boundary() {
    let body = r"\begin {aligned}x\end {aligned}";
    assert!(panache_body(body, OracleContext::Inline).is_err());
}

#[test]
fn environment_leading_signs_match_badness() {
    for body in ["1 \\\\\n+ 2", "1 \\\\\n+2", "1 \\\\\n- 2"] {
        assert_formatter_parity(body, OracleContext::Environment);
        let body = format!("\\begin{{multlined}}\n{body}\n\\end{{multlined}}");
        assert_formatter_parity(&body, OracleContext::Display);
    }
}

#[test]
fn authored_line_break_migration_slice_matches_badness() {
    for body in [
        "x&=1 \\\\\n&+ 2",
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

#[test]
fn control_space_after_authored_line_break_preserves_forced_line_end() {
    let body = "df & = a \\\\\\ \n& = b";
    let context = OracleContext::Environment;

    let once = panache_body(body, context).expect("first Panache pass");
    assert_eq!(once, "  df & = a \\\\\n  \\ \n     & = b");
    let twice = panache_body(&once, context).expect("second Panache pass");
    assert_eq!(
        once, twice,
        "control space after authored line break is not idempotent in {context:?}"
    );
}

#[test]
fn control_space_before_display_break_is_idempotent() {
    let body = r"\frac{\partial \mathrm C}{\partial \mathrm t} + \frac{1}{2}\sigma^{2} \mathrm S^{2} \frac{\partial^{2} \mathrm C}{\partial \mathrm S^2} + \mathrm r \mathrm S \frac{\partial \mathrm C}{\partial \mathrm S}\ = \mathrm r \mathrm C";
    let context = OracleContext::Display;
    let width = 80;

    let once = panache_body_with_preamble_and_width(body, None, context, width)
        .expect("first Panache pass");
    assert_eq!(
        once,
        "  \\frac{\\partial \\mathrm C}{\\partial \\mathrm t}\n  + \\frac{1}{2}\\sigma^{2} \\mathrm S^{2} \\frac{\\partial^{2} \\mathrm C}{\\partial \\mathrm S^2}\n  + \\mathrm r \\mathrm S \\frac{\\partial \\mathrm C}{\\partial \\mathrm S}\\ \n  = \\mathrm r \\mathrm C"
    );
    let twice = panache_body_with_preamble_and_width(&once, None, context, width)
        .expect("second Panache pass");
    assert_eq!(
        once, twice,
        "control space before display break is not idempotent"
    );
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
fn panache_separates_punctuated_equations_where_badness_chains_relations() {
    let body = r"\hat{f}_i^{-i} = \sum_{j \neq i} \frac{S_{ij}y_j}{1 - S_{ii}}, \qquad \hat{f}_i = \frac{1}{k} \sum_{j \in \mathcal{N}_i} y_j.";
    let badness = badness_body(body, OracleContext::Display).expect("Badness display-math oracle");
    assert_eq!(
        badness,
        "  \\hat{f}_i^{-i} = \\sum_{j \\neq i} \\frac{S_{ij}y_j}{1 - S_{ii}}, \\qquad \\hat{f}_i\n                 = \\frac{1}{k} \\sum_{j \\in \\mathcal{N}_i} y_j.",
    );
    let panache = panache_body_with_preamble_and_width(body, None, OracleContext::Display, 80)
        .expect("Panache display-math formatter");
    assert_eq!(
        panache,
        "  \\hat{f}_i^{-i} = \\sum_{j \\neq i} \\frac{S_{ij}y_j}{1 - S_{ii}},\n  \\qquad\n  \\hat{f}_i = \\frac{1}{k} \\sum_{j \\in \\mathcal{N}_i} y_j.",
    );
}

#[test]
fn width_driven_display_migration_slice_matches_badness() {
    let body = "A = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd";
    let width = 22;
    let badness = badness_body_with_width(body, OracleContext::Display, width)
        .expect("Badness display-math oracle");
    let panache = panache_body_with_preamble_and_width(body, None, OracleContext::Display, width)
        .expect("Panache display-math formatter");
    assert_eq!(panache, badness);
}

#[test]
fn definition_relation_typed_contexts_match_badness() {
    for body in ["x:=y", "a::=b", "x:=-y", r"\mu:=\nu"] {
        for context in [OracleContext::Inline, OracleContext::Environment] {
            assert_formatter_parity(body, context);
        }
    }
}

#[test]
fn free_display_definition_relations_match_badness() {
    let cases = [
        ("A := bbbbbbbbbb := cccccccccc", 20),
        ("A :=_i bbbbbbbbbb :=_j cccccccccc", 20),
        (concat!(r"A := a \\", "\n", r":= b \\", "\n", "= c"), 80),
        (concat!(r"A :=_i a \\", "\n", r":=_j b \\", "\n", "= c"), 80),
    ];

    for (body, width) in cases {
        let badness = badness_body_with_width(body, OracleContext::Display, width)
            .expect("Badness display-math oracle");
        let panache =
            panache_body_with_preamble_and_width(body, None, OracleContext::Display, width)
                .expect("Panache display-math formatter");
        assert_eq!(panache, badness, "definition-relation display: {body:?}");

        let twice =
            panache_body_with_preamble_and_width(&panache, None, OracleContext::Display, width)
                .expect("second Panache pass");
        assert_eq!(twice, panache, "definition-relation display: {body:?}");
    }
}

/// Badness 0.10 adds two spaces after a wrapped relation when its operand
/// carries authored leading whitespace. Keep both outputs pinned so this
/// oracle defect cannot change Panache's single-space policy.
#[test]
fn panache_uses_one_space_after_wrapped_relations_where_badness_uses_two() {
    let environment = "\\begin{matrix}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{matrix}";
    let mut cases = vec![
        (
            "A := bbbbbbbbbb = cccccccccc".to_owned(),
            20,
            "  A := bbbbbbbbbb\n    = cccccccccc".to_owned(),
            "  A := bbbbbbbbbb\n    =  cccccccccc".to_owned(),
        ),
        (
            "A :=_i bbbbbbbbbb =_j cccccccccc".to_owned(),
            20,
            "  A :=_i bbbbbbbbbb\n    =_j cccccccccc".to_owned(),
            "  A :=_i bbbbbbbbbb\n    =_j  cccccccccc".to_owned(),
        ),
    ];
    for script in ["", "^T"] {
        let prefix = format!(
            "  x \\gets \\begin{{matrix}}\n            a & = {{b % inner\n                   + c}} \\\\\n            d & = e\n          \\end{{matrix}}{script}\n    \\leq"
        );
        cases.push((
            format!("x\\gets{environment}{script}\\leq y"),
            80,
            format!("{prefix} y"),
            format!("{prefix}  y"),
        ));
    }

    for (body, width, expected_panache, expected_badness) in cases {
        let panache =
            panache_body_with_preamble_and_width(&body, None, OracleContext::Display, width)
                .expect("Panache display-math formatter");
        let badness = badness_body_with_width(&body, OracleContext::Display, width)
            .expect("Badness display-math oracle");
        assert_eq!(panache, expected_panache, "{body:?}");
        assert_eq!(
            badness, expected_badness,
            "known Badness 0.10 defect: {body:?}"
        );
        let twice =
            panache_body_with_preamble_and_width(&panache, None, OracleContext::Display, width)
                .expect("second Panache pass");
        assert_eq!(twice, panache, "{body:?}");
    }
}

#[test]
fn result_classification_distinguishes_all_audit_outcomes() {
    assert!(matches!(
        classify_result(
            "same.tex",
            "same",
            OracleContext::Inline,
            Ok("same".to_owned()),
            Some("same".to_owned()),
        ),
        Classification::MandatoryByteParity
    ));
    assert!(matches!(
        classify_result(
            "inline-flattening.tex",
            "a\nb",
            OracleContext::Inline,
            Ok("a\nb".to_owned()),
            Some("a b".to_owned()),
        ),
        Classification::IntentionalDifference {
            reason: IntentionalDifference::InlineHostFlattening,
            ..
        }
    ));
    assert!(matches!(
        classify_result(
            "groups/unclosed.tex",
            "{",
            OracleContext::Inline,
            Err("wrapper".to_owned()),
            None,
        ),
        Classification::Preserved {
            reason: PreservationReason::MalformedMath,
            ..
        }
    ));
    assert!(matches!(
        classify_result(
            "unknown.tex",
            "badness",
            OracleContext::Inline,
            Ok("badness".to_owned()),
            Some("panache".to_owned()),
        ),
        Classification::Unclassified { .. }
    ));
}

#[test]
fn report_rendering_is_deterministic() {
    let records = sample_report_records();
    let forward = render_report(records.clone(), 2);
    let reverse = render_report(records.into_iter().rev().collect(), 2);
    assert_eq!(forward, reverse);
    assert!(forward.contains("Mandatory byte parity: 1 / 3"));
    assert!(forward.contains("Preserved at named boundary: 1 / 3"));
    assert!(forward.contains("Unclassified: 1 / 3"));
}

#[test]
fn complete_report_requires_an_explicit_outcome_for_every_context_run() {
    let (corpus_count, records) = collect_audit_records();
    let report = render_report(records, corpus_count);

    assert!(
        report.contains("Unclassified: 0 /"),
        "the formatter report still has unaudited context runs:\n{report}"
    );
    assert!(
        report.contains("Mandatory byte parity:"),
        "the formatter report does not identify its mandatory parity set:\n{report}"
    );
}

#[test]
fn complete_report_exercises_every_preservation_boundary_entry() {
    let (_, records) = collect_audit_records();

    for (id, expected_reason) in PRESERVATION_BOUNDARY {
        let preserved = records
            .iter()
            .filter(|record| record.id == id)
            .collect::<Vec<_>>();
        assert_eq!(
            preserved.len(),
            OracleContext::ALL.len(),
            "preservation boundary entry `{id}` does not cover every context"
        );
        for record in preserved {
            assert!(
                matches!(
                    record.classification,
                    Classification::Preserved { reason, .. } if reason == expected_reason
                ),
                "preservation boundary entry `{id}` no longer has its named reason in {:?}: {:?}",
                record.context,
                record.classification,
            );
        }
    }
}

#[test]
fn math_badness_audit_matches_committed_report() {
    let (corpus_count, records) = collect_audit_records();
    let actual = render_report(records, corpus_count);
    let path = manifest_path(REPORT_REL);
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

    similar_asserts::assert_eq!(actual, expected);
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
#[ignore = "manual: regenerate the committed Badness formatter audit"]
fn math_badness_full_report() {
    let (corpus_count, records) = collect_audit_records();
    let report = render_report(records, corpus_count);
    let path = manifest_path(REPORT_REL);
    fs::create_dir_all(path.parent().expect("report path has a parent"))
        .unwrap_or_else(|error| panic!("failed to create report directory: {error}"));
    fs::write(&path, report)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}
