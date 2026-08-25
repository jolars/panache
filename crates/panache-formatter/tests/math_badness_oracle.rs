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
    writeln!(report, "Oracle: badness-formatter =0.5.0").unwrap();
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

fn assert_formatter_parity(body: &str, context: OracleContext) {
    let badness = badness_body(body, context).unwrap_or_else(|error| panic!("{error}"));
    let panache = panache_body(body, context).unwrap_or_else(|error| panic!("{error}"));
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
    for body in [
        "- x", "x = - y", "f( - x)", "x:=y", "x:=-y", "a::=b", "a/b", "a/ b", "a /b",
    ] {
        assert_formatter_parity(body, OracleContext::Inline);
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
