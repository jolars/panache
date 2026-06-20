//! `textDocument/semanticTokens/full`: additive, flavor-gated highlighting.
//!
//! This is deliberately *additive*, not a replacement highlighter. Editors
//! already highlight Markdown with CommonMark/GFM grammars (tree-sitter,
//! TextMate); we only emit tokens for the pandoc/quarto-specific constructs
//! those grammars miss (citations, cross-refs, fenced-div attributes,
//! shortcodes, math markers, footnote refs, bracketed-span attributes). Base
//! constructs (headings, emphasis, links, code) are left to the editor so we
//! never visibly recolor highlighting the user was happy with. (Raw-inline
//! format tags `{=html}` are deferred: the parser folds them into a generic
//! `ATTRIBUTE` node rather than a dedicated kind, so a later step can target
//! them without overlapping the bracketed-span attribute path.)
//!
//! The legend uses **custom** token types, so they no-op in any theme until the
//! user opts in via `editor.semanticTokenColorCustomizations` (VS Code) or
//! `@lsp.type.*` highlight links (neovim) — see `docs/guide/lsp.qmd`. That makes
//! the provider safe to advertise on by default.
//!
//! Flavor gate: CommonMark/GFM documents get zero tokens (panache's parse and
//! the editor's base grammar agree there, so there is nothing to add).
//!
//! Step 1 scope: `full` only (no `range`, no delta) and single-line tokens only.
//! The collected kinds (markers, keys, single-line info/attr nodes) are
//! inherently single-line; the encoder's cross-line guard is a safety net for
//! the rare multi-line shortcode/span, whose bodies are deferred to a later
//! step.

use lsp_types::{
    SemanticToken, SemanticTokenType, SemanticTokens, SemanticTokensLegend, SemanticTokensParams,
    SemanticTokensResult,
};
use rowan::TextRange;

use crate::config::Flavor;
use crate::lsp::conversions::offset_to_position;
use crate::lsp::global_state::StateSnapshot;
use crate::syntax::{SyntaxKind, SyntaxNode};

/// Custom token-type legend. Index = `token_type` emitted in the delta stream.
/// Keep in sync with [`token_type_for`].
const TOKEN_TYPES: &[&str] = &[
    "citation",  // 0: @key / [@key]
    "crossref",  // 1: @fig-1, \@ref(...)  (Quarto/RMarkdown flavors)
    "shortcode", // 2: {{< name args >}}
    "div",       // 3: fenced-div info string {.class ...}
    "math",      // 4: $ / $$ delimiters
    "footnote",  // 5: [^id] reference
    "attribute", // 6: bracketed-span attributes {.class ...}
];

/// The legend advertised in `ServerCapabilities` and referenced by the encoded
/// token-type indices. No modifiers in step 1.
pub(crate) fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES
            .iter()
            .map(|&name| SemanticTokenType::new(name))
            .collect(),
        token_modifiers: Vec::new(),
    }
}

/// Map a CST kind to its legend index, or `None` if we don't tokenize it.
///
/// The chosen kinds are mutually non-nesting (a `CITATION_KEY` never contains a
/// `DIV_INFO`, etc.), so collecting them in one descendant walk yields
/// non-overlapping tokens without subtree bookkeeping.
fn token_type_for(kind: SyntaxKind) -> Option<u32> {
    Some(match kind {
        SyntaxKind::CITATION_KEY => 0,
        SyntaxKind::CROSSREF_KEY => 1,
        SyntaxKind::SHORTCODE => 2,
        SyntaxKind::DIV_INFO => 3,
        SyntaxKind::INLINE_MATH_MARKER | SyntaxKind::DISPLAY_MATH_MARKER => 4,
        SyntaxKind::FOOTNOTE_REFERENCE => 5,
        SyntaxKind::SPAN_ATTRIBUTES => 6,
        _ => return None,
    })
}

pub(crate) fn semantic_tokens_full(
    snap: &StateSnapshot,
    params: SemanticTokensParams,
) -> Option<SemanticTokensResult> {
    let uri = &params.text_document.uri;

    // Flavor gate: nothing to add over the editor's base grammar for plain
    // CommonMark/GFM. Return an empty (not absent) result so the client knows
    // the document was handled.
    if matches!(snap.config(uri).flavor, Flavor::CommonMark | Flavor::Gfm) {
        return Some(empty());
    }

    // A genuinely missing document is `None` → `ContentModified` (the doc moved
    // on under us); an empty token list is a real, successful answer.
    let (text, root) = snap.document_content_and_tree(uri)?;

    let tokens = collect_tokens(&root);
    let data = encode(&text, tokens);
    Some(SemanticTokensResult::Tokens(SemanticTokens {
        result_id: None,
        data,
    }))
}

fn empty() -> SemanticTokensResult {
    SemanticTokensResult::Tokens(SemanticTokens {
        result_id: None,
        data: Vec::new(),
    })
}

