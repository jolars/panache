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
//!   not include a closing `]` / `}` token. Covers fixture 6JTT.
//!
//! - **G. Flow context anomalies** — implemented (partial):
//!   - DK4H / ZXT5: an implicit-key entry inside a `YAML_FLOW_SEQUENCE_ITEM`
//!     whose key shape spans a newline (either embedded `\n` in the
//!     plain key text, or a `NEWLINE` token between the key scalar
//!     and its colon). YAML 1.2 requires implicit keys in flow
//!     context to fit on a single line.
//!   - T833: a `YAML_FLOW_MAP_VALUE` containing a stray `YAML_COLON`
//!     token directly — indicating a missing comma between flow-map
//!     entries (the v2 builder folds two entries into one malformed
//!     entry with two colons in its value).
//!   - 9C9N (wrongly-indented flow seq) and N782 (doc markers inside
//!     flow) are deferred: 9C9N needs column tracking; N782 needs
//!     line-start positional context that the validator does not
//!     yet maintain.
//!
//! - **H. Multi-line quoted scalar under-indent** — implemented:
//!   a quoted (double- or single-quoted) `YAML_SCALAR` inside a
//!   `YAML_BLOCK_MAP_VALUE` whose continuation lines are indented
//!   less than the column where the scalar starts. Covers QB6E.
//!
//! - **D. Block indentation anomalies** — implemented:
//!   - 4EJS: a `WHITESPACE` token used as indent (i.e., immediately
//!     after a `NEWLINE` inside a structural node) whose text begins
//!     with a tab character. YAML 1.2 forbids tabs for indentation.
//!   - 4HVU / DMG6 / N4JP: a `YAML_BLOCK_MAP_VALUE` whose direct
//!     children include more than one structural collection node
//!     (multiple `YAML_BLOCK_MAP` / `YAML_BLOCK_SEQUENCE` siblings).
//!     The v2 builder splits a malformed single value into two
//!     sibling collections when an entry is dedented mid-collection.
//!   - ZVH3: a `YAML_BLOCK_SEQUENCE_ITEM` with mixed structural
//!     children (e.g. a `YAML_BLOCK_MAP` followed by a
//!     `YAML_BLOCK_SEQUENCE`) — a sequence item must contain a
//!     single value, not two collections.
//!   - 8XDJ / BF9H: a `YAML_BLOCK_MAP_VALUE` with more than one
//!     `YAML_SCALAR` token child — symptom of a comment splitting a
//!     multi-line plain scalar. YAML forbids comments inside plain
//!     scalars.
//!
//! - **E. Block scalar header anomalies** — implemented (partial):
//!   - S4GJ: a block scalar (text starts with `>` or `|`, optionally
//!     followed by chomping/indent indicators) whose header line has
//!     non-comment content. YAML 1.2 §8.1 requires the header line
//!     to end with end-of-line or a properly-spaced comment.
//!   - X4QW: a block scalar header where `#` appears without a
//!     preceding whitespace separator (e.g. `>#comment`). YAML §6.6
//!     requires whitespace before `#`.
//!   - W9L4 / 5LLU / S98Z (block-scalar body indent contracts) are
//!     deferred — they require modeling the chomping/indent
//!     indicators and content-indent inference, which the validator
//!     does not yet do.
//!
//! - I — pending.
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
    if let Some(diag) = check_flow_context_anomalies(&tree) {
        return Some(diag);
    }
    if let Some(diag) = check_multiline_quoted_indent(&tree, input) {
        return Some(diag);
    }
    if let Some(diag) = check_block_indent_anomalies(&tree) {
        return Some(diag);
    }
    if let Some(diag) = check_block_scalar_header(&tree) {
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

/// Cluster G — flow context anomalies (partial coverage).
///
/// Two malformed shapes are detected:
/// - A `YAML_FLOW_SEQUENCE_ITEM` whose direct children include a
///   `YAML_COLON` AND a newline preceding it (covering DK4H plain-key
///   form `[ key\n  : value ]` and ZXT5 quoted-key form
///   `[ "key"\n  :value ]`). YAML 1.2 forbids an implicit key in flow
///   context from spanning lines.
/// - A `YAML_FLOW_MAP_VALUE` containing a stray `YAML_COLON` token
///   directly (covering T833 `{ foo: 1\n bar: 2 }`). The v2 builder
///   folds two entries into a single malformed entry whose value
///   contains a second colon — that second colon is the symptom of
///   a missing comma between flow-map entries.
fn check_flow_context_anomalies(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for item in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE_ITEM)
    {
        if let Some(diag) = check_flow_seq_item_multiline_key(&item) {
            return Some(diag);
        }
    }
    for value in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_VALUE)
    {
        if let Some(diag) = check_flow_map_value_extra_colon(&value) {
            return Some(diag);
        }
    }
    None
}

