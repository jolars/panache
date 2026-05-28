//! v2-aware diagnostic validator.
//!
//! Phase-2 cutover: detection runs over the streaming scanner's token
//! output and the v2 CST. Each cluster of error-contract patterns
//! lands as its own checker function. The public entry
//! [`validate_yaml`] composes them in priority order and is wired into
//! [`super::parser::parse_yaml_report`] as the structural-validation
//! source. The v1 lexer is still called to surface lex-level
//! diagnostics (e.g. `LEX_INVALID_DOUBLE_QUOTED_ESCAPE`) and to handle
//! directive-ordering, because the v2 scanner does not yet recognize
//! `%`-prefixed lines after content (it folds them into a Plain
//! scalar). Closing those gaps is scanner-side follow-up work; once
//! complete, the v1 lexer body can be deleted.
//!
//! Cases that the v1 sniff used to catch but the validator cannot yet
//! cover without scanner enhancements are listed in `blocked.txt` and
//! are intentionally absent from `allowlist.txt`.
//!
//! Coverage status:
//! - **F. Directives** — implemented: directive after content,
//!   directive without `---` marker, duplicate `%YAML`, and malformed
//!   `%YAML` argument list. Covers EB22, RHX7, 9MMA, B63P, SF5V,
//!   H7TQ, 9HCY (`!foo "bar"\n%TAG ...` — tag dispatch in the
//!   scanner now keeps `%TAG` out of the preceding scalar so the
//!   directive lands in its real position). M7A3 / W4TN no longer
//!   trip false positives now that the scanner consumes block-scalar
//!   bodies past their header.
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
//! - **J. Doc-level bare-scalar-then-colon block map** —
//!   implemented: a `YAML_SCALAR` direct child of `YAML_DOCUMENT`
//!   immediately followed by a `YAML_BLOCK_MAP` whose first entry's
//!   key is colon-only (no scalar token before the `:`). The
//!   diagnostic differs by whether a `YAML_DOCUMENT_START` precedes
//!   the bare scalar:
//!   - With `---` on the same line: `LEX_TRAILING_CONTENT_AFTER_DOCUMENT_START`
//!     (matches yaml-test-suite cases 9KBC, CXX2 and the
//!     trailing-key-on-marker-line shape `--- key: value`).
//!   - Without a marker: `PARSE_INVALID_KEY_TOKEN` (a bare scalar at
//!     stream/document start is not a key — the trailing colon
//!     belongs to a separate, malformed entry).
//!
//! - **K. Flow continuation under-indent (9C9N)** — implemented: a
//!   `YAML_FLOW_SEQUENCE` or `YAML_FLOW_MAP` whose nearest enclosing
//!   `YAML_BLOCK_MAP_VALUE` exists must have all continuation lines
//!   indented strictly past the parent `YAML_BLOCK_MAP`'s column. A
//!   continuation at or below the block-map's column violates YAML
//!   1.2 §7.1's flow-in-block indentation contract.
//!
//! - **L. Invalid double-quoted escape** — implemented: walks every
//!   `YAML_SCALAR` whose text starts with `"` and emits
//!   `LEX_INVALID_DOUBLE_QUOTED_ESCAPE` for the first `\` followed by
//!   a character not in YAML 1.2 §5.7's escape table. Mirrors the v1
//!   lexer's `invalid_double_quote_escape_offset` contract.
//!
//! - **M. Anchor decorates alias** — implemented: walks every
//!   `YAML_BLOCK_MAP_KEY` / `YAML_BLOCK_MAP_VALUE` /
//!   `YAML_BLOCK_SEQUENCE_ITEM` (and flow analogues) and emits
//!   `PARSE_ANCHOR_DECORATES_ALIAS` when a `YAML_ANCHOR` token is
//!   immediately followed — modulo trivia — by a `YAML_ALIAS` token.
//!   YAML 1.2 §6.9.2: an alias is a complete node and cannot carry
//!   node properties. Covers SR86 (`key2: &b *a`) and SU74
//!   (`&b *alias : value2`).
//!
//! Cluster I (LHL4 — invalid tag syntax) is deferred: the scanner
//! accepts `!invalid{}tag` as a single tag token because its tag-name
//! class is relaxed (libyaml-style), so the validator would need to
//! reject the malformed characters separately. Cluster N (U99R —
//! invalid comma inside a `!!str,` tag suffix outside flow context)
//! has the same root cause and is deferred until the scanner's tag
//! name class tightens.
//!
//! See `.claude/skills/yaml-shadow-expand/scanner-rewrite.md` for the
//! cutover plan and per-cluster detection scope.
#![allow(dead_code)]

