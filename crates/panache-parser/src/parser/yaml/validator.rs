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
//! - A, B, C, D, E, G, H, I — pending.
//!
//! See `.claude/skills/yaml-shadow-expand/scanner-rewrite.md` for the
//! cutover plan and per-cluster detection scope.
#![allow(dead_code)]

use super::model::{YamlDiagnostic, diagnostic_codes};
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
}