/// Detects an implicit key in a `YAML_FLOW_SEQUENCE_ITEM` whose key
/// shape contains a newline before its colon (multi-line implicit key).
///
/// Explicit-key entries (CT4Q's `? foo\n bar : baz` shape) are allowed
/// to span lines and are skipped via the `YAML_KEY` indicator check.
fn check_flow_seq_item_multiline_key(item: &SyntaxNode) -> Option<YamlDiagnostic> {
    let starts_with_explicit_key = item.children_with_tokens().any(|c| {
        c.as_token()
            .is_some_and(|t| t.kind() == SyntaxKind::YAML_KEY)
    });
    if starts_with_explicit_key {
        return None;
    }
    let mut saw_newline_before_colon = false;
    for child in item.children_with_tokens() {
        match &child {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::NEWLINE => saw_newline_before_colon = true,
                SyntaxKind::YAML_SCALAR if t.text().contains('\n') => {
                    saw_newline_before_colon = true;
                }
                SyntaxKind::YAML_COLON => {
                    if saw_newline_before_colon {
                        return Some(diag_at_range(
                            t.text_range().start().into(),
                            t.text_range().end().into(),
                            diagnostic_codes::PARSE_INVALID_KEY_TOKEN,
                            "implicit key in flow context cannot span lines",
                        ));
                    }
                    break;
                }
                _ => {}
            },
            NodeOrToken::Node(_) => {}
        }
    }
    None
}

/// Detects a `YAML_FLOW_MAP_VALUE` whose direct children include a
/// scalar followed by a stray `YAML_COLON` token — the T833 pattern
/// where a missing comma between entries causes the v2 builder to
/// fold two entries into one malformed value.
///
/// A leading colon in the value (`{x: :x}`, `{"key"::value}`) is *not*
/// flagged: the v2 builder tokenizes the leading `:` as `YAML_COLON`
/// even though semantically it is part of the value scalar text. The
/// "scalar before colon" guard distinguishes T833's two-entry fold
/// from this benign tokenization quirk.
fn check_flow_map_value_extra_colon(value: &SyntaxNode) -> Option<YamlDiagnostic> {
    let mut saw_scalar = false;
    for child in value.children_with_tokens() {
        if let NodeOrToken::Token(t) = &child {
            match t.kind() {
                SyntaxKind::YAML_SCALAR => saw_scalar = true,
                SyntaxKind::YAML_COLON if saw_scalar => {
                    return Some(diag_at_range(
                        t.text_range().start().into(),
                        t.text_range().end().into(),
                        diagnostic_codes::PARSE_INVALID_FLOW_SEQUENCE_COMMA,
                        "expected comma between flow-mapping entries",
                    ));
                }
                _ => {}
            }
        }
    }
    None
}