use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
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
    if let Some(diag) = check_directives(input, &tokens) {
        return Some(diag);
    }
    if let Some(diag) = check_unterminated_quoted(input) {
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
    if let Some(diag) = check_block_scalar_leading_indent(&tree) {
        return Some(diag);
    }
    if let Some(diag) = check_doc_level_bare_scalar_then_colon_map(&tree) {
        return Some(diag);
    }
    if let Some(diag) = check_block_collection_after_value_scalar(&tree) {
        return Some(diag);
    }
    if let Some(diag) = check_flow_continuation_indent(&tree, input) {
        return Some(diag);
    }
    if let Some(diag) = check_flow_doc_markers(&tree, input) {
        return Some(diag);
    }
    if let Some(diag) = check_invalid_dq_escapes(&tree) {
        return Some(diag);
    }
    if let Some(diag) = check_comment_not_preceded_by_space(&tree, input) {
        return Some(diag);
    }
    if let Some(diag) = check_anchor_decorates_alias(&tree) {
        return Some(diag);
    }
    if let Some(diag) = check_anchor_before_block_indicator(&tree) {
        return Some(diag);
    }
    if let Some(diag) = check_anchor_without_target(&tree) {
        return Some(diag);
    }
    if let Some(diag) = check_invalid_tag_chars(&tree) {
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

/// Lex-level cluster — unterminated quoted scalar.
///
/// The streaming scanner already detects a `"`/`'` scalar that never
/// reaches its closing quote: both at EOF (`key: "missing close`) and
/// when a `---`/`...` document marker at column 0 aborts the still-open
/// scalar (`---\n"\n---\n"`, `---\n'\n...\n'`). It records the failure
/// on its diagnostic channel, but `validate_yaml` otherwise consumes
/// only the token stream, so this lex diagnostic was dropped on the
/// floor. Surface it here so the document is rejected rather than
/// parsed with a silently-truncated scalar.
///
/// Covers fixtures CQ3W (EOF) and 5TRB / RXY3 (document-marker abort).
fn check_unterminated_quoted(input: &str) -> Option<YamlDiagnostic> {
    let mut scanner = Scanner::new(input);
    while scanner.next_token().is_some() {}
    scanner
        .diagnostics()
        .iter()
        .find(|d| d.code == diagnostic_codes::LEX_UNTERMINATED_QUOTED_SCALAR)
        .cloned()
}

/// Cluster F — directive ordering and lone-directive checks.
///
/// Surfaces four failures, all driven off scanner-emitted `Directive`
/// tokens:
/// - `PARSE_DIRECTIVE_AFTER_CONTENT` when a directive appears after
///   non-trivia, non-`...` content. YAML 1.2 requires a `...`
///   document end before subsequent directives.
/// - `PARSE_DIRECTIVE_WITHOUT_DOCUMENT_START` when any directive is
///   present but no `---` marker exists in the stream. A directive
///   without `---` has no document to attach to.
/// - `PARSE_DUPLICATE_YAML_DIRECTIVE` when two `%YAML` directives
///   precede the same document (§6.8.1 — at most one per document).
/// - `PARSE_MALFORMED_YAML_DIRECTIVE` when a `%YAML` directive carries
///   anything beyond its single version argument (a trailing comment
///   is still allowed), e.g. `%YAML 1.2 foo`.
///
/// The streaming scanner emits `Directive` only when a `%`-prefixed
/// line is in a directive position (stream start, or after `...`).
/// Lines that look like directives but are scalar continuations,
/// block-scalar bodies, or flow-context content are correctly *not*
/// emitted as directives — so this check inherits the scanner's
/// spec-correct view.
///
/// Covers fixtures EB22, RHX7, 9MMA, B63P, SF5V, H7TQ.
fn check_directives(input: &str, tokens: &[Token]) -> Option<YamlDiagnostic> {
    let mut seen_content = false;
    let mut yaml_directive_in_scope = false;
    for tok in tokens {
        match tok.kind {
            TokenKind::Directive if seen_content => {
                return Some(diag_at_token(
                    tok,
                    diagnostic_codes::PARSE_DIRECTIVE_AFTER_CONTENT,
                    "directive requires document end before subsequent directives",
                ));
            }
            TokenKind::Directive => {
                let text = &input[tok.start.index..tok.end.index];
                if directive_name(text) == "YAML" {
                    if yaml_directive_in_scope {
                        return Some(diag_at_token(
                            tok,
                            diagnostic_codes::PARSE_DUPLICATE_YAML_DIRECTIVE,
                            "a document may carry at most one %YAML directive",
                        ));
                    }
                    yaml_directive_in_scope = true;
                    if yaml_directive_has_trailing_content(text) {
                        return Some(diag_at_token(
                            tok,
                            diagnostic_codes::PARSE_MALFORMED_YAML_DIRECTIVE,
                            "%YAML directive takes a single version argument",
                        ));
                    }
                }
            }
            TokenKind::Trivia(_) | TokenKind::StreamStart | TokenKind::StreamEnd => {}
            TokenKind::DocumentStart => {
                seen_content = true;
                yaml_directive_in_scope = false;
            }
            TokenKind::DocumentEnd => {
                seen_content = false;
                yaml_directive_in_scope = false;
            }
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

/// The directive name — the run of non-whitespace characters following
/// the leading `%`. `%YAML 1.2` → `"YAML"`, `%TAG ! …` → `"TAG"`.
fn directive_name(text: &str) -> &str {
    text.strip_prefix('%')
        .unwrap_or(text)
        .split_whitespace()
        .next()
        .unwrap_or("")
}

/// True when a `%YAML` directive carries content beyond its single
/// version argument. A trailing comment (`# …`) is permitted; any other
/// token is invalid (spec §6.8.1), e.g. the `foo` in `%YAML 1.2 foo`.
fn yaml_directive_has_trailing_content(text: &str) -> bool {
    let mut fields = text.strip_prefix('%').unwrap_or(text).split_whitespace();
    let _name = fields.next();
    let _version = fields.next();
    matches!(fields.next(), Some(field) if !field.starts_with('#'))
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
    for container in tree.descendants().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_BLOCK_MAP_VALUE | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
        )
    }) {
        if let Some(diag) = check_trailing_after_flow_in_container(&container) {
            return Some(diag);
        }
    }
    if let Some(diag) = check_trailing_after_doc_end(tree) {
        return Some(diag);
    }
    None
}

/// 62EZ / P2EQ — a closed flow map/sequence inside a block-map value
/// (or block-sequence item) followed by non-trivia content. The
/// closing `}` / `]` ends the flow node; any subsequent scalar /
/// collection on the same logical line is unspaced trailing content.
fn check_trailing_after_flow_in_container(container: &SyntaxNode) -> Option<YamlDiagnostic> {
    let mut after_flow = false;
    let mut have_separator = false;
    for child in container.children_with_tokens() {
        match &child {
            NodeOrToken::Node(n) => {
                let kind = n.kind();
                if matches!(
                    kind,
                    SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
                ) {
                    after_flow = true;
                    have_separator = false;
                } else if after_flow {
                    return Some(diag_at_range(
                        n.text_range().start().into(),
                        n.text_range().end().into(),
                        diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END,
                        "unexpected content after flow-collection close in block context",
                    ));
                }
            }
            NodeOrToken::Token(t) => {
                if !after_flow {
                    continue;
                }
                match t.kind() {
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => have_separator = true,
                    SyntaxKind::YAML_COMMENT => {
                        if !have_separator {
                            return Some(diag_at_range(
                                t.text_range().start().into(),
                                t.text_range().end().into(),
                                diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END,
                                "comment must be preceded by whitespace after flow-collection close",
                            ));
                        }
                    }
                    SyntaxKind::YAML_SCALAR => {
                        return Some(diag_at_range(
                            t.text_range().start().into(),
                            t.text_range().end().into(),
                            diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END,
                            "unexpected content after flow-collection close in block context",
                        ));
                    }
                    _ => {}
                }
            }
        }
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
                        // the block-map sibling jointly form the entry,
                        // BUT YAML 1.2 §7.4 requires implicit keys to
                        // fit on a single line. A flow node spanning a
                        // newline cannot serve as an implicit key
                        // (C2SP), so the bytes after the close are
                        // trailing content.
                        let flow_nodes: Vec<SyntaxNode> = doc
                            .children()
                            .filter(|c| {
                                matches!(
                                    c.kind(),
                                    SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
                                )
                            })
                            .collect();
                        let preceding_flow_spans_lines = flow_nodes
                            .last()
                            .map(|f| f.text().to_string().contains('\n'))
                            .unwrap_or(false);
                        if preceding_flow_spans_lines {
                            return Some(diag_at_range(
                                n.text_range().start().into(),
                                n.text_range().end().into(),
                                diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END,
                                "implicit key flow node cannot span lines",
                            ));
                        }
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
/// Three malformed shapes are detected:
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
/// - A flow item/key/value whose content is a lone `-` scalar (covering
///   YJV2 `[-]` and G5U8 `- [-, -]`); a `-` abutting a flow indicator is
///   a bare indicator, not a valid `ns-plain-first` scalar start.
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
    if let Some(diag) = check_flow_lone_dash(tree) {
        return Some(diag);
    }
    None
}

/// Detects a lone `-` plain scalar inside a flow collection.
///
/// In flow context a plain scalar may only begin with `-` when the next
/// character is a non-space, non-flow-indicator char (YAML 1.2
/// `ns-plain-first` over `ns-plain-safe-in`). When the `-` is followed by
/// a flow indicator (`,`, `]`, `}`) or the collection close, it is a bare
/// indicator rather than a scalar and the document is malformed. The
/// scanner still tokenizes the dash as a `YAML_SCALAR`, so the surviving
/// signal is a flow item/key/value whose only significant content is a
/// scalar whose text is exactly `-`.
///
/// Covers fixtures G5U8 (`- [-, -]`) and YJV2 (`[-]`). Scoped to `-`
/// specifically; `?`/`:` are tokenized distinctly and handled elsewhere.
fn check_flow_lone_dash(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for holder in tree.descendants().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_FLOW_SEQUENCE_ITEM
                | SyntaxKind::YAML_FLOW_MAP_KEY
                | SyntaxKind::YAML_FLOW_MAP_VALUE
        )
    }) {
        let lone_dash = holder.children_with_tokens().find_map(|c| match c {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::YAML_SCALAR && t.text() == "-" => {
                Some(t)
            }
            _ => None,
        });
        if let Some(dash) = lone_dash {
            return Some(diag_at_range(
                dash.text_range().start().into(),
                dash.text_range().end().into(),
                diagnostic_codes::PARSE_INVALID_PLAIN_SCALAR_IN_FLOW,
                "`-` cannot start a plain scalar in flow context",
            ));
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
/// Covers fixture QB6E (block-map value) and JKF3 (nested block-seq
/// item where the continuation drops to column 0).
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
        if let Some(diag) = check_quoted_scalar_continuation(&value, input, parent_indent) {
            return Some(diag);
        }
    }
    for item in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
    {
        let Some(block_seq) = item.parent() else {
            continue;
        };
        if block_seq.kind() != SyntaxKind::YAML_BLOCK_SEQUENCE {
            continue;
        }
        let block_seq_start: usize = block_seq.text_range().start().into();
        let parent_indent = column_of(input, block_seq_start);
        if let Some(diag) = check_quoted_scalar_continuation(&item, input, parent_indent) {
            return Some(diag);
        }
    }
    None
}

/// Scan direct `YAML_SCALAR` children of `container` and require any
/// multi-line quoted scalar's continuation lines to indent strictly
/// past `parent_indent`. Shared by the block-map-value and
/// block-sequence-item variants of `check_multiline_quoted_indent`.
fn check_quoted_scalar_continuation(
    container: &SyntaxNode,
    input: &str,
    parent_indent: usize,
) -> Option<YamlDiagnostic> {
    for child in container.children_with_tokens() {
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
        let bytes = text.as_bytes();
        let mut offset = 0usize;
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
            // §6.1 — a tab in the *required-indentation slot* of a
            // multi-line quoted scalar continuation is illegal (DK95/01).
            // Spaces past `parent_indent` provide the required indentation;
            // any tab after enough leading spaces is content (4ZYM), not
            // indentation, and must not be flagged.
            let leading_spaces = line_text_in_src.bytes().take_while(|b| *b == b' ').count();
            if leading_spaces <= parent_indent
                && line_text_in_src.as_bytes().get(leading_spaces) == Some(&b'\t')
            {
                let tab_byte = line_start_in_src + leading_spaces;
                return Some(diag_at_range(
                    tab_byte,
                    tab_byte + 1,
                    diagnostic_codes::PARSE_UNEXPECTED_INDENT,
                    "tab character used as indentation is not allowed in YAML",
                ));
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
///   `YAML_BLOCK_MAP_VALUE` (8XDJ, BF9H) or directly under a
///   `YAML_DOCUMENT` (BS4K): a comment line split a multi-line plain
///   scalar into two pieces. Top-level scalars carry the same
///   "comment-inside-plain-scalar" failure mode as value-level ones;
///   the only difference is the absence of an enclosing block-map.
fn check_block_indent_anomalies(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    if let Some(diag) = check_tab_as_indent(tree) {
        return Some(diag);
    }
    if let Some(diag) = check_inline_block_seq_in_value(tree) {
        return Some(diag);
    }
    for node in tree.descendants().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_BLOCK_MAP_VALUE
                | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
                | SyntaxKind::YAML_DOCUMENT
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
        // The struct_count > 1 and scalar-after-structural checks below
        // are about block-collection siblings being split by indent
        // anomalies — they do not apply to YAML_DOCUMENT, which can
        // legitimately hold multiple block collections under a
        // doc-start marker before a doc-end one in error-recovery
        // shapes. The scalar_count > 1 check that follows applies
        // uniformly to all included node kinds.
        let is_doc = node.kind() == SyntaxKind::YAML_DOCUMENT;
        if !is_doc && struct_count > 1 {
            let n = last_struct.expect("struct_count > 1 implies last_struct set");
            return Some(diag_at_range(
                n.text_range().start().into(),
                n.text_range().end().into(),
                diagnostic_codes::PARSE_UNEXPECTED_DEDENT,
                "block collection has mismatched indentation, splitting it into siblings",
            ));
        }
        if struct_count >= 1
            && scalar_count >= 1
            && node.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE
            && let Some(trailing_scalar) = scalar_after_structural_in_block_map_value(&node)
        {
            // A scalar AFTER a structural collection inside a block-map
            // value — e.g. `key:\n - item1\n - item2\ninvalid\n`
            // (9CWY) where a stray top-level scalar is absorbed into
            // the value alongside a block sequence. Compact mapping
            // shapes (`a: <scalar>: <value>`, W5VH/26DV) put the
            // scalar BEFORE the inner map and remain valid.
            return Some(diag_at_range(
                trailing_scalar.text_range().start().into(),
                trailing_scalar.text_range().end().into(),
                diagnostic_codes::PARSE_INVALID_KEY_TOKEN,
                "stray scalar after a block collection in a block-map value",
            ));
        }
        if scalar_count > 1 {
            // At YAML_DOCUMENT scope, the v2 scanner currently folds
            // `%YAML` / `%TAG` directives into plain scalars, so a bare
            // "two scalars at doc root" shape isn't reliable evidence of
            // the comment-splits-scalar bug — it commonly fires on
            // well-formed directive-prefixed documents instead. Require
            // a YAML_COMMENT token between two scalars to fire, which is
            // BS4K's actual failure mode. The existing BLOCK_MAP_VALUE /
            // BLOCK_SEQUENCE_ITEM scopes keep the simpler scalar-count
            // contract because those parents cannot legitimately hold
            // two sibling scalars without a comment under any shape.
            if is_doc && !has_comment_between_scalars(&node) {
                continue;
            }
            let scalars: Vec<_> = node
                .children_with_tokens()
                .filter_map(|c| c.into_token())
                .filter(|t| t.kind() == SyntaxKind::YAML_SCALAR)
                .collect();
            let last_scalar = scalars
                .last()
                .expect("scalar_count > 1 implies at least one scalar child");
            let (code, message) = match node.kind() {
                SyntaxKind::YAML_BLOCK_MAP_VALUE | SyntaxKind::YAML_DOCUMENT => (
                    diagnostic_codes::PARSE_UNEXPECTED_DEDENT,
                    "comment cannot appear inside a multi-line plain scalar",
                ),
                _ => (
                    diagnostic_codes::PARSE_INVALID_KEY_TOKEN,
                    "stray content following a block sequence item at its indent level",
                ),
            };
            return Some(diag_at_range(
                last_scalar.text_range().start().into(),
                last_scalar.text_range().end().into(),
                code,
                message,
            ));
        }
    }
    None
}

/// True when at least one `YAML_COMMENT` token appears between two
/// `YAML_SCALAR` token children of `node` (with only trivia in
/// between). Distinguishes BS4K's "comment splits a multi-line plain
/// scalar" shape from a multi-scalar artifact caused by the v2
/// scanner folding `%YAML` / `%TAG` directives as plain scalars
/// (5TYM, well-formed directive-prefixed documents).
///
/// A `YAML_DOCUMENT_START` between the two scalars resets the state:
/// the next scalar is content of a fresh document section, not a
/// continuation of the previous one. A leading `%` on a scalar marks
/// a folded directive and is also ignored.
fn has_comment_between_scalars(node: &SyntaxNode) -> bool {
    let mut saw_scalar = false;
    let mut saw_comment_since_scalar = false;
    for child in node.children_with_tokens() {
        let NodeOrToken::Token(t) = &child else {
            continue;
        };
        match t.kind() {
            SyntaxKind::YAML_SCALAR => {
                if t.text().starts_with('%') {
                    continue;
                }
                if saw_scalar && saw_comment_since_scalar {
                    return true;
                }
                saw_scalar = true;
                saw_comment_since_scalar = false;
            }
            SyntaxKind::YAML_COMMENT => {
                if saw_scalar {
                    saw_comment_since_scalar = true;
                }
            }
            SyntaxKind::YAML_DOCUMENT_START | SyntaxKind::YAML_DOCUMENT_END => {
                saw_scalar = false;
                saw_comment_since_scalar = false;
            }
            _ => {}
        }
    }
    false
}

/// Returns the first `YAML_SCALAR` token child of `block_map_value`
/// that appears AFTER any structural collection node child
/// (`YAML_BLOCK_MAP` / `YAML_BLOCK_SEQUENCE`). Returns `None` if no
/// scalar follows a collection — preserves the compact-mapping shape
/// `a: <scalar>: <value>` where the scalar precedes the inner map.
fn scalar_after_structural_in_block_map_value(value: &SyntaxNode) -> Option<SyntaxToken> {
    let mut saw_struct = false;
    for child in value.children_with_tokens() {
        match &child {
            NodeOrToken::Node(n) => {
                if matches!(
                    n.kind(),
                    SyntaxKind::YAML_BLOCK_MAP | SyntaxKind::YAML_BLOCK_SEQUENCE
                ) {
                    saw_struct = true;
                }
            }
            NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::YAML_SCALAR && saw_struct {
                    return Some(t.clone());
                }
            }
        }
    }
    None
}

/// Detects an inline block-sequence start on the same line as the
/// owning block-map key (5U3A): `key: - a\n     - b\n`. YAML 1.2
/// requires a block sequence to start on its own line at a column
/// indented past the key. The v2 builder accepts the shape and emits
/// a `YAML_BLOCK_SEQUENCE` directly inside `YAML_BLOCK_MAP_VALUE`
/// without an intervening `NEWLINE`. Flag at the start of the second
/// `YAML_BLOCK_SEQUENCE_ITEM` (the dash that turned the inline shape
/// into a multi-line one), matching the v1 contract.
///
/// Exempts explicit-key entries (`? key` / `: - a`): the YAML 1.2
/// grammar's `ns-l-compact-sequence` permits a block sequence to begin
/// on the explicit value-indicator line (5WE3, A2M4, KK5P). The
/// prohibition is specific to implicit keys (`key: - a`), whose value
/// production (`l-block-map-implicit-value`) has no compact form.
fn check_inline_block_seq_in_value(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for value in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
    {
        if block_map_entry_key_is_explicit(&value) {
            continue;
        }
        let mut seen_newline = false;
        for child in value.children_with_tokens() {
            match &child {
                NodeOrToken::Token(t) => {
                    if t.kind() == SyntaxKind::NEWLINE {
                        seen_newline = true;
                    }
                }
                NodeOrToken::Node(n) => {
                    if n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE && !seen_newline {
                        let second_item = n
                            .children()
                            .filter(|c| c.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
                            .nth(1)
                            .unwrap_or_else(|| n.clone());
                        return Some(diag_at_range(
                            second_item.text_range().start().into(),
                            (Into::<usize>::into(second_item.text_range().start())) + 1,
                            diagnostic_codes::PARSE_INVALID_KEY_TOKEN,
                            "block sequence cannot start on the same line as its key",
                        ));
                    }
                    // Other inline content resets — but a second
                    // collection inside one value is detected by the
                    // sibling-collection check, not here.
                }
            }
        }
    }
    None
}

/// True when the `YAML_BLOCK_MAP_VALUE`'s owning entry uses an
/// explicit key indicator (`?`) — i.e. its sibling
/// `YAML_BLOCK_MAP_KEY` contains a `YAML_KEY` token. Explicit-key
/// entries permit a compact block sequence on the value-indicator
/// line, so the inline-block-sequence prohibition does not apply.
fn block_map_entry_key_is_explicit(value: &SyntaxNode) -> bool {
    value
        .parent()
        .into_iter()
        .flat_map(|entry| entry.children())
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
        .any(|key| {
            key.children_with_tokens()
                .any(|c| matches!(&c, NodeOrToken::Token(t) if t.kind() == SyntaxKind::YAML_KEY))
        })
}

/// Tab characters are never legal indentation (§6.1 — "indentation is
/// restricted to space characters"). Three shapes share that root cause:
///
/// - **NEWLINE → tab-bearing WHITESPACE → content** in any block or flow
///   container. A tab-only line that is followed immediately by another
///   NEWLINE (or by EOF) is whitespace-only and isn't load-bearing
///   indentation, so we skip it (covers Y79Y/002's `[NEWLINE, "\t",
///   NEWLINE]` inside a flow sequence).
/// - **block scalar (`|` / `>`) ending with `\n` → tab-bearing
///   WHITESPACE**. The scalar token absorbs everything it can fit into
///   its body; if the next byte after the trailing newline is a tab, the
///   tab is sitting in the scalar's body-line indentation slot (Y79Y/000).
/// - **block indicator (`-` / `?` / `:`) → tab-bearing WHITESPACE →
///   nested block collection**. After `s-separate-in-line` the spec
///   wants `s-indent(n+m)` for the inner collection, which is defined as
///   spaces only; a tab in this separator slot makes the inner block's
///   indentation column ambiguous (Y79Y/004-009).
///
/// Tabs in WHITESPACE that *don't* sit in an indentation slot — e.g.
/// `-\tscalar`, where `s-separate-in-line` legitimately accepts a tab
/// between the indicator and a plain scalar — are not flagged.
fn check_tab_as_indent(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    fn tab_diag(t: &crate::syntax::SyntaxToken) -> YamlDiagnostic {
        diag_at_range(
            t.text_range().start().into(),
            t.text_range().end().into(),
            diagnostic_codes::PARSE_UNEXPECTED_INDENT,
            "tab character used as indentation is not allowed in YAML",
        )
    }
    fn flow_has_block_ancestor(n: &SyntaxNode) -> bool {
        n.ancestors().any(|a| {
            matches!(
                a.kind(),
                SyntaxKind::YAML_BLOCK_MAP
                    | SyntaxKind::YAML_BLOCK_SEQUENCE
                    | SyntaxKind::YAML_BLOCK_MAP_KEY
                    | SyntaxKind::YAML_BLOCK_MAP_VALUE
                    | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
            )
        })
    }
    for node in tree.descendants().filter(|n| {
        let is_block = matches!(
            n.kind(),
            SyntaxKind::YAML_BLOCK_MAP_VALUE
                | SyntaxKind::YAML_BLOCK_MAP_KEY
                | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
                | SyntaxKind::YAML_BLOCK_MAP
                | SyntaxKind::YAML_BLOCK_SEQUENCE
        );
        let is_nested_flow = matches!(
            n.kind(),
            SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
        ) && flow_has_block_ancestor(n);
        is_block || is_nested_flow
    }) {
        let children: Vec<_> = node.children_with_tokens().collect();
        for (i, child) in children.iter().enumerate() {
            let NodeOrToken::Token(t) = child else {
                continue;
            };
            if t.kind() != SyntaxKind::WHITESPACE || !t.text().contains('\t') {
                continue;
            }
            let starts_with_tab = t.text().starts_with('\t');

            let prev_kind = i
                .checked_sub(1)
                .and_then(|j| children.get(j))
                .and_then(|c| match c {
                    NodeOrToken::Token(pt) => Some((pt.kind(), pt.text().to_string())),
                    _ => None,
                });
            let next = children.get(i + 1);
            let next_is_newline = matches!(
                next,
                Some(NodeOrToken::Token(nt)) if nt.kind() == SyntaxKind::NEWLINE
            );
            let at_eof = next.is_none();

            // Case (a-newline): tab in indent slot after a NEWLINE
            // token. Only fires when the WHITESPACE *starts* with a
            // tab — a tab following leading spaces is "after
            // indentation" and is legal `s-separate-in-line` in flow
            // context (6HB6's `  \tStill by two` line). Whitespace-
            // only lines (next is NEWLINE or EOF) are line-folding
            // fodder, not indentation (Y79Y/002).
            let prev_is_newline_token = matches!(&prev_kind, Some((SyntaxKind::NEWLINE, _)));
            if prev_is_newline_token && starts_with_tab && !next_is_newline && !at_eof {
                return Some(tab_diag(t));
            }

            // Case (a-blockscalar): tab immediately after a block
            // scalar (`|` / `>`) whose text ends with `\n`. The
            // scalar absorbed everything it could fit into its body,
            // so a dangling tab in the body-line indent slot is
            // always an error, even if the next byte is another
            // newline (Y79Y/000).
            let prev_is_block_scalar_with_trailing_newline = matches!(&prev_kind, Some((SyntaxKind::YAML_SCALAR, text))
                    if (text.starts_with('|') || text.starts_with('>')) && text.ends_with('\n'));
            if prev_is_block_scalar_with_trailing_newline && starts_with_tab {
                return Some(tab_diag(t));
            }

            // Case (b): tab between a block indicator and a nested
            // block collection (same-line compact form). The inner
            // collection's indentation column would be set by the
            // tab — ambiguous per §6.1. The block indicator may be
            // an immediate sibling (`?`, `-` inside a key/item) or
            // the implicit `:` separator when WHITESPACE is the
            // first child of a YAML_BLOCK_MAP_VALUE (Y79Y/007,
            // Y79Y/009 where the colon lives in the sibling
            // YAML_BLOCK_MAP_KEY container).
            let prev_is_block_indicator = matches!(
                &prev_kind,
                Some((
                    SyntaxKind::YAML_BLOCK_SEQ_ENTRY
                        | SyntaxKind::YAML_KEY
                        | SyntaxKind::YAML_COLON,
                    _,
                ))
            );
            let leads_block_map_value = i == 0 && node.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE;
            let next_is_block_collection = matches!(
                next,
                Some(NodeOrToken::Node(n))
                    if matches!(
                        n.kind(),
                        SyntaxKind::YAML_BLOCK_SEQUENCE | SyntaxKind::YAML_BLOCK_MAP
                    )
            );
            if (prev_is_block_indicator || leads_block_map_value) && next_is_block_collection {
                return Some(tab_diag(t));
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
/// comment. Malformed shapes:
/// - 2G84/00: indent indicator `0` (must be `1-9`, e.g. `|0`).
/// - 2G84/01: multi-digit indent indicator (e.g. `|10`).
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
        // Skip indicator + chomping/indent characters, tracking digit run for §8.1.1.1
        // (indent indicator must be a single digit in `1-9`).
        let mut i = 1usize;
        let mut digit_start: Option<usize> = None;
        let mut digit_end: usize = i;
        let mut digit_count: usize = 0;
        let mut zero_digit: Option<usize> = None;
        while i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-' || bytes[i].is_ascii_digit())
        {
            if bytes[i].is_ascii_digit() {
                if digit_start.is_none() {
                    digit_start = Some(i);
                }
                digit_end = i + 1;
                digit_count += 1;
                if bytes[i] == b'0' && zero_digit.is_none() {
                    zero_digit = Some(i);
                }
            }
            i += 1;
        }
        if digit_count > 0 {
            let scalar_start: usize = token.text_range().start().into();
            // `|0`: zero as the indent indicator is invalid (range `[1-9]`).
            if let Some(zero_off) = zero_digit
                && digit_count == 1
            {
                return Some(diag_at_range(
                    scalar_start + zero_off,
                    scalar_start + zero_off + 1,
                    diagnostic_codes::PARSE_INVALID_KEY_TOKEN,
                    "block scalar indent indicator must be in range 1-9",
                ));
            }
            // `|10`: multi-digit run is never a valid indent indicator.
            if digit_count > 1
                && let Some(start_off) = digit_start
            {
                return Some(diag_at_range(
                    scalar_start + start_off,
                    scalar_start + digit_end,
                    diagnostic_codes::PARSE_INVALID_KEY_TOKEN,
                    "block scalar indent indicator must be a single digit in range 1-9",
                ));
            }
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

/// §8.1.1.1 — block-scalar leading empty line over-indented.
///
/// For a block scalar with *auto-detected* indentation (no explicit
/// indent-indicator digit in the header), the content indentation `m`
/// is the leading-space count of the first non-empty body line. The
/// spec forbids any *leading* empty line — one appearing before that
/// first non-empty line — from containing more spaces than `m`; such a
/// line would be more indented than the content it precedes, leaving
/// the auto-detected indentation ambiguous.
///
/// Block scalars are captured as a single `>`/`|`-prefixed
/// `YAML_SCALAR` token (header + body) whose body lines we walk
/// directly. An explicit indent indicator skips the check (the
/// indentation is then fixed, not detected). Tab-bearing lines are
/// handled conservatively: a tab in the first non-empty line bails out
/// (other checks own tab errors), and whitespace-only tab lines are
/// skipped rather than space-compared.
///
/// Covers fixtures 5LLU, S98Z, W9L4.
fn check_block_scalar_leading_indent(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
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
        // Skip indicator + chomping/indent characters; a digit among
        // them is an explicit indent indicator, which disables the
        // auto-detection rule this check enforces.
        let bytes = text.as_bytes();
        let mut i = 1usize;
        let mut explicit_indent = false;
        while i < header_end && (bytes[i] == b'+' || bytes[i] == b'-' || bytes[i].is_ascii_digit())
        {
            explicit_indent |= bytes[i].is_ascii_digit();
            i += 1;
        }
        if explicit_indent {
            continue;
        }

        let scalar_start: usize = token.text_range().start().into();
        // Leading blank lines, as (leading-space count, byte offset in `text`).
        let mut leading_blanks: Vec<(usize, usize)> = Vec::new();
        let mut cursor = header_end + 1; // first byte after the header's newline
        while cursor <= text.len() {
            let line_end = text[cursor..]
                .find('\n')
                .map(|rel| cursor + rel)
                .unwrap_or(text.len());
            let line = &text[cursor..line_end];

            if line.bytes().any(|b| b == b'\t') {
                if line.trim_matches([' ', '\t']).is_empty() {
                    // Whitespace-only line with a tab: not space-comparable.
                    if line_end >= text.len() {
                        break;
                    }
                    cursor = line_end + 1;
                    continue;
                }
                // First non-empty line carries a tab — leave it to the
                // tab-indent / block-indent checks.
                break;
            }

            let space_count = line.bytes().take_while(|b| *b == b' ').count();
            if space_count == line.len() {
                leading_blanks.push((space_count, cursor));
            } else {
                // First non-empty content line establishes `m`.
                let m = space_count;
                if let Some(&(_, offset)) = leading_blanks.iter().find(|(sp, _)| *sp > m) {
                    let at = scalar_start + offset;
                    return Some(diag_at_range(
                        at,
                        at + 1,
                        diagnostic_codes::PARSE_UNEXPECTED_INDENT,
                        "block scalar leading empty line is more indented than its content",
                    ));
                }
                break;
            }

            if line_end >= text.len() {
                break;
            }
            cursor = line_end + 1;
        }
    }
    None
}

/// Cluster J — bare scalar at document level immediately followed by a
/// block-map whose first entry's key is colon-only.
///
/// The v2 builder emits this shape whenever a `key:` shape is present
/// but the "key" lives outside the block-map node — either because a
/// `---` document-start marker is on the same line (`--- key: value`),
/// or because a stray multi-line plain scalar precedes the first
/// colon (`this\n is\n  invalid: x`). YAML 1.2 rejects both shapes:
/// - `--- key: value` (and its multi-line continuation form) is
///   rejected by yaml-test-suite cases 9KBC and CXX2 — a compact block
///   mapping cannot start on the marker line.
/// - A bare scalar that drops into a block-map context without
///   forming its own key is `PARSE_INVALID_KEY_TOKEN`.
///
/// Both shapes share a common CST signature: a `YAML_SCALAR` token
/// directly inside `YAML_DOCUMENT`, immediately followed by a
/// `YAML_BLOCK_MAP` whose first `YAML_BLOCK_MAP_KEY` contains only a
/// `YAML_COLON` token (no preceding scalar). The validator
/// distinguishes the two by checking whether a `YAML_DOCUMENT_START`
/// token appears as a direct child of the same document.
fn check_doc_level_bare_scalar_then_colon_map(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    if let Some(diag) = check_value_level_scalar_then_colon_map(tree) {
        return Some(diag);
    }
    for doc in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
    {
        let mut has_doc_start = false;
        let mut last_bare_scalar: Option<SyntaxToken> = None;
        for child in doc.children_with_tokens() {
            match &child {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::YAML_DOCUMENT_START => {
                        has_doc_start = true;
                    }
                    SyntaxKind::YAML_SCALAR => {
                        last_bare_scalar = Some(t.clone());
                    }
                    // Node properties (`&anchor`, `*alias`) are not
                    // bare scalars; they attach to the following content
                    // and must not reset or claim the slot.
                    SyntaxKind::YAML_ANCHOR | SyntaxKind::YAML_ALIAS => {}
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::YAML_COMMENT => {}
                    _ => {
                        last_bare_scalar = None;
                    }
                },
                NodeOrToken::Node(n) => {
                    if n.kind() == SyntaxKind::YAML_BLOCK_MAP
                        && let Some(scalar) = last_bare_scalar.take()
                        && first_entry_has_colon_only_key(n)
                    {
                        let (code, message) = if has_doc_start {
                            (
                                diagnostic_codes::LEX_TRAILING_CONTENT_AFTER_DOCUMENT_START,
                                "trailing content after document start marker",
                            )
                        } else {
                            (
                                diagnostic_codes::PARSE_INVALID_KEY_TOKEN,
                                "unexpected scalar at block-map level (no key)",
                            )
                        };
                        return Some(diag_at_range(
                            scalar.text_range().start().into(),
                            scalar.text_range().end().into(),
                            code,
                            message,
                        ));
                    }
                    last_bare_scalar = None;
                }
            }
        }
    }
    None
}

/// A `YAML_BLOCK_MAP_VALUE` containing a `YAML_SCALAR` immediately
/// followed by a `YAML_BLOCK_MAP` whose first entry's key is
/// colon-only. Two malformed shapes share this CST signature:
/// - Single-line inline nested mapping: `a: b: c` (ZCZ6) and
///   `a: 'b': c` (ZL4Z) — the value scalar is followed by a second
///   `: ` value-indicator on the same line, which YAML 1.2 forbids.
/// - Multi-line implicit key: `key:\n  word1 word2\n  no: key`
///   (HU3P) — §7.4 forbids an implicit key spanning lines.
///
/// Both are exempt when the value scalar is purely a node property
/// (anchor `&`, tag `!`, or alias `*`): the trailing `:` then
/// annotates an anchored/tagged value or its nested map, the valid
/// compact-mapping shapes W5VH and 26DV.
fn check_value_level_scalar_then_colon_map(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for value in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
    {
        let mut last_scalar: Option<SyntaxToken> = None;
        for child in value.children_with_tokens() {
            match &child {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::YAML_SCALAR => last_scalar = Some(t.clone()),
                    // Node properties (`&anchor`, `*alias`) are not
                    // bare scalars; they attach to the following content
                    // and must not reset or claim the slot.
                    SyntaxKind::YAML_ANCHOR | SyntaxKind::YAML_ALIAS => {}
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::YAML_COMMENT => {}
                    _ => last_scalar = None,
                },
                NodeOrToken::Node(n) => {
                    if n.kind() == SyntaxKind::YAML_BLOCK_MAP
                        && let Some(scalar) = last_scalar.take()
                        && first_entry_has_colon_only_key(n)
                        && scalar_is_content_implicit_key(scalar.text())
                    {
                        let message = if scalar.text().contains('\n') {
                            "implicit key cannot span lines"
                        } else {
                            "mapping values are not allowed in this context"
                        };
                        return Some(diag_at_range(
                            scalar.text_range().start().into(),
                            scalar.text_range().end().into(),
                            diagnostic_codes::PARSE_INVALID_KEY_TOKEN,
                            message,
                        ));
                    }
                    last_scalar = None;
                }
            }
        }
    }
    None
}

/// True when a value-level scalar that precedes a colon-only inner
/// block-map carries real implicit-key content, i.e. its first line
/// is not made up solely of node properties.
///
/// Flags HU3P (`word1 word2\n  no` → content), ZCZ6 (`b` → content),
/// and ZL4Z (`'b'` → content). Exempts 26DV (`&node3\n  *alias1`)
/// and W5VH (`&anchor:`), where the leading line is only an anchor
/// `&`, tag `!`, or alias `*` declaration: the actual key/value then
/// sits past the property and meets the YAML 1.2 §7.4 contract.
fn scalar_is_content_implicit_key(text: &str) -> bool {
    let first_line = text.split_once('\n').map_or(text, |(first, _)| first);
    let mut head = first_line.trim();
    while !head.is_empty() {
        let token_end = head.find(char::is_whitespace).unwrap_or(head.len());
        let (tok, rest) = head.split_at(token_end);
        let is_property = tok.starts_with('&') || tok.starts_with('!') || tok.starts_with('*');
        if !is_property {
            return true;
        }
        head = rest.trim_start();
    }
    false
}

/// True if `block_map`'s first `YAML_BLOCK_MAP_ENTRY` has a key
/// containing only a `YAML_COLON` token (i.e. no `YAML_SCALAR` child
/// inside the key).
fn first_entry_has_colon_only_key(block_map: &SyntaxNode) -> bool {
    let Some(first_entry) = block_map
        .children()
        .find(|c| c.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
    else {
        return false;
    };
    let Some(key) = first_entry
        .children()
        .find(|c| c.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
    else {
        return false;
    };
    let mut has_colon = false;
    for child in key.children_with_tokens() {
        match &child {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::YAML_COLON => has_colon = true,
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {}
                _ => return false,
            },
            NodeOrToken::Node(_) => return false,
        }
    }
    has_colon
}

/// Cluster J' — content scalar value followed by a deeper-indent
/// block collection inside the same `YAML_BLOCK_MAP_VALUE`.
///
/// YAML 1.2 §8.2.2 requires the value of an implicit block-mapping
/// entry to be either a single node or empty. Once a content scalar
/// has terminated the value (e.g. `key1: "quoted1"`), the next
/// non-blank line at a deeper indent column cannot start a fresh
/// nested block collection underneath the same key — the indent
/// implies sub-content, but the value slot is already filled.
///
/// CST signature: a `YAML_BLOCK_MAP_VALUE` whose direct children
/// include a content `YAML_SCALAR`, then `NEWLINE` trivia, then a
/// `YAML_BLOCK_MAP` or `YAML_BLOCK_SEQUENCE`. Compact-mapping shapes
/// (`a: <scalar>: <value>`, W5VH/26DV) put the scalar and the inner
/// map on the same line with no separating newline, so they are
/// preserved by the `NEWLINE`-between requirement.
///
/// Node-property–only scalars (`&anchor`, `!tag`, `*alias`) are
/// exempt: the scanner currently folds anchor/alias/tag indicators
/// into plain-scalar tokens, so a placeholder scalar like `&node1`
/// preceding the actual value's block collection is a parser shadow
/// that should not be flagged. `scalar_is_content_implicit_key`
/// already encodes that exemption.
///
/// Covers fixture U44R (`key1: "quoted1"\n   key2: ...`).
fn check_block_collection_after_value_scalar(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for value in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
    {
        let mut last_scalar: Option<SyntaxToken> = None;
        let mut saw_newline_after_scalar = false;
        for child in value.children_with_tokens() {
            match &child {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::YAML_SCALAR => {
                        last_scalar = Some(t.clone());
                        saw_newline_after_scalar = false;
                    }
                    SyntaxKind::NEWLINE => {
                        if last_scalar.is_some() {
                            saw_newline_after_scalar = true;
                        }
                    }
                    SyntaxKind::WHITESPACE | SyntaxKind::YAML_COMMENT => {}
                    _ => {
                        last_scalar = None;
                        saw_newline_after_scalar = false;
                    }
                },
                NodeOrToken::Node(n) => {
                    if saw_newline_after_scalar
                        && matches!(
                            n.kind(),
                            SyntaxKind::YAML_BLOCK_MAP | SyntaxKind::YAML_BLOCK_SEQUENCE
                        )
                        && let Some(scalar) = last_scalar.as_ref()
                        && scalar_is_content_implicit_key(scalar.text())
                    {
                        return Some(diag_at_range(
                            n.text_range().start().into(),
                            (Into::<usize>::into(n.text_range().start())) + 1,
                            diagnostic_codes::PARSE_UNEXPECTED_INDENT,
                            "block collection cannot follow a scalar value at a deeper indent",
                        ));
                    }
                    last_scalar = None;
                    saw_newline_after_scalar = false;
                }
            }
        }
    }
    None
}

/// Cluster K — flow collection inside a block-map value whose
/// continuation lines drop to or below the parent block-map's indent
/// column.
///
/// YAML 1.2 §7.1 requires flow content nested inside a block-map
/// value to be indented strictly past the block context. The v1
/// lexer surfaces this as `LEX_WRONG_INDENTED_FLOW` with the contract
/// `line_indent <= flow_base_indent` where `flow_base_indent` is the
/// indent of the line that opened the flow. The v2-aware analog: walk
/// each `YAML_FLOW_SEQUENCE` / `YAML_FLOW_MAP` whose ancestor chain
/// includes a `YAML_BLOCK_MAP_VALUE`, and verify that every line
/// inside the flow node's byte range starts at a column strictly
/// greater than the column of the enclosing `YAML_BLOCK_MAP`.
///
/// Top-level flow collections (no block-map ancestor) are exempt —
/// v1 only sets `flow_requires_indent` when the flow opens inside a
/// raw block-mapping value.
fn check_flow_continuation_indent(tree: &SyntaxNode, input: &str) -> Option<YamlDiagnostic> {
    for flow in tree.descendants().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
        )
    }) {
        let Some(block_map) = enclosing_block_map_for_flow(&flow) else {
            continue;
        };
        let block_map_start: usize = block_map.text_range().start().into();
        let threshold = column_of(input, block_map_start);
        let flow_start: usize = flow.text_range().start().into();
        let flow_end: usize = flow.text_range().end().into();
        let bytes = input.as_bytes();
        let mut i = flow_start;
        while i < flow_end {
            if bytes[i] != b'\n' {
                i += 1;
                continue;
            }
            let line_start = i + 1;
            if line_start >= flow_end {
                break;
            }
            let mut col = 0usize;
            let mut j = line_start;
            while j < flow_end && (bytes[j] == b' ' || bytes[j] == b'\t') {
                col += 1;
                j += 1;
            }
            // Blank-only continuation lines do not impose indent.
            if j >= flow_end || bytes[j] == b'\n' {
                i = j;
                continue;
            }
            if col <= threshold {
                return Some(diag_at_range(
                    line_start,
                    j + 1,
                    diagnostic_codes::LEX_WRONG_INDENTED_FLOW,
                    "wrong indentation for continued flow collection",
                ));
            }
            i = j;
        }
    }
    None
}

/// Walk the ancestor chain of `flow` and return the nearest
/// enclosing `YAML_BLOCK_MAP` whose body owns a `YAML_BLOCK_MAP_VALUE`
/// containing the flow. Returns `None` for top-level flows or flows
/// not nested inside a block-map value.
fn enclosing_block_map_for_flow(flow: &SyntaxNode) -> Option<SyntaxNode> {
    let mut node = flow.parent();
    let mut saw_block_map_value = false;
    while let Some(current) = node {
        match current.kind() {
            SyntaxKind::YAML_BLOCK_MAP_VALUE => saw_block_map_value = true,
            SyntaxKind::YAML_BLOCK_MAP if saw_block_map_value => return Some(current),
            _ => {}
        }
        node = current.parent();
    }
    None
}

/// Detects YAML document markers (`---`, `...`) appearing at the start
/// of a line inside a flow collection. YAML 1.2 §9.1.1 forbids document
/// marker lines from appearing within flow content; once a `[` or `{`
/// opens a flow context the entire flow must close before a new document
/// can begin. The scanner currently folds the marker into a plain scalar
/// at column 0, so we surface the violation by walking flow-collection
/// scalars and matching `---`/`...` followed by space/newline/end.
///
/// Covers fixture N782 (`[\n---,\n...\n]`).
fn check_flow_doc_markers(tree: &SyntaxNode, input: &str) -> Option<YamlDiagnostic> {
    let bytes = input.as_bytes();
    for flow in tree.descendants().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
        )
    }) {
        for scalar in flow.descendants_with_tokens().filter_map(|c| match c {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::YAML_SCALAR => Some(t),
            _ => None,
        }) {
            let start: usize = scalar.text_range().start().into();
            let at_line_start = start == 0 || bytes.get(start - 1) == Some(&b'\n');
            if !at_line_start {
                continue;
            }
            let text = scalar.text();
            let head = text.as_bytes();
            // Only plain scalars (no quote/block-style prefix) can carry
            // the marker shape; quoted text starting with `---`/`...` is
            // legal content.
            if matches!(head.first(), Some(b'"' | b'\'' | b'|' | b'>')) {
                continue;
            }
            let (msg, marker_len) = match head.get(..3) {
                Some(b"---") => ("`---` document marker not allowed in flow content", 3usize),
                Some(b"...") => ("`...` document marker not allowed in flow content", 3usize),
                _ => continue,
            };
            match head.get(marker_len) {
                None | Some(b' ' | b'\t' | b'\n') => {}
                _ => continue,
            }
            return Some(diag_at_range(
                start,
                start + marker_len,
                diagnostic_codes::PARSE_INVALID_PLAIN_SCALAR_IN_FLOW,
                msg,
            ));
        }
    }
    None
}

