//! v2-aware diagnostic validator.
//!
//! Phase-2 cutover groundwork: replaces v1's `parse_stream` sniff with
//! detection that runs over the streaming scanner's token output (and,
//! later clusters, the v2 CST). Each cluster of error-contract patterns
//! lands as its own checker function. The public entry
//! [`validate_yaml`] composes them in priority order.
//!
//! Until every uncaught pattern is covered, [`validate_yaml`] is not
//! wired into `parse_yaml_report` — it exists alongside the v1 sniff
//! while the validator grows. Once all 32 uncaught patterns are
//! covered, the v1 sniff is replaced wholesale and the line-based
//! lexer/parser bodies are deleted.
//!
//! Coverage status:
//! - **F. Directives** — implemented: directive after content,
//!   directive without `---` marker. Covers EB22, RHX7, 9MMA, B63P
//!   (4 of 5 cluster-F error contracts).
//!
//!   Known false-positive risk on M7A3 and W4TN: the streaming
//!   scanner currently emits `Directive` for `%`-prefixed lines that
//!   are actually the body of an open `|`/`>` block scalar, because
//!   it does not yet consume block-scalar bodies past the header.
//!   The fix belongs in the scanner (proper block-scalar body
//!   tokenization), not in a validator workaround. Until that lands,
//!   neither M7A3, W4TN, nor 9HCY (where the scanner subsumes a
//!   `%TAG` line into the scalar) is allowlisted.
//!
//! - **A. Trailing content after structure close** — implemented:
//!   trailing content after a closed flow sequence/map at document
//!   level (KS4U, 4H7K, 9JBA), and content on the same line as `...`
//!   (3HFZ).
//!
//! - **C. Empty/leading commas in flow** — implemented: a comma in a
//!   flow sequence or flow map with no preceding item since the last
//!   separator (covers leading-comma `[ , a ]` and consecutive
//!   commas `[ a, , b ]`). Trailing comma before the close bracket is
//!   allowed by YAML 1.2 and is intentionally not flagged. Covers
//!   fixtures 9MAG, CTN5.
//!
//! - **B. Unterminated flow at EOF** — implemented: a
//!   `YAML_FLOW_SEQUENCE` or `YAML_FLOW_MAP` whose direct children do
//!   not include a closing `]` / `}` token. Covers fixture 6JTT. The
//!   sibling case T833 (`{ foo: 1\n bar: 2 }` — flow map missing a
//!   comma between entries) is *not* an unterminated flow — the v2
//!   builder closes the map and produces a malformed
//!   `FLOW_MAP_ENTRY` with two colons inside its value. T833 belongs
//!   in cluster G (flow context anomalies) and is deferred until that
//!   lands.
//!
//! - D, E, G, H, I — pending.
//!
//! Cluster I (LHL4 — invalid tag syntax) is also deferred: the v2
//! scanner currently absorbs `!invalid{}tag scalar` as a single bare
//! scalar with no Tag token, so the validator has nothing to inspect.
//! The fix belongs in the scanner.
//!
//! See `.claude/skills/yaml-shadow-expand/scanner-rewrite.md` for the
//! cutover plan and per-cluster detection scope.
#![allow(dead_code)]

use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

use super::model::{YamlDiagnostic, diagnostic_codes};
use super::parser_v2::parse_v2;
use super::scanner::{Scanner, Token, TokenKind};

/// Run every implemented diagnostic cluster over `input`, returning the
/// first failure. Order matches the per-cluster priority chosen at
/// integration time — directive-level checks run before structural
/// checks because they govern whether a stream is even a valid stream
/// shape.
pub(crate) fn validate_yaml(input: &str) -> Option<YamlDiagnostic> {
    let tokens = collect_tokens(input);
    if let Some(diag) = check_directives(&tokens) {
        return Some(diag);
    }
    let tree = parse_v2(input);
    if let Some(diag) = check_trailing_content(&tree) {
        return Some(diag);
    }
    if let Some(diag) = check_flow_commas(&tree) {
        return Some(diag);
    }
    if let Some(diag) = check_unterminated_flow(&tree) {
        return Some(diag);
    }
    None
}

fn collect_tokens(input: &str) -> Vec<Token> {
    let mut scanner = Scanner::new(input);
    let mut tokens = Vec::new();
    while let Some(tok) = scanner.next_token() {
        tokens.push(tok);
    }
    tokens
}