/// Cluster H — multi-line quoted scalar under-indented.
///
/// A quoted (double- or single-quoted) `YAML_SCALAR` whose text spans
/// a newline is a multi-line scalar. YAML 1.2 spec 7.3.1 requires
/// every continuation line to be indented strictly more than the
/// surrounding block context's indent — i.e. the column of the
/// enclosing block-mapping's first key. A continuation line whose
/// first non-whitespace char sits at or before that column is
/// rejected with `PARSE_UNEXPECTED_DEDENT`.
///
/// Note: this is *not* a comparison against the scalar's start column
/// — a continuation indented less than the scalar but still greater
/// than the parent's indent is well-formed.
///
/// Covers fixture QB6E.
fn check_multiline_quoted_indent(tree: &SyntaxNode, input: &str) -> Option<YamlDiagnostic> {
    for value in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
    {
        let Some(entry) = value.parent() else {
            continue;
        };
        let Some(block_map) = entry.parent() else {
            continue;
        };
        if block_map.kind() != SyntaxKind::YAML_BLOCK_MAP {
            continue;
        }
        let block_map_start: usize = block_map.text_range().start().into();
        let parent_indent = column_of(input, block_map_start);
        for child in value.children_with_tokens() {
            let NodeOrToken::Token(t) = child else {
                continue;
            };
            if t.kind() != SyntaxKind::YAML_SCALAR {
                continue;
            }
            let text = t.text();
            if !text.contains('\n') {
                continue;
            }
            let starts_quoted = text.starts_with('"') || text.starts_with('\'');
            if !starts_quoted {
                continue;
            }
            let scalar_start: usize = t.text_range().start().into();
            let mut offset = 0usize;
            let bytes = text.as_bytes();
            while offset < bytes.len() {
                if bytes[offset] != b'\n' {
                    offset += 1;
                    continue;
                }
                let line_start_in_src = scalar_start + offset + 1;
                let line_end_in_text = text[offset + 1..]
                    .find('\n')
                    .map(|i| offset + 1 + i)
                    .unwrap_or(text.len());
                let line_end_in_src = scalar_start + line_end_in_text.min(text.len());
                let line_text_in_src = &input[line_start_in_src..line_end_in_src];
                let leading_ws = line_text_in_src
                    .bytes()
                    .take_while(|b| *b == b' ' || *b == b'\t')
                    .count();
                // Blank continuation lines do not impose indent
                // (line folding consumes them).
                if leading_ws == line_text_in_src.len() {
                    offset += 1;
                    continue;
                }
                let first_non_ws_col = leading_ws;
                let first_non_ws_byte = line_start_in_src + leading_ws;
                if first_non_ws_col <= parent_indent {
                    return Some(diag_at_range(
                        first_non_ws_byte,
                        first_non_ws_byte + 1,
                        diagnostic_codes::PARSE_UNEXPECTED_DEDENT,
                        "multi-line quoted scalar continuation indented at or below parent block indent",
                    ));
                }
                offset += 1;
            }
        }
    }
    None
}

/// Cluster D — block indentation anomalies.
///
/// Three sub-shapes are detected:
/// - Tabs used for indentation (4EJS): a `WHITESPACE` token that
///   follows a `NEWLINE` inside a `YAML_BLOCK_MAP_VALUE` /
///   `YAML_BLOCK_MAP_KEY` / `YAML_BLOCK_SEQUENCE_ITEM` and starts
///   with `\t`.
/// - Sibling collections inside one block-map value or sequence item
///   (4HVU, DMG6, N4JP, ZVH3): a `YAML_BLOCK_MAP_VALUE` (or
///   `YAML_BLOCK_SEQUENCE_ITEM`) whose direct children include more
///   than one of `YAML_BLOCK_MAP` / `YAML_BLOCK_SEQUENCE`, or one of
///   each — symptom of a dedent or over-indent that broke the parent
///   collection.
/// - Multiple `YAML_SCALAR` token children inside a single
///   `YAML_BLOCK_MAP_VALUE` (8XDJ, BF9H): a comment line split a
///   multi-line plain scalar into two pieces.
fn check_block_indent_anomalies(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    if let Some(diag) = check_tab_as_indent(tree) {
        return Some(diag);
    }
    for node in tree.descendants().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_BLOCK_MAP_VALUE | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
        )
    }) {
        let mut struct_count = 0usize;
        let mut scalar_count = 0usize;
        let mut last_struct = None;
        for child in node.children_with_tokens() {
            match &child {
                NodeOrToken::Node(n) => {
                    if matches!(
                        n.kind(),
                        SyntaxKind::YAML_BLOCK_MAP | SyntaxKind::YAML_BLOCK_SEQUENCE
                    ) {
                        struct_count += 1;
                        last_struct = Some(n.clone());
                    }
                }
                NodeOrToken::Token(t) => {
                    if t.kind() == SyntaxKind::YAML_SCALAR {
                        scalar_count += 1;
                    }
                }
            }
        }
        if struct_count > 1 {
            let n = last_struct.expect("struct_count > 1 implies last_struct set");
            return Some(diag_at_range(
                n.text_range().start().into(),
                n.text_range().end().into(),
                diagnostic_codes::PARSE_UNEXPECTED_DEDENT,
                "block collection has mismatched indentation, splitting it into siblings",
            ));
        }
        if node.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE && scalar_count > 1 {
            let scalars: Vec<_> = node
                .children_with_tokens()
                .filter_map(|c| c.into_token())
                .filter(|t| t.kind() == SyntaxKind::YAML_SCALAR)
                .collect();
            let last_scalar = scalars
                .last()
                .expect("scalar_count > 1 implies at least one scalar child");
            return Some(diag_at_range(
                last_scalar.text_range().start().into(),
                last_scalar.text_range().end().into(),
                diagnostic_codes::PARSE_UNEXPECTED_DEDENT,
                "comment cannot appear inside a multi-line plain scalar",
            ));
        }
    }
    None
}