/// Cluster L — invalid double-quoted escape sequences.
///
/// Walks every `YAML_SCALAR` token whose text begins with `"` and
/// looks for `\` followed by a character not in YAML 1.2's escape
/// table (§5.7). Emits `LEX_INVALID_DOUBLE_QUOTED_ESCAPE` at the
/// position of the offending backslash. Mirrors the v1 lexer's
/// `invalid_double_quote_escape_offset` contract.
fn check_invalid_dq_escapes(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for token in tree
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::YAML_SCALAR)
    {
        let text = token.text();
        if !text.starts_with('"') {
            continue;
        }
        if let Some(rel_idx) = invalid_dq_escape_offset(text) {
            let scalar_start: usize = token.text_range().start().into();
            return Some(diag_at_range(
                scalar_start + rel_idx,
                scalar_start + rel_idx + 1,
                diagnostic_codes::LEX_INVALID_DOUBLE_QUOTED_ESCAPE,
                "invalid escape in double quoted scalar",
            ));
        }
    }
    None
}

/// Lex-level cluster — comment not preceded by whitespace.
///
/// YAML 1.2 §6.6: a comment (`c-nb-comment-text`) must follow
/// `s-separate-in-line` — at least one space/tab — or begin at the start
/// of a line. A `#` immediately abutting a non-space character is neither
/// a valid comment nor (since `#` is a `c-indicator`) a valid plain-scalar
/// start, so the document is in error.
///
/// The streaming scanner tokenizes such a `#…` run as a `YAML_COMMENT`
/// (preserving the bytes), so we walk every comment token and reject the
/// first whose preceding character is not a space, tab, or line break.
///
/// Covers fixtures SU5Z (`key: "value"# invalid comment`) and CVW2
/// (`[ a, b, c,#invalid ]`).
fn check_comment_not_preceded_by_space(tree: &SyntaxNode, input: &str) -> Option<YamlDiagnostic> {
    for token in tree
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::YAML_COMMENT)
    {
        let start: usize = token.text_range().start().into();
        // Start of input / line start, or a separating space/tab → valid.
        let preceded_ok = matches!(
            input[..start].chars().next_back(),
            None | Some('\n' | '\r' | ' ' | '\t')
        );
        if !preceded_ok {
            return Some(diag_at_range(
                start,
                token.text_range().end().into(),
                diagnostic_codes::LEX_COMMENT_NOT_PRECEDED_BY_SPACE,
                "comment must be preceded by whitespace",
            ));
        }
    }
    None
}