/// Cluster F — directive ordering and lone-directive checks.
///
/// Surfaces two failures, both driven off scanner-emitted `Directive`
/// tokens:
/// - `PARSE_DIRECTIVE_AFTER_CONTENT` when a directive appears after
///   non-trivia, non-`...` content. YAML 1.2 requires a `...`
///   document end before subsequent directives.
/// - `PARSE_DIRECTIVE_WITHOUT_DOCUMENT_START` when any directive is
///   present but no `---` marker exists in the stream. A directive
///   without `---` has no document to attach to.
///
/// The streaming scanner emits `Directive` only when a `%`-prefixed
/// line is in a directive position (stream start, or after `...`).
/// Lines that look like directives but are scalar continuations,
/// block-scalar bodies, or flow-context content are correctly *not*
/// emitted as directives — so this check inherits the scanner's
/// spec-correct view.
///
/// Covers fixtures EB22, RHX7, 9MMA, B63P.
fn check_directives(tokens: &[Token]) -> Option<YamlDiagnostic> {
    let mut seen_content = false;
    for tok in tokens {
        match tok.kind {
            TokenKind::Directive if seen_content => {
                return Some(diag_at_token(
                    tok,
                    diagnostic_codes::PARSE_DIRECTIVE_AFTER_CONTENT,
                    "directive requires document end before subsequent directives",
                ));
            }
            TokenKind::Directive
            | TokenKind::Trivia(_)
            | TokenKind::StreamStart
            | TokenKind::StreamEnd => {}
            TokenKind::DocumentEnd => seen_content = false,
            _ => seen_content = true,
        }
    }

    if let Some(directive) = tokens.iter().find(|t| t.kind == TokenKind::Directive)
        && !tokens.iter().any(|t| t.kind == TokenKind::DocumentStart)
    {
        return Some(diag_at_token(
            directive,
            diagnostic_codes::PARSE_DIRECTIVE_WITHOUT_DOCUMENT_START,
            "directive requires an explicit document start marker",
        ));
    }

    None
}

fn diag_at_token(tok: &Token, code: &'static str, message: &'static str) -> YamlDiagnostic {
    YamlDiagnostic {
        code,
        message,
        byte_start: tok.start.index,
        byte_end: tok.end.index,
    }
}

/// Cluster A — trailing content after a structure close at document
/// level.
///
/// Two failures are surfaced:
/// - `PARSE_TRAILING_CONTENT_AFTER_FLOW_END` when a `YAML_DOCUMENT`
///   contains body content after a `YAML_FLOW_SEQUENCE` /
///   `YAML_FLOW_MAP` has closed (KS4U, 4H7K, 9JBA). A spaceless `]#`
///   sequence (parsed as `YAML_COMMENT` by the scanner) also counts —
///   YAML 1.2 §6.6 requires whitespace before `#`.
/// - `LEX_TRAILING_CONTENT_AFTER_DOCUMENT_END` when content appears on
///   the same line as a `...` document-end marker (3HFZ).
///
/// Covers fixtures KS4U, 4H7K, 9JBA, 3HFZ.
fn check_trailing_content(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for doc in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
    {
        if let Some(diag) = check_trailing_after_flow(&doc) {
            return Some(diag);
        }
    }
    if let Some(diag) = check_trailing_after_doc_end(tree) {
        return Some(diag);
    }
    None
}