/// Detects a `WHITESPACE` token that begins with a tab, used as
/// indent (i.e. immediately preceded by a `NEWLINE` token within a
/// structural block-context node).
fn check_tab_as_indent(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for node in tree.descendants().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_BLOCK_MAP_VALUE
                | SyntaxKind::YAML_BLOCK_MAP_KEY
                | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
                | SyntaxKind::YAML_BLOCK_MAP
                | SyntaxKind::YAML_BLOCK_SEQUENCE
        )
    }) {
        let mut prev_was_newline = false;
        for child in node.children_with_tokens() {
            if let NodeOrToken::Token(t) = &child {
                match t.kind() {
                    SyntaxKind::NEWLINE => prev_was_newline = true,
                    SyntaxKind::WHITESPACE if prev_was_newline => {
                        if t.text().starts_with('\t') {
                            return Some(diag_at_range(
                                t.text_range().start().into(),
                                t.text_range().end().into(),
                                diagnostic_codes::PARSE_UNEXPECTED_INDENT,
                                "tab character used as indentation is not allowed in YAML",
                            ));
                        }
                        prev_was_newline = false;
                    }
                    _ => {
                        prev_was_newline = false;
                    }
                }
            } else {
                prev_was_newline = false;
            }
        }
    }
    None
}

/// Cluster E — block scalar header anomalies (partial coverage).
///
/// Inspects every `YAML_SCALAR` token whose text begins with `>` or
/// `|` (folded / literal block scalar). After the indicator and any
/// chomping (`+` / `-`) or explicit-indent (digit) characters, the
/// header line must end at end-of-line or with a properly-spaced
/// comment. Two malformed shapes:
/// - S4GJ: non-comment content on the header line (e.g. `> first line`).
/// - X4QW: `#` immediately after the indicator with no whitespace
///   separator (e.g. `>#comment`).
fn check_block_scalar_header(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for token in tree
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::YAML_SCALAR)
    {
        let text = token.text();
        if !text.starts_with('>') && !text.starts_with('|') {
            continue;
        }
        let header_end = text.find('\n').unwrap_or(text.len());
        let header = &text[..header_end];
        let bytes = header.as_bytes();
        // Skip indicator + chomping/indent characters.
        let mut i = 1usize;
        while i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-' || bytes[i].is_ascii_digit())
        {
            i += 1;
        }
        let rest = &header[i..];
        if rest.is_empty() {
            continue;
        }
        // X4QW: `#` immediately after the indicator (no whitespace).
        if rest.starts_with('#') {
            let scalar_start: usize = token.text_range().start().into();
            return Some(diag_at_range(
                scalar_start + i,
                scalar_start + i + 1,
                diagnostic_codes::PARSE_INVALID_KEY_TOKEN,
                "comment after block scalar indicator must be preceded by whitespace",
            ));
        }
        let leading_ws = rest
            .bytes()
            .take_while(|b| *b == b' ' || *b == b'\t')
            .count();
        let after_ws = &rest[leading_ws..];
        if after_ws.is_empty() || after_ws.starts_with('#') {
            // Blank-only or properly-spaced comment — header is fine.
            continue;
        }
        // S4GJ: non-whitespace, non-comment content on the header line.
        let scalar_start: usize = token.text_range().start().into();
        let content_start = scalar_start + i + leading_ws;
        let content_end = scalar_start + header_end;
        return Some(diag_at_range(
            content_start,
            content_end,
            diagnostic_codes::PARSE_INVALID_KEY_TOKEN,
            "block scalar header line must end at EOL or with a comment",
        ));
    }
    None
}

