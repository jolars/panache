//! Differential structural oracle for Panache and Badness math CSTs.
//!
//! The projectors in this test are intentionally mechanical. They rename kinds,
//! shift Badness's `$...$` wrapper offset, and remove Panache host trivia that
//! can be interleaved into `MATH_CONTENT`. They do not parse TeX, infer command
//! arguments, attach scripts, or repair either tree.

use std::fmt::Write as _;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use badness_parser::parser::parse as parse_badness;
use badness_parser::rowan::NodeOrToken as BadnessNodeOrToken;
use badness_parser::syntax::{
    SyntaxElement as BadnessElement, SyntaxKind as BadnessKind, SyntaxNode as BadnessNode,
};
use panache_parser::parser::math::{MathParseOptions, parse_math_content};
use panache_parser::syntax::{
    SyntaxElement as PanacheElement, SyntaxKind as PanacheKind, SyntaxNode as PanacheNode,
};
use rowan::NodeOrToken as PanacheNodeOrToken;

const PASSING_REL: &str = "tests/math_badness/passing.txt";
const REPORT_REL: &str = "tests/math_badness/report.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonicalElement {
    kind: String,
    range: Range<usize>,
    text: Option<String>,
    gaps: Vec<String>,
    children: Vec<CanonicalElement>,
}

/// The canonical spelling is the locked kind map. Within each source language
/// every source kind has a distinct spelling; equal spellings across the two
/// tables are the only kinds claimed to be equivalent. Legacy Panache kinds use
/// an explicit `PANACHE_` spelling so the oracle cannot hide a migration gap.
const BADNESS_MATH_KIND_MAP: &[(BadnessKind, &str)] = &[
    (BadnessKind::CONTROL_WORD, "CONTROL_WORD"),
    (BadnessKind::CONTROL_SYMBOL, "CONTROL_SYMBOL"),
    (BadnessKind::L_BRACE, "GROUP_OPEN"),
    (BadnessKind::R_BRACE, "GROUP_CLOSE"),
    (BadnessKind::L_BRACKET, "BRACKET_OPEN"),
    (BadnessKind::R_BRACKET, "BRACKET_CLOSE"),
    (BadnessKind::DOLLAR, "DOLLAR"),
    (BadnessKind::AMPERSAND, "ALIGN"),
    (BadnessKind::HASH, "HASH"),
    (BadnessKind::CARET, "CARET"),
    (BadnessKind::UNDERSCORE, "UNDERSCORE"),
    (BadnessKind::TILDE, "TILDE"),
    (BadnessKind::COMMENT, "COMMENT"),
    (BadnessKind::WHITESPACE, "SPACE"),
    (BadnessKind::NEWLINE, "NEWLINE"),
    (BadnessKind::WORD, "WORD"),
    (BadnessKind::VERB, "VERB"),
    (BadnessKind::VERBATIM_BODY, "VERBATIM_BODY"),
    (BadnessKind::DOC_MARGIN, "BADNESS_DOC_MARGIN"),
    (BadnessKind::GUARD, "BADNESS_GUARD"),
    (BadnessKind::ERROR, "ERROR"),
    (BadnessKind::GROUP, "GROUP"),
    (BadnessKind::OPTIONAL, "OPTIONAL"),
    (BadnessKind::ARGUMENT, "ARGUMENT"),
    (BadnessKind::COMMAND, "COMMAND"),
    (BadnessKind::ENVIRONMENT, "ENVIRONMENT"),
    (BadnessKind::BEGIN, "BEGIN"),
    (BadnessKind::END, "END"),
    (BadnessKind::NAME_GROUP, "NAME_GROUP"),
    (BadnessKind::CONDITIONAL, "CONDITIONAL"),
    (BadnessKind::CONDITIONAL_BRANCH, "CONDITIONAL_BRANCH"),
    (BadnessKind::INLINE_MATH, "BADNESS_INLINE_MATH"),
    (BadnessKind::DISPLAY_MATH, "BADNESS_DISPLAY_MATH"),
    (BadnessKind::MATH, "MATH"),
    (BadnessKind::SCRIPTED, "SCRIPTED"),
    (BadnessKind::SUBSCRIPT, "SUBSCRIPT"),
    (BadnessKind::SUPERSCRIPT, "SUPERSCRIPT"),
    (BadnessKind::LEFT_RIGHT, "LEFT_RIGHT"),
    (BadnessKind::PARAGRAPH, "BADNESS_PARAGRAPH"),
    (BadnessKind::DOC_COMMENT, "BADNESS_DOC_COMMENT"),
    (BadnessKind::TEXT, "TEXT"),
    (BadnessKind::LINE_BREAK, "LINE_BREAK"),
    (BadnessKind::STATEMENT, "STATEMENT"),
    (BadnessKind::ROOT, "BADNESS_ROOT"),
];

