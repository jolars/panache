//! `textDocument/semanticTokens/full`: additive, flavor-gated highlighting of
//! pandoc/quarto-specific constructs.

use super::helpers::*;
use lsp_types::{
    SemanticToken, SemanticTokensFullOptions, SemanticTokensResult,
    SemanticTokensServerCapabilities, Uri,
};
use std::fs;
use tempfile::TempDir;

/// The server advertises a semantic-tokens provider with the custom legend,
/// `full` enabled, and `range` disabled (step 1 scope).
#[test]
fn advertises_semantic_tokens_capability() {
    let mut server = TestLspServer::new();
    let result = server.initialize_result("file:///ws");
    let provider = result
        .capabilities
        .semantic_tokens_provider
        .expect("semantic tokens provider advertised");
    let options = match provider {
        SemanticTokensServerCapabilities::SemanticTokensOptions(options) => options,
        SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(_) => {
            panic!("expected inline options, not registration options")
        }
    };
    assert_eq!(options.range, Some(false));
    assert!(matches!(
        options.full,
        Some(SemanticTokensFullOptions::Bool(true))
    ));
    let types: Vec<String> = options
        .legend
        .token_types
        .iter()
        .map(|t| t.as_str().to_string())
        .collect();
    assert_eq!(
        types,
        [
            "citation",
            "crossref",
            "shortcode",
            "div",
            "math",
            "footnote",
            "attribute"
        ]
    );
    assert!(
        options.legend.token_modifiers.is_empty(),
        "no modifiers in step 1"
    );
}

/// Decode the LSP relative-delta stream to absolute `(line, char, len, type)`.
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

fn data_of(result: Option<SemanticTokensResult>) -> Vec<SemanticToken> {
    match result.expect("semantic tokens result") {
        SemanticTokensResult::Tokens(tokens) => tokens.data,
        SemanticTokensResult::Partial(_) => panic!("unexpected partial result"),
    }
}

/// A Quarto document emits tokens for the pandoc/quarto constructs the editor's
/// base grammar misses. Crossref (type 1) is the discriminator: `@fig-plot`
/// tokenizes as a cross-reference only under Quarto/RMarkdown flavors.
#[test]
fn quarto_document_emits_differentiator_tokens() {
    let mut server = TestLspServer::new();
    let content = "\
See [@knuth1984] and @fig-plot.

Inline $E=mc^2$ and

$$a^2+b^2$$

A {{< video x >}} shortcode and a note[^n].

[^n]: the note.
";
    server.open_document("file:///doc.qmd", content, "quarto");

    let data = data_of(server.semantic_tokens_full("file:///doc.qmd"));
    assert!(!data.is_empty(), "expected tokens for a Quarto document");

    let types: std::collections::BTreeSet<u32> =
        decode(&data).into_iter().map(|(_, _, _, t)| t).collect();
    // citation(0), crossref(1), shortcode(2), math(4), footnote(5).
    for expected in [0u32, 1, 2, 4, 5] {
        assert!(
            types.contains(&expected),
            "expected token type {expected} present, got {types:?}"
        );
    }

    // The citation key `knuth1984` sits on line 0 starting at UTF-16 col 5
    // (after `See [@`), length 9 — brackets/`@` are left to the base grammar.
    let decoded = decode(&data);
    assert!(
        decoded.contains(&(0, 6, 9, 0)),
        "expected citation key token at (0,6,9,0), got {decoded:?}"
    );
}

/// Flavor gate: a GFM document gets an empty (not absent) token set — panache's
/// parse and the editor's base grammar agree there, so there is nothing to add.
#[test]
fn gfm_document_is_flavor_gated_to_empty() {
    let mut server = TestLspServer::new();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join("panache.toml"), "flavor = \"gfm\"\n").unwrap();

    let doc_path = root.join("doc.md");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");
    server.initialize(root_uri.as_str());

    // Constructs that would tokenize under Pandoc/Quarto; under GFM they must not.
    let content = "See [@knuth1984] and a note[^n].\n\n[^n]: the note.\n";
    server.open_document(doc_uri.as_str(), content, "markdown");

    let data = data_of(server.semantic_tokens_full(doc_uri.as_str()));
    assert!(
        data.is_empty(),
        "expected no tokens under GFM flavor, got {:?}",
        decode(&data)
    );
}