/// Cluster M — anchor decorates alias.
///
/// YAML 1.2 §6.9.2: an alias node (`*name`) is itself a complete node
/// and cannot carry node properties. In particular, an anchor cannot
/// decorate an alias: `&b *a` is invalid because the alias already
/// resolves to the original anchored node and cannot be re-anchored.
///
/// We walk every container that holds node-property tokens directly
/// (`YAML_BLOCK_MAP_KEY`, `YAML_BLOCK_MAP_VALUE`,
/// `YAML_BLOCK_SEQUENCE_ITEM`, and the flow analogues), and look for a
/// `YAML_ANCHOR` token immediately followed — modulo trivia — by a
/// `YAML_ALIAS` token. When found, the diagnostic points at the alias
/// (the spec-illegal node).
///
/// Covers fixtures SR86 (`key2: &b *a`) and SU74 (`&b *alias : value`).
/// Cluster N — invalid character in tag.
///
/// YAML 1.2 §5.6 defines `ns-tag-char` as `ns-uri-char` minus `!` and
/// minus the flow indicators `,`, `[`, `]`, `{`, `}`. The scanner consumes
/// any non-whitespace char into the tag suffix (libyaml/PyYAML-style relaxed
/// name class) and only excludes flow indicators when already inside a flow
/// context. That keeps block-context tokens like `!invalid{}tag` or `!!str,`
/// in one piece for round-tripping, but the spec rejects them. Surface a
/// diagnostic at the first offending byte so cases like LHL4 and U99R end up
/// in the `error_contract_ok` bucket.
///
/// Verbatim form (`!<uri>`) is excluded: its body is `ns-uri-char+`, which
/// legitimately allows `,`, `[`, `]`.
fn check_invalid_tag_chars(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for tok in tree
        .descendants_with_tokens()
        .filter_map(|c| c.into_token())
    {
        if tok.kind() != SyntaxKind::YAML_TAG {
            continue;
        }
        let text = tok.text();
        if text.starts_with("!<") {
            continue;
        }
        for (offset, ch) in text.char_indices() {
            if matches!(ch, ',' | '{' | '}') {
                let start: usize = tok.text_range().start().into();
                return Some(diag_at_range(
                    start + offset,
                    start + offset + ch.len_utf8(),
                    diagnostic_codes::PARSE_INVALID_TAG_CHARACTER,
                    "invalid character in tag",
                ));
            }
        }
    }
    None
}

