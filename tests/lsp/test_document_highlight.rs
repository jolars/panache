use super::helpers::*;
use lsp_types::DocumentHighlight;

/// Collect highlight ranges as sorted `(line, start_char, end_char)` tuples for
/// order-independent assertions.
fn spans(highlights: &[DocumentHighlight]) -> Vec<(u32, u32, u32)> {
    let mut out: Vec<(u32, u32, u32)> = highlights
        .iter()
        .map(|h| {
            (
                h.range.start.line,
                h.range.start.character,
                h.range.end.character,
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn full_reference_link_usage_and_definition() {
    let mut server = TestLspServer::new();
    let content = "[text][label] here.\n\n[label]: https://example.com\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let expected = vec![(0, 7, 12), (2, 1, 6)];

    // Highlighting from the usage label and from the definition agree.
    let from_usage = server
        .document_highlight("file:///test.qmd", 0, 9)
        .expect("highlight from usage");
    assert_eq!(spans(&from_usage), expected);

    let from_def = server
        .document_highlight("file:///test.qmd", 2, 3)
        .expect("highlight from definition");
    assert_eq!(spans(&from_def), expected);
}

#[test]
fn footnote_reference_and_definition() {
    let mut server = TestLspServer::new();
    let content = "See[^note].\n\n[^note]: A note.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let expected = vec![(0, 5, 9), (2, 2, 6)];

    let from_usage = server
        .document_highlight("file:///test.qmd", 0, 7)
        .expect("highlight from footnote reference");
    assert_eq!(spans(&from_usage), expected);

    let from_def = server
        .document_highlight("file:///test.qmd", 2, 4)
        .expect("highlight from footnote definition");
    assert_eq!(spans(&from_def), expected);
}

#[test]
fn citation_key_used_multiple_times() {
    let mut server = TestLspServer::new();
    let content = "See @doe2020 and @doe2020 again.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let result = server
        .document_highlight("file:///test.qmd", 0, 7)
        .expect("highlight for citation");
    assert_eq!(spans(&result), vec![(0, 5, 12), (0, 18, 25)]);
}

#[test]
fn heading_id_and_explicit_link() {
    let mut server = TestLspServer::new();
    let content = "# Heading {#sec}\n\nSee [text](#sec).\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let expected = vec![(0, 12, 15), (2, 12, 15)];

    let from_link = server
        .document_highlight("file:///test.qmd", 2, 13)
        .expect("highlight from heading link");
    assert_eq!(spans(&from_link), expected);

    let from_id = server
        .document_highlight("file:///test.qmd", 0, 13)
        .expect("highlight from heading id");
    assert_eq!(spans(&from_id), expected);
}

#[test]
fn case_variant_definition_is_included() {
    let mut server = TestLspServer::new();
    // Unlike linked editing (which requires identical source text), document
    // highlight marks every occurrence of the normalized symbol, including the
    // differently-cased definition `[foo]:`.
    let content = "[a][Foo] and [b][Foo].\n\n[foo]: https://example.com\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let result = server
        .document_highlight("file:///test.qmd", 0, 5)
        .expect("highlight for reference usages");
    assert_eq!(spans(&result), vec![(0, 4, 7), (0, 17, 20), (2, 1, 4)]);
}

#[test]
fn single_occurrence_still_highlights() {
    let mut server = TestLspServer::new();
    // A lone citation has no partner, but a single highlight is still valid
    // (linked editing would return None here).
    let content = "See @solo2021 only.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let result = server
        .document_highlight("file:///test.qmd", 0, 7)
        .expect("highlight for lone citation");
    assert_eq!(spans(&result), vec![(0, 5, 13)]);
}

#[test]
fn plain_prose_yields_no_highlight() {
    let mut server = TestLspServer::new();
    let content = "Just some ordinary prose here.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    assert!(
        server
            .document_highlight("file:///test.qmd", 0, 5)
            .is_none(),
        "cursor on plain prose should not highlight anything"
    );
}