/// Detects trailing content after a closed flow sequence/map at
/// document level. Walks the document's direct children: after a
/// `YAML_FLOW_SEQUENCE` or `YAML_FLOW_MAP`, the only legal followers
/// are pure trivia (whitespace, newlines, properly-spaced comments),
/// a `YAML_DOCUMENT_END` marker, or a `YAML_BLOCK_MAP` whose first
/// entry's key is colon-only — that shape encodes the YAML 1.2
/// "flow-collection-as-implicit-key" form (e.g. `[flow]: block` or
/// `{a: b}: c`).
fn check_trailing_after_flow(doc: &SyntaxNode) -> Option<YamlDiagnostic> {
    let mut after_flow = false;
    let mut have_separator = false;
    for child in doc.children_with_tokens() {
        match &child {
            NodeOrToken::Node(n) => {
                let kind = n.kind();
                if matches!(
                    kind,
                    SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
                ) {
                    if after_flow {
                        // Two flow structures back-to-back — second is trailing content.
                        return Some(diag_at_range(
                            n.text_range().start().into(),
                            n.text_range().end().into(),
                            diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END,
                            "unexpected content after flow-collection close",
                        ));
                    }
                    after_flow = true;
                    have_separator = false;
                } else if after_flow {
                    if kind == SyntaxKind::YAML_BLOCK_MAP && is_implicit_flow_key_block_map(n) {
                        // Flow used as the implicit key of a block-map
                        // entry (`[flow]: block`). The flow node and
                        // the block-map sibling jointly form the entry.
                        after_flow = false;
                        have_separator = false;
                        continue;
                    }
                    return Some(diag_at_range(
                        n.text_range().start().into(),
                        n.text_range().end().into(),
                        diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END,
                        "unexpected content after flow-collection close",
                    ));
                }
            }
            NodeOrToken::Token(t) => {
                if !after_flow {
                    continue;
                }
                match t.kind() {
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {
                        have_separator = true;
                    }
                    SyntaxKind::YAML_COMMENT => {
                        if !have_separator {
                            // Spaceless `]#…` — scanner emitted a comment, but
                            // YAML §6.6 requires whitespace before `#`. The
                            // bytes are trailing content, not a comment.
                            return Some(diag_at_range(
                                t.text_range().start().into(),
                                t.text_range().end().into(),
                                diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END,
                                "comment must be preceded by whitespace after flow-collection close",
                            ));
                        }
                    }
                    SyntaxKind::YAML_DOCUMENT_END => {
                        // `...` legitimately follows a flow document.
                        after_flow = false;
                        have_separator = false;
                    }
                    _ => {
                        return Some(diag_at_range(
                            t.text_range().start().into(),
                            t.text_range().end().into(),
                            diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END,
                            "unexpected content after flow-collection close",
                        ));
                    }
                }
            }
        }
    }
    None
}

/// Returns true when `block_map`'s first `YAML_BLOCK_MAP_ENTRY` has a
/// `YAML_BLOCK_MAP_KEY` containing only the `:` colon (and trivia).
/// The v2 builder produces this shape when a flow sequence/map is used
/// as the implicit key of a block-map entry — the actual key bytes
/// live in the *preceding sibling* flow node, and the block-map
/// itself starts with a bare-colon key.
fn is_implicit_flow_key_block_map(block_map: &SyntaxNode) -> bool {
    let Some(entry) = block_map
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
    else {
        return false;
    };
    let Some(key) = entry
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
    else {
        return false;
    };
    key.children_with_tokens().all(|c| {
        matches!(
            c.kind(),
            SyntaxKind::YAML_COLON
                | SyntaxKind::WHITESPACE
                | SyntaxKind::NEWLINE
                | SyntaxKind::YAML_COMMENT
        )
    })
}

/// Detects content on the same line as a `...` document-end marker.
/// Walks every `YAML_DOCUMENT_END` token; scans forward in the linear
/// token stream until a `NEWLINE` (legal end-of-line) or the end of
/// input. Anything other than whitespace or a properly-spaced comment
/// before that newline is illegal trailing content.
fn check_trailing_after_doc_end(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    let tokens: Vec<_> = tree
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .collect();
    for (i, tok) in tokens.iter().enumerate() {
        if tok.kind() != SyntaxKind::YAML_DOCUMENT_END {
            continue;
        }
        let mut have_separator = false;
        for next in &tokens[i + 1..] {
            match next.kind() {
                SyntaxKind::NEWLINE => break,
                SyntaxKind::WHITESPACE => {
                    have_separator = true;
                }
                SyntaxKind::YAML_COMMENT if have_separator => break,
                SyntaxKind::YAML_COMMENT => {
                    // Spaceless `...#` is malformed.
                    return Some(diag_at_range(
                        next.text_range().start().into(),
                        next.text_range().end().into(),
                        diagnostic_codes::LEX_TRAILING_CONTENT_AFTER_DOCUMENT_END,
                        "comment must be preceded by whitespace after document end marker",
                    ));
                }
                _ => {
                    return Some(diag_at_range(
                        next.text_range().start().into(),
                        next.text_range().end().into(),
                        diagnostic_codes::LEX_TRAILING_CONTENT_AFTER_DOCUMENT_END,
                        "unexpected content on the same line as document end marker",
                    ));
                }
            }
        }
    }
    None
}