/// Cluster M — anchor decorates a non-node. YAML 1.2 §6.9.2: an alias
/// is a complete node and cannot carry node properties, and §6.9.2
/// likewise forbids multiple anchors on a single node. Either pattern
/// is rejected here.
///
/// Covers:
/// - SR86 / SU74 (`&b *alias`): `YAML_ANCHOR` followed by a
///   `YAML_ALIAS` token within the same value/key/item container.
/// - 4JVG (`top2: &node2\n  &v2 val2`): two `YAML_ANCHOR` tokens
///   within the same container with no node between them — the
///   spec allows only one anchor per node.
fn check_anchor_decorates_alias(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for container in tree.descendants().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_BLOCK_MAP_KEY
                | SyntaxKind::YAML_BLOCK_MAP_VALUE
                | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
                | SyntaxKind::YAML_FLOW_MAP_KEY
                | SyntaxKind::YAML_FLOW_MAP_VALUE
                | SyntaxKind::YAML_FLOW_SEQUENCE_ITEM
        )
    }) {
        let mut saw_anchor = false;
        for child in container.children_with_tokens() {
            let NodeOrToken::Token(tok) = child else {
                saw_anchor = false;
                continue;
            };
            match tok.kind() {
                SyntaxKind::YAML_ANCHOR if saw_anchor => {
                    return Some(diag_at_range(
                        tok.text_range().start().into(),
                        tok.text_range().end().into(),
                        diagnostic_codes::PARSE_MULTIPLE_ANCHORS_ON_NODE,
                        "node cannot have multiple anchors",
                    ));
                }
                SyntaxKind::YAML_ANCHOR => saw_anchor = true,
                SyntaxKind::YAML_ALIAS if saw_anchor => {
                    return Some(diag_at_range(
                        tok.text_range().start().into(),
                        tok.text_range().end().into(),
                        diagnostic_codes::PARSE_ANCHOR_DECORATES_ALIAS,
                        "alias node cannot be decorated with an anchor",
                    ));
                }
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::YAML_COMMENT => {}
                _ => saw_anchor = false,
            }
        }
    }
    None
}