/// Compute the byte-based column (zero-indexed) of `byte_offset`
/// relative to the previous newline in `input`. Tabs are not
/// width-expanded; this is byte-distance, sufficient for indent
/// comparisons in space-indented YAML.
fn column_of(input: &str, byte_offset: usize) -> usize {
    match input[..byte_offset].rfind('\n') {
        Some(nl) => byte_offset - nl - 1,
        None => byte_offset,
    }
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

    // ---- Cluster G: flow context anomalies ----

    #[test]
    fn flow_seq_implicit_key_spans_lines_dk4h() {
        // DK4H: `[ key\n  : value ]` — plain-key implicit entry where
        // the key spans a newline before its colon.
        let input = "---\n[ key\n  : value ]\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_INVALID_KEY_TOKEN);
    }

    #[test]
    fn flow_seq_implicit_key_quoted_spans_lines_zxt5() {
        // ZXT5: `[ "key"\n  :value ]` — quoted-key form of DK4H.
        let input = "[ \"key\"\n  :value ]\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_INVALID_KEY_TOKEN);
    }

    #[test]
    fn flow_map_missing_comma_t833() {
        // T833: `{\n foo: 1\n bar: 2 }` — missing comma between
        // entries; v2 builder folds them into one malformed entry
        // with two colons in its value.
        let input = "---\n{\n foo: 1\n bar: 2 }\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_INVALID_FLOW_SEQUENCE_COMMA
        );
    }

    #[test]
    fn flow_seq_single_line_implicit_key_passes() {
        // Sanity: `[ key: value ]` — single-line implicit key is fine.
        let input = "---\n[ key: value ]\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn flow_map_well_formed_multiline_passes() {
        // `{ foo: 1, bar: 2 }` split across lines with proper commas
        // is well-formed.
        let input = "---\n{\n foo: 1,\n bar: 2\n}\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn flow_map_value_starting_with_colon_passes_58mp() {
        // 58MP: `{x: :x}` — value is the scalar `:x`. v2 tokenizes the
        // leading `:` as YAML_COLON, but no scalar precedes it inside
        // the value, so it must not be confused with T833.
        let input = "{x: :x}\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn flow_map_value_starting_with_double_colon_passes_5t43() {
        // 5T43: `{ "key"::value }` — value is the scalar `:value`.
        // Same shape as 58MP at the value level (leading colon, no
        // preceding scalar in the value).
        let input = "- { \"key\":value }\n- { \"key\"::value }\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn flow_seq_explicit_key_spans_lines_passes_ct4q() {
        // CT4Q: `[ ? foo\n bar : baz ]` — explicit-key indicator (`?`)
        // permits the key to span lines. The check must skip items
        // that begin with YAML_KEY.
        let input = "[\n? foo\n bar : baz\n]\n";
        assert!(run(input).is_none());
    }

    // ---- Cluster H: multi-line quoted scalar under-indent ----

    #[test]
    fn multiline_quoted_under_indent_qb6e() {
        // QB6E: `quoted: "a\nb\nc"` — continuation lines `b` and `c`
        // sit at column 0, less than the scalar's start column 8.
        let input = "---\nquoted: \"a\nb\nc\"\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_DEDENT);
    }

    #[test]
    fn multiline_quoted_properly_indented_passes() {
        // Sanity: continuation lines at column >= scalar-start col.
        let input = "---\nquoted: \"a\n  b\n  c\"\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn singleline_quoted_passes() {
        // No newline in scalar text — no continuation rule applies.
        let input = "---\nquoted: \"a b c\"\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn multiline_single_quoted_under_indent_rejects() {
        // Same shape as QB6E with single quotes.
        let input = "---\nquoted: 'a\nb\nc'\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_DEDENT);
    }

    // ---- Cluster D: block indentation anomalies ----

    #[test]
    fn tab_as_indent_4ejs() {
        // 4EJS: tabs used for indentation are not allowed in YAML.
        let input = "---\na:\n\tb:\n\t\tc: value\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_INDENT);
    }

    #[test]
    fn map_under_indent_dmg6() {
        // DMG6: `key:\n  ok: 1\n wrong: 2` — `wrong` dedented to col 1.
        let input = "key:\n  ok: 1\n wrong: 2\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_DEDENT);
    }

    #[test]
    fn map_under_indent_quoted_n4jp() {
        // N4JP: same as DMG6 but with quoted values.
        let input = "map:\n  key1: \"quoted1\"\n key2: \"bad indentation\"\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_DEDENT);
    }

    #[test]
    fn seq_under_indent_4hvu() {
        // 4HVU: sequence items at col 3, then a `wrong` item at col 2.
        let input = "key:\n   - ok\n   - also ok\n  - wrong\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_DEDENT);
    }

    #[test]
    fn seq_item_with_extra_subseq_zvh3() {
        // ZVH3: `- key: value\n - item1` — over-indented `- item1`
        // appears as a sibling sub-sequence inside the first item.
        let input = "- key: value\n - item1\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_DEDENT);
    }

    #[test]
    fn comment_in_multiline_plain_8xdj() {
        // 8XDJ: comment line splitting a multi-line plain scalar.
        let input = "key: word1\n#  xxx\n  word2\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_DEDENT);
    }

    #[test]
    fn trailing_comment_in_multiline_plain_bf9h() {
        // BF9H: trailing comment on a continuation line splits the scalar.
        let input = "---\nplain: a\n       b # end of scalar\n       c\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_DEDENT);
    }

    #[test]
    fn block_map_with_well_formed_entries_passes() {
        let input = "key:\n  a: 1\n  b: 2\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn block_seq_with_well_formed_items_passes() {
        let input = "key:\n  - a\n  - b\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn nested_block_seq_in_seq_item_passes() {
        // `- - x` (nested sequence in single item) is well-formed.
        let input = "- - x\n  - y\n- z\n";
        assert!(run(input).is_none());
    }

    // ---- Cluster E: block scalar header anomalies ----

    #[test]
    fn block_scalar_header_content_s4gj() {
        // S4GJ: `folded: > first line` — text after `>` is not a comment.
        let input = "---\nfolded: > first line\n  second line\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_INVALID_KEY_TOKEN);
    }

    #[test]
    fn block_scalar_header_unspaced_comment_x4qw() {
        // X4QW: `block: ># comment` — `#` immediately after `>`.
        let input = "block: ># comment\n  scalar\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_INVALID_KEY_TOKEN);
    }

    #[test]
    fn block_scalar_with_strip_chomp_and_body_passes() {
        // `text: |-\n  body` — `-` after `|` is a chomp indicator.
        let input = "text: |-\n  body\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn block_scalar_with_indent_indicator_passes() {
        // `text: |2\n  body` — `2` is an explicit indent indicator.
        let input = "text: |2\n  body\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn block_scalar_with_spaced_comment_passes() {
        // `text: > # ok\n  body` — comment with whitespace after `>` is fine.
        let input = "text: > # ok\n  body\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn block_scalar_bare_header_passes() {
        // `text: >\n  body` — no content on header line.
        let input = "text: >\n  body\n";
        assert!(run(input).is_none());
    }
}