/// Cluster C — empty / leading commas inside flow collections.
///
/// In YAML 1.2 a flow sequence or flow map separator (`,`) must be
/// preceded by an item since the previous separator (or since the
/// opening bracket). A leading comma (`[ , a ]`) or two consecutive
/// commas with only whitespace between them (`[ a, , b ]`) are
/// rejected with `PARSE_INVALID_FLOW_SEQUENCE_COMMA`.
///
/// A trailing comma immediately before the closing bracket
/// (`[ a, b, ]`) is **legal** YAML and is intentionally not flagged —
/// the check tracks "item seen since last separator" but doesn't
/// require an item to follow the final separator.
///
/// The v2 builder stores `[`, `]`, `{`, `}`, and `,` as `YAML_SCALAR`
/// children directly on the `YAML_FLOW_SEQUENCE` / `YAML_FLOW_MAP`
/// node; real content lives inside `YAML_FLOW_SEQUENCE_ITEM` /
/// `YAML_FLOW_MAP_ENTRY` siblings, so a structural-token vs. content
/// distinction at this level is just a text comparison.
///
/// Covers fixtures 9MAG, CTN5.
fn check_flow_commas(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for flow in tree.descendants().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
        )
    }) {
        if let Some(diag) = check_flow_node_commas(&flow) {
            return Some(diag);
        }
    }
    None
}

fn check_flow_node_commas(flow: &SyntaxNode) -> Option<YamlDiagnostic> {
    let mut seen_item_since_separator = false;
    for child in flow.children_with_tokens() {
        match &child {
            // Any nested node — `YAML_FLOW_MAP_ENTRY`,
            // `YAML_FLOW_SEQUENCE_ITEM`, or a nested flow collection —
            // is an item.
            NodeOrToken::Node(_) => {
                seen_item_since_separator = true;
            }
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::YAML_COMMENT => {}
                SyntaxKind::YAML_SCALAR if t.text() == "," => {
                    if !seen_item_since_separator {
                        return Some(diag_at_range(
                            t.text_range().start().into(),
                            t.text_range().end().into(),
                            diagnostic_codes::PARSE_INVALID_FLOW_SEQUENCE_COMMA,
                            "comma must follow a flow-collection item",
                        ));
                    }
                    seen_item_since_separator = false;
                }
                // Structural opener/closer brackets — neutral.
                SyntaxKind::YAML_SCALAR if matches!(t.text(), "[" | "]" | "{" | "}") => {}
                // Any other token — bare scalar (implicit-null map
                // entry like `single line` in `{ single line, a: b }`,
                // or a plain-value entry in `{ http://foo.com, … }`),
                // anchor, tag, etc. — counts as item evidence.
                _ => {
                    seen_item_since_separator = true;
                }
            },
        }
    }
    None
}

/// Cluster B — unterminated flow collection at EOF.
///
/// A `YAML_FLOW_SEQUENCE` whose direct children include no `]` token,
/// or a `YAML_FLOW_MAP` whose direct children include no `}` token,
/// reached EOF without closing. Note that nested flow brackets live
/// inside `YAML_FLOW_SEQUENCE_ITEM` / `YAML_FLOW_MAP_ENTRY` wrappers,
/// not as direct children — so an inner `]` does not satisfy an
/// outer flow's close requirement.
///
/// Covers fixture 6JTT.
fn check_unterminated_flow(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for flow in tree.descendants().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
        )
    }) {
        let close = if flow.kind() == SyntaxKind::YAML_FLOW_SEQUENCE {
            "]"
        } else {
            "}"
        };
        let has_close = flow.children_with_tokens().any(|c| {
            c.as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_SCALAR && t.text() == close)
        });
        if !has_close {
            let (code, message) = if flow.kind() == SyntaxKind::YAML_FLOW_SEQUENCE {
                (
                    diagnostic_codes::PARSE_UNTERMINATED_FLOW_SEQUENCE,
                    "flow sequence reached end of input without `]`",
                )
            } else {
                (
                    diagnostic_codes::PARSE_UNTERMINATED_FLOW_MAP,
                    "flow mapping reached end of input without `}`",
                )
            };
            return Some(diag_at_range(
                flow.text_range().start().into(),
                flow.text_range().end().into(),
                code,
                message,
            ));
        }
    }
    None
}