/// Cluster O — anchor immediately precedes a block sequence indicator
/// on the same line. YAML 1.2 §8.1: a block sequence's `-` must start
/// a new line; an anchor cannot share the line with the first `-`
/// because the anchor's node would have to be the sequence itself,
/// which requires the indicator on its own line at appropriate
/// indent.
///
/// Covers SY6V (`&anchor - sequence entry`).
fn check_anchor_before_block_indicator(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for container in tree.descendants().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_DOCUMENT
                | SyntaxKind::YAML_BLOCK_MAP_KEY
                | SyntaxKind::YAML_BLOCK_MAP_VALUE
                | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
        )
    }) {
        let mut anchor_pending: Option<(usize, usize)> = None;
        for child in container.children_with_tokens() {
            match child {
                NodeOrToken::Token(tok) => match tok.kind() {
                    SyntaxKind::YAML_ANCHOR => {
                        anchor_pending = Some((
                            tok.text_range().start().into(),
                            tok.text_range().end().into(),
                        ));
                    }
                    SyntaxKind::WHITESPACE | SyntaxKind::YAML_COMMENT => {}
                    SyntaxKind::NEWLINE => {
                        anchor_pending = None;
                    }
                    _ => {
                        anchor_pending = None;
                    }
                },
                NodeOrToken::Node(node) => {
                    if let Some((start, end)) = anchor_pending.take()
                        && node.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE
                    {
                        return Some(diag_at_range(
                            start,
                            end,
                            diagnostic_codes::PARSE_ANCHOR_BEFORE_BLOCK_INDICATOR,
                            "anchor cannot precede a block sequence indicator on the same line",
                        ));
                    }
                }
            }
        }
    }
    None
}