/// One descendant walk, bucketing the kinds we tokenize. Returns `(range, type)`
/// sorted by start offset (preorder already yields that ordering).
fn collect_tokens(root: &SyntaxNode) -> Vec<(TextRange, u32)> {
    root.descendants_with_tokens()
        .filter_map(|element| {
            let (kind, range) = match &element {
                rowan::NodeOrToken::Node(node) => (node.kind(), node.text_range()),
                rowan::NodeOrToken::Token(token) => (token.kind(), token.text_range()),
            };
            token_type_for(kind).map(|token_type| (range, token_type))
        })
        .collect()
}

/// Encode `(range, type)` pairs into the LSP relative-delta wire format.
///
/// Tokens must be sorted by start offset (they are, from preorder). Cross-line
/// tokens are skipped in step 1: the classic encoding has no multi-line token,
/// and every kind we collect is single-line in practice.
fn encode(text: &str, tokens: Vec<(TextRange, u32)>) -> Vec<SemanticToken> {
    let mut data = Vec::with_capacity(tokens.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for (range, token_type) in tokens {
        let start = offset_to_position(text, range.start().into());
        let end = offset_to_position(text, range.end().into());

        // Single-line guard: defer any token that spans a line boundary.
        if start.line != end.line {
            continue;
        }
        let length = end.character.saturating_sub(start.character);
        if length == 0 {
            continue;
        }

        let delta_line = start.line - prev_line;
        let delta_start = if delta_line == 0 {
            start.character - prev_start
        } else {
            start.character
        };

        data.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: 0,
        });

        prev_line = start.line;
        prev_start = start.character;
    }

    data
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(content: &str, flavor: Flavor) -> SyntaxNode {
        let config = crate::config::Config {
            flavor,
            extensions: crate::config::Extensions::for_flavor(flavor),
            ..crate::config::Config::default()
        };
        crate::parser::parse(content, Some(config))
    }

    /// Decode the relative-delta stream back to absolute `(line, char, len, type)`
    /// tuples for readable assertions.
    fn decode(data: &[SemanticToken]) -> Vec<(u32, u32, u32, u32)> {
        let mut out = Vec::new();
        let (mut line, mut start) = (0u32, 0u32);
        for tok in data {
            if tok.delta_line == 0 {
                start += tok.delta_start;
            } else {
                line += tok.delta_line;
                start = tok.delta_start;
            }
            out.push((line, start, tok.length, tok.token_type));
        }
        out
    }

    #[test]
    fn maps_quarto_constructs_to_types() {
        // `@fig-1` is a crossref only in Quarto; citation/shortcode/footnote/
        // span-attrs/div-info/math all present.
        let content = "[@key] @fig-1 $a$ $$b$$ {{< x >}} [^1]\n\n[s]{.cls}\n\n::: {.note}\nhi\n:::\n\n[^1]: note\n";
        let root = parse(content, Flavor::Quarto);
        let types: std::collections::BTreeSet<u32> =
            collect_tokens(&root).into_iter().map(|(_, t)| t).collect();
        // citation(0), crossref(1), shortcode(2), div(3), math(4), footnote(5),
        // attribute(6) should all appear.
        assert_eq!(types, (0..=6).collect());
    }

    #[test]
    fn encodes_relative_deltas_single_line() {
        // Two citations on the same line; deltas are relative to the previous.
        let content = "see [@a] and [@bb] here\n";
        let root = parse(content, Flavor::Pandoc);
        let data = encode(content, collect_tokens(&root));
        let decoded = decode(&data);
        // `a` at col 6 len 1, `bb` at col 15 len 2 (keys only, no `[@`/`]`).
        assert_eq!(decoded, vec![(0, 6, 1, 0), (0, 15, 2, 0)]);
    }

    #[test]
    fn utf16_length_and_offsets() {
        // A multi-byte char before the citation shifts UTF-16 columns by 1 each.
        let content = "é [@kä] x\n";
        let root = parse(content, Flavor::Pandoc);
        let data = encode(content, collect_tokens(&root));
        let decoded = decode(&data);
        // "é " = 2 UTF-16 units, then "[@" = 2 → key starts at col 4; "kä" = 2 units.
        assert_eq!(decoded, vec![(0, 4, 2, 0)]);
    }

    #[test]
    fn skips_cross_line_token() {
        // A synthetic range spanning a newline must be dropped by the guard.
        let text = "ab\ncd\n";
        let range = TextRange::new(1.into(), 4.into()); // 'b\nc'
        let data = encode(text, vec![(range, 0)]);
        assert!(data.is_empty(), "cross-line token should be skipped");
    }

    #[test]
    fn crlf_line_deltas() {
        let content = "[@a]\r\n[@b]\r\n";
        let root = parse(content, Flavor::Pandoc);
        let data = encode(content, collect_tokens(&root));
        let decoded = decode(&data);
        assert_eq!(decoded, vec![(0, 2, 1, 0), (1, 2, 1, 0)]);
    }
}