fn diag_at_range(
    byte_start: usize,
    byte_end: usize,
    code: &'static str,
    message: &'static str,
) -> YamlDiagnostic {
    YamlDiagnostic {
        code,
        message,
        byte_start,
        byte_end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str) -> Option<YamlDiagnostic> {
        validate_yaml(input)
    }

    #[test]
    fn directive_after_content_eb22() {
        // EB22: scalar content, then a fresh directive without intervening `...`.
        let input = "---\nscalar1 # comment\n%YAML 1.2\n---\nscalar2\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_DIRECTIVE_AFTER_CONTENT);
    }

    #[test]
    fn directive_after_content_rhx7() {
        // RHX7: block-map content, then `%YAML 1.2` without `...` between.
        let input = "---\nkey: value\n%YAML 1.2\n---\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_DIRECTIVE_AFTER_CONTENT);
    }

    #[test]
    fn directive_without_document_start_9mma() {
        // 9MMA: bare `%YAML 1.2` with no `---` anywhere.
        let input = "%YAML 1.2\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_DIRECTIVE_WITHOUT_DOCUMENT_START
        );
    }

    #[test]
    fn directive_without_document_start_b63p() {
        // B63P: directive followed by `...` only — `...` is DocumentEnd, not DocumentStart.
        let input = "%YAML 1.2\n...\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_DIRECTIVE_WITHOUT_DOCUMENT_START
        );
    }

    #[test]
    fn well_formed_directive_then_marker_passes() {
        // Sanity: `%YAML 1.2\n---\nfoo: bar\n` is well-formed.
        let input = "%YAML 1.2\n---\nfoo: bar\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn directive_then_doc_then_directive_with_separator_passes() {
        // Two-document stream with proper `...` separator between
        // them must NOT trigger PARSE_DIRECTIVE_AFTER_CONTENT.
        let input = "%YAML 1.2\n---\nfoo: 1\n...\n%YAML 1.2\n---\nbar: 2\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn empty_input_passes() {
        assert!(run("").is_none());
    }

    #[test]
    fn plain_document_no_directives_passes() {
        let input = "key: value\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn plain_scalar_continuation_with_percent_passes_xlq9() {
        // XLQ9: `scalar\n%YAML 1.2` is a single multi-line plain
        // scalar (`%YAML 1.2` is the continuation line), not a
        // directive. The scanner correctly emits one Scalar token,
        // no Directive.
        let input = "---\nscalar\n%YAML 1.2\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn percent_at_col0_inside_flow_map_is_content_ut92() {
        // UT92: `% : 20 }` is a flow-map key inside an open `{...}`.
        // The scanner does not emit a Directive token here because we
        // are still in an open flow context.
        let input = "---\n{ matches\n% : 20 }\n...\n---\n# Empty\n...\n";
        assert!(run(input).is_none());
    }

    // M7A3, W4TN, 9HCY tests intentionally absent — their correct
    // resolution depends on scanner-side fixes (proper block-scalar
    // body tokenization for M7A3/W4TN; tighter quoted-scalar closure
    // for 9HCY). The module-level docstring captures the gap.

    // ---- Cluster A: trailing content after structure close ----

    #[test]
    fn trailing_content_after_doc_end_3hfz() {
        // 3HFZ: `... invalid` — content on the same line as `...`.
        let input = "---\nkey: value\n... invalid\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::LEX_TRAILING_CONTENT_AFTER_DOCUMENT_END
        );
    }

    #[test]
    fn trailing_content_after_flow_seq_ks4u() {
        // KS4U: `[ ... ]\ninvalid item` — bare scalar after flow seq close.
        let input = "---\n[\nsequence item\n]\ninvalid item\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END
        );
    }

    #[test]
    fn trailing_extra_flow_closer_4h7k() {
        // 4H7K: `[ a, b, c ] ]` — extra `]` after flow seq close.
        let input = "---\n[ a, b, c ] ]\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END
        );
    }

    #[test]
    fn trailing_spaceless_comment_after_flow_9jba() {
        // 9JBA: `]#invalid` — `#invalid` directly adjacent to `]`.
        // Per YAML §6.6, a comment must be preceded by whitespace; the
        // scanner emits this as YAML_COMMENT but it is malformed.
        let input = "---\n[ a, b, c, ]#invalid\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END
        );
    }

    #[test]
    fn flow_then_properly_spaced_comment_passes() {
        // Sanity: `[a, b] # ok` — properly-spaced comment after `]` is fine.
        let input = "---\n[ a, b ] # ok\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn flow_then_doc_end_passes() {
        // Sanity: a flow document followed by `...` is well-formed.
        let input = "---\n[ a, b ]\n...\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn doc_end_then_newline_then_content_is_valid_new_doc() {
        // `...` ending a doc, then NEWLINE, then a fresh doc body — fine.
        let input = "---\nfirst\n...\nsecond\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn doc_end_with_trailing_spaced_comment_passes() {
        // `... # comment` — comment after `...` with whitespace separator is fine.
        let input = "---\nkey: value\n... # comment\n";
        assert!(run(input).is_none());
    }

    // ---- Cluster C: empty / leading commas in flow ----

    #[test]
    fn flow_seq_leading_comma_9mag() {
        // 9MAG: `[ , a, b, c ]` — leading comma with no preceding item.
        let input = "---\n[ , a, b, c ]\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_INVALID_FLOW_SEQUENCE_COMMA
        );
    }

    #[test]
    fn flow_seq_double_comma_ctn5() {
        // CTN5: `[ a, b, c, , ]` — empty entry between commas.
        let input = "---\n[ a, b, c, , ]\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_INVALID_FLOW_SEQUENCE_COMMA
        );
    }

    #[test]
    fn flow_map_leading_comma_rejects() {
        // `{ , a: 1 }` — same shape as 9MAG but in a flow map.
        let input = "---\n{ , a: 1 }\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_INVALID_FLOW_SEQUENCE_COMMA
        );
    }

    #[test]
    fn flow_map_double_comma_rejects() {
        // `{ a: 1, , b: 2 }` — same shape as CTN5 but in a flow map.
        let input = "---\n{ a: 1, , b: 2 }\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_INVALID_FLOW_SEQUENCE_COMMA
        );
    }

    #[test]
    fn flow_seq_trailing_comma_passes() {
        // YAML 1.2 allows a trailing comma immediately before the close
        // bracket — the validator must not flag this as invalid.
        let input = "---\n[ a, b, c, ]\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn flow_map_trailing_comma_passes() {
        // Same trailing-comma allowance for flow maps (covers fixture 5C5M).
        let input = "---\n{ a: 1, b: 2, }\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn flow_seq_well_formed_passes() {
        let input = "---\n[ a, b, c ]\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn flow_seq_empty_passes() {
        // No commas at all in an empty flow sequence.
        let input = "---\n[ ]\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn flow_map_implicit_null_entry_passes_8kb6() {
        // 8KB6: `{ single line, a: b }` — `single line` is a key with
        // implicit-null value. The v2 builder emits it as a bare
        // YAML_SCALAR child of YAML_FLOW_MAP, not wrapped in
        // YAML_FLOW_MAP_ENTRY. The validator must recognize that bare
        // scalar as item evidence so the following comma is legal.
        let input = "---\n- { single line, a: b}\n- { multi\n  line, a: b}\n";
        assert!(run(input).is_none());
    }

    // ---- Cluster B: unterminated flow at EOF ----

    #[test]
    fn unterminated_flow_seq_6jtt() {
        // 6JTT: `[ [ a, b, c ]` — outer `[` never closes (inner does).
        let input = "---\n[ [ a, b, c ]\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_UNTERMINATED_FLOW_SEQUENCE
        );
    }

    #[test]
    fn unterminated_flow_map() {
        // `{ foo: 1` — flow map open, no close.
        let input = "---\n{ foo: 1\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNTERMINATED_FLOW_MAP);
    }

    #[test]
    fn balanced_nested_flow_passes() {
        let input = "---\n[ [ a, b, c ] ]\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn empty_flow_seq_terminated_passes() {
        // Sanity: `[ ]` closes immediately.
        let input = "---\n[ ]\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn flow_map_plain_entry_passes_4abk() {
        // 4ABK: `{ unquoted : "separate", http://foo.com, … }` — the
        // bare `http://foo.com` is a plain-scalar entry with implicit
        // null. Same shape concern as 8KB6: a comma after an unwrapped
        // bare scalar must not be flagged.
        let input = "{\nunquoted : \"separate\",\nhttp://foo.com,\nomitted value:,\n}\n";
        assert!(run(input).is_none());
    }
}