/// Cluster P — anchor inside a block sequence item with no target.
/// YAML 1.2 §6.9.2 requires every node property (anchor or tag) to
/// be attached to a following node. A `YAML_ANCHOR` token that
/// appears *after* the item's value scalar/node has no target it
/// could legitimately decorate.
///
/// Covers GT5M (`- item1\n&node\n- item2`): the parser folds the
/// stray anchor into the preceding item, but it cannot belong there
/// — items are single-value containers and the anchor follows that
/// single value.
fn check_anchor_without_target(tree: &SyntaxNode) -> Option<YamlDiagnostic> {
    for container in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
    {
        let mut saw_value = false;
        for child in container.children_with_tokens() {
            match child {
                NodeOrToken::Token(tok) => match tok.kind() {
                    SyntaxKind::YAML_ANCHOR if saw_value => {
                        return Some(diag_at_range(
                            tok.text_range().start().into(),
                            tok.text_range().end().into(),
                            diagnostic_codes::PARSE_ANCHOR_WITHOUT_TARGET,
                            "anchor has no target node",
                        ));
                    }
                    SyntaxKind::YAML_SCALAR | SyntaxKind::YAML_ALIAS => saw_value = true,
                    _ => {}
                },
                NodeOrToken::Node(_) => saw_value = true,
            }
        }
    }
    None
}

fn invalid_dq_escape_offset(text: &str) -> Option<usize> {
    let mut chars = text.char_indices().peekable();
    let mut in_double = false;
    let mut escape_start: Option<usize> = None;
    while let Some((idx, ch)) = chars.next() {
        if !in_double {
            if ch == '"' {
                in_double = true;
            }
            continue;
        }
        if let Some(start) = escape_start.take() {
            if !is_valid_dq_escape(ch) {
                return Some(start);
            }
            continue;
        }
        match ch {
            '\\' => {
                if chars.peek().is_none() {
                    return Some(idx);
                }
                escape_start = Some(idx);
            }
            '"' => in_double = false,
            _ => {}
        }
    }
    None
}