const PANACHE_MATH_KIND_MAP: &[(PanacheKind, &str)] = &[
    (PanacheKind::MATH_CONTENT, "MATH"),
    (PanacheKind::MATH_GROUP, "GROUP"),
    (PanacheKind::MATH_OPTIONAL, "OPTIONAL"),
    (PanacheKind::MATH_ENVIRONMENT, "ENVIRONMENT"),
    (PanacheKind::MATH_BEGIN, "BEGIN"),
    (PanacheKind::MATH_END, "END"),
    (PanacheKind::MATH_NAME_GROUP, "NAME_GROUP"),
    (PanacheKind::MATH_DELIMITED, "LEFT_RIGHT"),
    (PanacheKind::MATH_SCRIPTED, "SCRIPTED"),
    (PanacheKind::MATH_SUBSCRIPT, "SUBSCRIPT"),
    (PanacheKind::MATH_SUPERSCRIPT, "SUPERSCRIPT"),
    (PanacheKind::MATH_GROUP_OPEN, "GROUP_OPEN"),
    (PanacheKind::MATH_GROUP_CLOSE, "GROUP_CLOSE"),
    (PanacheKind::MATH_BRACKET_OPEN, "BRACKET_OPEN"),
    (PanacheKind::MATH_BRACKET_CLOSE, "BRACKET_CLOSE"),
    (PanacheKind::MATH_COMMAND, "COMMAND"),
    (PanacheKind::MATH_CONTROL_WORD, "CONTROL_WORD"),
    (PanacheKind::MATH_CONTROL_SYMBOL, "CONTROL_SYMBOL"),
    (PanacheKind::MATH_LINE_BREAK, "LINE_BREAK"),
    (PanacheKind::MATH_ALIGN, "ALIGN"),
    (PanacheKind::MATH_CARET, "CARET"),
    (PanacheKind::MATH_UNDERSCORE, "UNDERSCORE"),
    (PanacheKind::MATH_COMMENT, "COMMENT"),
    (PanacheKind::MATH_WORD, "WORD"),
    (PanacheKind::MATH_SPACE, "SPACE"),
    (PanacheKind::MATH_NEWLINE, "NEWLINE"),
    (
        PanacheKind::MATH_EQUATION_LABEL,
        "PANACHE_HOST_EQUATION_LABEL",
    ),
];

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn corpus_root() -> PathBuf {
    manifest_path("../panache-formatter/tests/fixtures/math_corpus")
}

#[path = "../../panache-formatter/tests/common/math_corpus.rs"]
mod math_corpus;
use math_corpus::discover_cases;

fn read_passing() -> Vec<String> {
    let path = manifest_path(PASSING_REL);
    let mut cases = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    cases.sort();
    cases
}

fn badness_kind(kind: BadnessKind) -> String {
    mapped_kind(BADNESS_MATH_KIND_MAP, kind, "Badness").to_owned()
}

fn panache_kind(kind: PanacheKind) -> String {
    mapped_kind(PANACHE_MATH_KIND_MAP, kind, "Panache").to_owned()
}