fn is_valid_dq_escape(ch: char) -> bool {
    matches!(
        ch,
        '0' | 'a'
            | 'b'
            | 't'
            // `\<TAB>` is accepted by the scanner's escape table (§5.7).
            | '\t'
            // `\<line-break>` is the escaped line break / line continuation
            // (§7.5); the multi-line scalar token carries a literal break here.
            | '\n'
            | '\r'
            | 'n'
            | 'v'
            | 'f'
            | 'r'
            | 'e'
            | ' '
            | '"'
            | '/'
            | '\\'
            | 'N'
            | '_'
            | 'L'
            | 'P'
            | 'x'
            | 'u'
            | 'U'
    )
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
    fn unterminated_quoted_scalar_at_eof_cq3w() {
        // CQ3W: a double-quoted value that never reaches its closing quote.
        let input = "---\nkey: \"missing closing quote";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::LEX_UNTERMINATED_QUOTED_SCALAR);
    }

    #[test]
    fn unterminated_quoted_scalar_aborted_by_doc_marker_5trb_rxy3() {
        // 5TRB / RXY3: a `---`/`...` marker at column 0 aborts an open
        // quoted scalar before its closing quote is found.
        for input in ["---\n\"\n---\n\"\n", "---\n'\n...\n'\n"] {
            let diag = run(input).expect("expected diagnostic");
            assert_eq!(
                diag.code,
                diagnostic_codes::LEX_UNTERMINATED_QUOTED_SCALAR,
                "{input:?}"
            );
        }
    }

    #[test]
    fn block_scalar_leading_blank_overindented_5llu() {
        // 5LLU: folded scalar, leading blanks at 1/2/3 spaces, first
        // content line `invalid` at 1 space.
        let input = "block scalar: >\n \n  \n   \n invalid\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_INDENT);
    }

    #[test]
    fn block_scalar_leading_blank_overindented_w9l4() {
        // W9L4: literal scalar, leading blank at 5 spaces, content at 2.
        let input = "---\nblock scalar: |\n     \n  more spaces at the beginning\n  are invalid\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_INDENT);
    }

    #[test]
    fn block_scalar_leading_blank_overindented_s98z() {
        // S98Z: folded scalar, leading blanks at 1/2/3 spaces, first
        // non-empty line ` # comment` at 1 space (a `#` is literal
        // content inside a block scalar).
        let input = "empty block scalar: >\n \n  \n   \n # comment\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_INDENT);
    }

    #[test]
    fn block_scalar_explicit_indent_indicator_not_flagged() {
        // Explicit indent indicator ⇒ not auto-detected; the §8.1.1.1
        // leading-blank rule does not apply (and a deeper first line is
        // legitimate content).
        let input = "a: |2\n     \n   more\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
    }

    #[test]
    fn block_scalar_well_indented_leading_blank_passes() {
        // Leading blank (1 space) is not more indented than content
        // (2 spaces) ⇒ no error.
        let input = "a: |\n \n  body\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
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
    fn duplicate_yaml_directive_sf5v() {
        // SF5V: two `%YAML` directives precede the same `---` — a document
        // may carry at most one YAML directive (spec §6.8.1).
        let input = "%YAML 1.2\n%YAML 1.2\n---\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_DUPLICATE_YAML_DIRECTIVE);
    }

    #[test]
    fn malformed_yaml_directive_trailing_content_h7tq() {
        // H7TQ: `%YAML 1.2 foo` — the YAML directive takes a single version
        // argument; `foo` is invalid trailing content.
        let input = "%YAML 1.2 foo\n---\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_MALFORMED_YAML_DIRECTIVE);
    }

    #[test]
    fn yaml_directive_with_trailing_comment_passes() {
        // A trailing comment after the version is allowed.
        let input = "%YAML 1.2 # comment\n---\nfoo: bar\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn yaml_then_tag_directive_passes() {
        // One `%YAML` plus one `%TAG` is well-formed; the duplicate check is
        // scoped to `%YAML` only.
        let input = "%YAML 1.2\n%TAG ! tag:example.com,2000:app/\n---\nfoo: bar\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn yaml_directives_across_documents_pass() {
        // A second `%YAML` after a `...` belongs to a new document — not a
        // duplicate.
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

    #[test]
    fn flow_seq_lone_dash_yjv2() {
        // YJV2: `[-]` — `-` is followed by the flow close `]`, so it is a
        // bare indicator, not a valid plain scalar.
        let input = "[-]\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_INVALID_PLAIN_SCALAR_IN_FLOW
        );
    }

    #[test]
    fn flow_seq_lone_dash_items_g5u8() {
        // G5U8: `- [-, -]` — both flow-seq items are a lone `-` followed
        // by a flow indicator (`,` then `]`).
        let input = "---\n- [-, -]\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_INVALID_PLAIN_SCALAR_IN_FLOW
        );
    }

    #[test]
    fn flow_seq_dash_prefixed_scalar_passes() {
        // Sanity: `[-1, -x]` — the `-` is followed by a plain-safe char,
        // so each item is a legitimate plain scalar, not a bare dash.
        let input = "[-1, -x]\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
    }

    #[test]
    fn block_dash_prefixed_scalar_passes() {
        // The flow-only rule must not touch block context: `key: -x` is a
        // plain scalar value (`-` not followed by a space) outside any
        // flow collection, so it carries no flow-node kinds to match.
        let input = "key: -x\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
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
    fn doc_level_comment_in_multiline_plain_bs4k() {
        // BS4K: a comment splits a multi-line plain scalar that sits
        // at the document root (no enclosing block map / sequence).
        let input = "word1  # comment\nword2\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_UNEXPECTED_DEDENT);
    }

    #[test]
    fn doc_level_single_multiline_plain_passes() {
        // Plain scalar that legitimately spans multiple lines at the
        // document root (no intervening comment).
        let input = "word1\nword2\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
    }

    #[test]
    fn multi_document_each_with_single_scalar_passes() {
        // Two distinct documents, each containing a single scalar.
        let input = "---\nfoo\n---\nbar\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
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

    // ---- Cluster J: inline nested mapping in a block-map value ----

    #[test]
    fn value_level_inline_nested_map_zcz6() {
        // ZCZ6: `a: b: c: d` — the block-map value `b` is followed by a
        // second `: ` value-indicator on the same line, forming an
        // illegal inline nested mapping.
        let input = "a: b: c: d\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_INVALID_KEY_TOKEN);
    }

    #[test]
    fn value_level_inline_nested_map_quoted_zl4z() {
        // ZL4Z: `a: 'b': c` — a quoted block-map value followed by a `: `
        // value-indicator is the same illegal inline nested mapping.
        let input = "---\na: 'b': c\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_INVALID_KEY_TOKEN);
    }

    #[test]
    fn value_level_property_only_scalar_then_colon_passes_w5vh() {
        // W5VH: `a: &anchor: scalar` — the value-level scalar is purely a
        // node property (anchor), so the trailing `:` annotates an anchored
        // value, not an inline nested mapping. Must stay accepted.
        let input = "a: &anchor: scalar a\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn value_level_colon_without_space_passes() {
        // `a: b:c` — the inner colon is not followed by whitespace, so
        // `b:c` is a single plain scalar value, not a nested mapping.
        let input = "a: b:c\n";
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

    // ---- Cluster L: double-quoted escapes ----

    #[test]
    fn dq_escaped_line_break_passes_np9h() {
        // NP9H (spec 7.5): a `\` at end of line is the escaped line break
        // (line continuation), not an invalid escape. The validator's
        // escape table previously omitted `\n`/`\r` and falsely rejected it.
        let input = "\"folded \nto a space,\t\n \nto a line feed, or \t\\\n \\ \tnon-content\"\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn dq_escaped_line_break_with_marker_passes_q8ad() {
        // Q8AD: same escaped-line-break scalar behind a `---` marker.
        let input =
            "---\n\"folded \nto a space,\n \nto a line feed, or \t\\\n \\ \tnon-content\"\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn dq_escaped_tab_passes() {
        // `\<TAB>` is accepted by the scanner's escape table; the
        // validator must agree so the two paths don't diverge.
        let input = "key: \"a\\\tb\"\n";
        assert!(run(input).is_none());
    }

    #[test]
    fn dq_truly_invalid_escape_still_rejected() {
        // Contract guard: a genuinely unknown escape (`\q`) is still flagged.
        let input = "key: \"a\\qb\"\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::LEX_INVALID_DOUBLE_QUOTED_ESCAPE
        );
    }

    #[test]
    fn comment_abutting_closing_quote_rejected_su5z() {
        // SU5Z: a `#` directly after a closing quote with no separating
        // space is not a valid comment (§6.6).
        let input = "key: \"value\"# invalid comment\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::LEX_COMMENT_NOT_PRECEDED_BY_SPACE
        );
    }

    #[test]
    fn comment_abutting_flow_comma_rejected_cvw2() {
        // CVW2: a `#` immediately following a flow `,` separator.
        let input = "---\n[ a, b, c,#invalid\n]\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::LEX_COMMENT_NOT_PRECEDED_BY_SPACE
        );
    }

    #[test]
    fn comment_preceded_by_space_passes() {
        // Contract guard: a properly separated comment must not be flagged.
        for input in [
            "key: value # ok\n",
            "# line-start comment\nkey: value\n",
            "key: value\t# tab-separated\n",
        ] {
            assert!(run(input).is_none(), "{input:?}");
        }
    }

    #[test]
    fn anchor_decorates_alias_sr86() {
        // SR86: anchor on a block-map value whose body is itself an alias.
        let input = "key1: &a value\nkey2: &b *a\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_ANCHOR_DECORATES_ALIAS);
    }

    #[test]
    fn anchor_decorates_alias_su74() {
        // SU74: anchor + alias used together as a block-map key.
        let input = "key1: &alias value1\n&b *alias : value2\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_ANCHOR_DECORATES_ALIAS);
    }

    #[test]
    fn anchor_followed_by_scalar_passes() {
        // Contract guard: a plain anchor decorating an ordinary scalar
        // (i.e. not an alias) must not be flagged.
        let input = "key: &a value\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
    }

    #[test]
    fn lone_alias_without_anchor_passes() {
        // Contract guard: a bare alias (no preceding anchor sibling) is
        // valid.
        let input = "key1: &a value\nkey2: *a\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
    }

    #[test]
    fn invalid_tag_braces_lhl4() {
        // LHL4: tag suffix carries `{}` which are c-flow-indicators,
        // forbidden from ns-tag-char.
        let input = "---\n!invalid{}tag scalar\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_INVALID_TAG_CHARACTER);
    }

    #[test]
    fn invalid_tag_comma_u99r() {
        // U99R: `!!str,` — comma is a c-flow-indicator and not a
        // valid ns-tag-char.
        let input = "- !!str, xxx\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_INVALID_TAG_CHARACTER);
    }

    #[test]
    fn valid_tag_passes() {
        // Contract guard: tags from the well-formed allowlist must not
        // trip the invalid-char check.
        let input = "key: !!str value\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
    }

    #[test]
    fn verbatim_tag_with_uri_chars_passes() {
        // Contract guard: verbatim form `!<uri>` allows any ns-uri-char
        // in the URI body, including `,` and `[`.
        let input = "key: !<tag:example.com,2011:foo[bar]> value\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
    }

    #[test]
    fn multiple_anchors_on_node_4jvg() {
        // 4JVG: scalar value decorated with two anchors
        // (`top2: &node2\n  &v2 val2`).
        let input = "top1: &node1\n  &k1 key1: val1\ntop2: &node2\n  &v2 val2\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_MULTIPLE_ANCHORS_ON_NODE);
    }

    #[test]
    fn single_anchor_per_node_passes() {
        // Contract guard: one anchor per value is fine, even when
        // siblings carry anchors of their own.
        let input = "k1: &a 1\nk2: &b 2\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
    }

    #[test]
    fn anchor_before_block_seq_indicator_sy6v() {
        // SY6V: `&anchor - sequence entry` — anchor must not share its
        // line with the leading block-sequence `-`.
        let input = "&anchor - sequence entry\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(
            diag.code,
            diagnostic_codes::PARSE_ANCHOR_BEFORE_BLOCK_INDICATOR
        );
    }

    #[test]
    fn anchor_on_own_line_before_block_seq_passes() {
        // Contract guard: anchor on its own line preceding a block
        // sequence is valid.
        let input = "&anchor\n- item\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
    }

    #[test]
    fn anchor_without_target_gt5m() {
        // GT5M: `- item1\n&node\n- item2` — anchor between sequence
        // items has no target node.
        let input = "- item1\n&node\n- item2\n";
        let diag = run(input).expect("expected diagnostic");
        assert_eq!(diag.code, diagnostic_codes::PARSE_ANCHOR_WITHOUT_TARGET);
    }

    #[test]
    fn anchor_before_block_seq_item_value_passes() {
        // Contract guard: `- &a item` — anchor before the item's value
        // is valid.
        let input = "- &a item\n- b\n";
        assert!(run(input).is_none(), "got {:?}", run(input));
    }
}