fn mapped_kind<K: Copy + PartialEq + std::fmt::Debug>(
    mapping: &[(K, &'static str)],
    kind: K,
    language: &str,
) -> &'static str {
    mapping
        .iter()
        .find_map(|(source, canonical)| (*source == kind).then_some(*canonical))
        .unwrap_or_else(|| panic!("unmapped {language} math kind: {kind:?}"))
}

fn usize_range(range: rowan::TextRange) -> Range<usize> {
    u32::from(range.start()) as usize..u32::from(range.end()) as usize
}

fn project_badness(body: &str) -> CanonicalElement {
    let wrapped = format!("${body}$");
    let parsed = parse_badness(&wrapped);
    assert_eq!(parsed.syntax().to_string(), wrapped, "Badness losslessness");
    let inline = parsed
        .syntax()
        .descendants()
        .find(|node| node.kind() == BadnessKind::INLINE_MATH)
        .unwrap_or_else(|| panic!("Badness did not recognize the wrapped body: {body:?}"));
    let math = inline
        .children()
        .find(|node| node.kind() == BadnessKind::MATH)
        .expect("Badness inline math has no direct MATH child");
    assert_eq!(math.to_string(), body, "Badness wrapper changed the body");
    project_badness_node(&math, body, 1)
}

fn project_badness_node(
    node: &BadnessNode,
    source: &str,
    wrapper_offset: usize,
) -> CanonicalElement {
    let children = node
        .children_with_tokens()
        .map(|element| project_badness_element(element, source, wrapper_offset))
        .collect::<Vec<_>>();
    let raw = usize_range(node.text_range());
    let range = raw.start - wrapper_offset..raw.end - wrapper_offset;
    CanonicalElement {
        kind: badness_kind(node.kind()),
        gaps: source_gaps(source, &range, &children),
        range,
        text: None,
        children,
    }
}

fn project_badness_element(
    element: BadnessElement,
    source: &str,
    wrapper_offset: usize,
) -> CanonicalElement {
    match element {
        BadnessNodeOrToken::Node(node) => project_badness_node(&node, source, wrapper_offset),
        BadnessNodeOrToken::Token(token) => {
            let raw = usize_range(token.text_range());
            CanonicalElement {
                kind: badness_kind(token.kind()),
                range: raw.start - wrapper_offset..raw.end - wrapper_offset,
                text: Some(token.text().to_owned()),
                gaps: Vec::new(),
                children: Vec::new(),
            }
        }
    }
}

fn project_panache(body: &str) -> CanonicalElement {
    let green = parse_math_content(body, MathParseOptions::default());
    let math = PanacheNode::new_root(green);
    assert_eq!(math.to_string(), body, "Panache losslessness");
    project_panache_node(&math, body)
}

fn project_panache_node(node: &PanacheNode, source: &str) -> CanonicalElement {
    let children = node
        .children_with_tokens()
        .filter(|element| !is_documented_host_trivia(element))
        .map(|element| project_panache_element(element, source))
        .collect::<Vec<_>>();
    let range = usize_range(node.text_range());
    CanonicalElement {
        kind: panache_kind(node.kind()),
        gaps: source_gaps(source, &range, &children),
        range,
        text: None,
        children,
    }
}

fn project_panache_element(element: PanacheElement, source: &str) -> CanonicalElement {
    match element {
        PanacheNodeOrToken::Node(node) => project_panache_node(&node, source),
        PanacheNodeOrToken::Token(token) => CanonicalElement {
            kind: panache_kind(token.kind()),
            range: usize_range(token.text_range()),
            text: Some(token.text().to_owned()),
            gaps: Vec::new(),
            children: Vec::new(),
        },
    }
}

/// Host block machinery may splice only these non-math kinds into a realized
/// `MATH_CONTENT`. Bare corpus inputs do not contain them, but keeping the
/// omission here documents the sole exception allowed to the projector.
fn is_documented_host_trivia(element: &PanacheElement) -> bool {
    matches!(
        element.kind(),
        PanacheKind::LINE_PREFIX | PanacheKind::WHITESPACE | PanacheKind::NEWLINE
    )
}

fn source_gaps(source: &str, parent: &Range<usize>, children: &[CanonicalElement]) -> Vec<String> {
    let mut gaps = Vec::with_capacity(children.len() + 1);
    let mut cursor = parent.start;
    for child in children {
        gaps.push(source[cursor..child.range.start].to_owned());
        cursor = child.range.end;
    }
    gaps.push(source[cursor..parent.end].to_owned());
    gaps
}

fn render(element: &CanonicalElement, depth: usize, output: &mut String) {
    let indent = depth * 2;
    if let Some(text) = &element.text {
        writeln!(
            output,
            "{:indent$}{}@{}..{} {:?}",
            "", element.kind, element.range.start, element.range.end, text
        )
        .unwrap();
    } else {
        writeln!(
            output,
            "{:indent$}{}@{}..{} gaps={:?}",
            "", element.kind, element.range.start, element.range.end, element.gaps
        )
        .unwrap();
        for child in &element.children {
            render(child, depth + 1, output);
        }
    }
}

fn rendered(element: &CanonicalElement) -> String {
    let mut output = String::new();
    render(element, 0, &mut output);
    output
}

fn projections(body: &str) -> (CanonicalElement, CanonicalElement) {
    (project_badness(body), project_panache(body))
}

#[test]
fn canonical_kind_maps_are_explicit_and_one_to_one() {
    fn assert_injective<K: Copy + std::fmt::Debug>(language: &str, mapping: &[(K, &'static str)]) {
        let mut canonical = std::collections::BTreeSet::new();
        for (kind, name) in mapping {
            assert!(
                canonical.insert(*name),
                "{language} kinds collapse at canonical `{name}` (latest source kind: {kind:?})"
            );
        }
    }

    assert_injective("Badness", BADNESS_MATH_KIND_MAP);
    assert_injective("Panache", PANACHE_MATH_KIND_MAP);
}

#[test]
fn passing_corpus_has_structural_parity() {
    let root = corpus_root();
    let available = discover_cases(&root)
        .into_iter()
        .map(|path| {
            path.strip_prefix(&root)
                .expect("corpus case outside root")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect::<std::collections::BTreeSet<_>>();
    let passing = read_passing();
    assert!(!passing.is_empty(), "the mandatory parity corpus is empty");

    for id in passing {
        assert!(available.contains(&id), "missing parity case `{id}`");
        let body = fs::read_to_string(root.join(&id))
            .unwrap_or_else(|error| panic!("failed to read `{id}`: {error}"));
        let (badness, panache) = projections(&body);
        assert_eq!(
            panache,
            badness,
            "structural parity failed for `{id}`\n\nBadness:\n{}\nPanache:\n{}",
            rendered(&badness),
            rendered(&panache),
        );
    }
}

#[test]
#[ignore = "manual: regenerate the committed Badness differential report"]
fn math_badness_full_report() {
    let root = corpus_root();
    let mut passing = Vec::new();
    let mut divergent = Vec::new();

    for path in discover_cases(&root) {
        let id = path
            .strip_prefix(&root)
            .expect("corpus case outside root")
            .to_string_lossy()
            .replace('\\', "/");
        let body = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read `{id}`: {error}"));
        let (badness, panache) = projections(&body);
        if badness == panache {
            passing.push(id);
        } else {
            divergent.push((id, rendered(&badness), rendered(&panache)));
        }
    }

    let total = passing.len() + divergent.len();
    let mut report = String::new();
    writeln!(report, "Badness math parser parity report").unwrap();
    writeln!(report, "Oracle: badness-parser =0.4.0").unwrap();
    writeln!(
        report,
        "Corpus: ../panache-formatter/tests/fixtures/math_corpus"
    )
    .unwrap();
    writeln!(report, "Passing: {} / {total}", passing.len()).unwrap();
    writeln!(report, "Divergent: {} / {total}\n", divergent.len()).unwrap();
    writeln!(report, "Regenerate with:").unwrap();
    writeln!(
        report,
        "  cargo test -p panache-parser --test math_badness_parity math_badness_full_report -- --ignored --nocapture\n"
    )
    .unwrap();
    writeln!(report, "=== Passing candidates ===").unwrap();
    for id in &passing {
        writeln!(report, "{id}").unwrap();
    }
    writeln!(report, "\n=== Remaining divergences ===").unwrap();
    for (id, badness, panache) in divergent {
        writeln!(report, "\n--- {id} ---").unwrap();
        writeln!(report, "Badness:\n{badness}Panache:\n{panache}").unwrap();
    }

    let path = manifest_path(REPORT_REL);
    fs::write(&path, report)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}
