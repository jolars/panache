use super::helpers::*;
use lsp_types::Range;

/// Collect linked-editing ranges as sorted `(line, start_char, end_char)`
/// tuples for order-independent assertions.
fn spans(ranges: &[Range]) -> Vec<(u32, u32, u32)> {
    let mut out: Vec<(u32, u32, u32)> = ranges
        .iter()
        .map(|r| (r.start.line, r.start.character, r.end.character))
        .collect();
    out.sort();
    out
}

#[test]
fn full_reference_links_label_and_definition() {
    let mut server = TestLspServer::new();
    let content = "[text][label] here.\n\n[label]: https://example.com\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let expected = vec![(0, 7, 12), (2, 1, 6)];

    // Cursor inside the usage label.
    let from_usage = server
        .linked_editing_range("file:///test.qmd", 0, 9)
        .expect("linked editing from usage");
    assert_eq!(spans(&from_usage.ranges), expected);

    // Cursor inside the definition label resolves to the same set.
    let from_def = server
        .linked_editing_range("file:///test.qmd", 2, 3)
        .expect("linked editing from definition");
    assert_eq!(spans(&from_def.ranges), expected);
}

#[test]
fn footnote_reference_and_definition() {
    let mut server = TestLspServer::new();
    let content = "See[^note].\n\n[^note]: A note.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let expected = vec![(0, 5, 9), (2, 2, 6)];

    let from_usage = server
        .linked_editing_range("file:///test.qmd", 0, 7)
        .expect("linked editing from footnote reference");
    assert_eq!(spans(&from_usage.ranges), expected);

    let from_def = server
        .linked_editing_range("file:///test.qmd", 2, 4)
        .expect("linked editing from footnote definition");
    assert_eq!(spans(&from_def.ranges), expected);
}

#[test]
fn citation_key_used_multiple_times() {
    let mut server = TestLspServer::new();
    let content = "See @doe2020 and @doe2020 again.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let result = server
        .linked_editing_range("file:///test.qmd", 0, 7)
        .expect("linked editing for citation");
    assert_eq!(spans(&result.ranges), vec![(0, 5, 12), (0, 18, 25)]);
}

#[test]
fn heading_id_and_explicit_link() {
    let mut server = TestLspServer::new();
    let content = "# Heading {#sec}\n\nSee [text](#sec).\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let expected = vec![(0, 12, 15), (2, 12, 15)];

    let from_link = server
        .linked_editing_range("file:///test.qmd", 2, 13)
        .expect("linked editing from heading link");
    assert_eq!(spans(&from_link.ranges), expected);

    let from_id = server
        .linked_editing_range("file:///test.qmd", 0, 13)
        .expect("linked editing from heading id");
    assert_eq!(spans(&from_id.ranges), expected);
}

#[test]
fn case_variant_definition_is_excluded() {
    let mut server = TestLspServer::new();
    // Two identically-cased usages share a span set; the differently-cased
    // definition (`[foo]:`) must be dropped — the protocol requires every
    // returned range to contain identical text.
    let content = "[a][Foo] and [b][Foo].\n\n[foo]: https://example.com\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let result = server
        .linked_editing_range("file:///test.qmd", 0, 5)
        .expect("linked editing for identically-cased usages");
    let spans = spans(&result.ranges);
    assert_eq!(spans, vec![(0, 4, 7), (0, 17, 20)]);
    assert!(
        spans.iter().all(|(line, _, _)| *line == 0),
        "case-variant definition on line 2 must be excluded"
    );
}

#[test]
fn no_linked_ranges_in_plain_prose() {
    let mut server = TestLspServer::new();
    let content = "Just plain prose here.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    assert!(
        server
            .linked_editing_range("file:///test.qmd", 0, 6)
            .is_none(),
        "plain prose has no linked editing ranges"
    );
}

#[test]
fn no_linked_ranges_in_yaml_frontmatter() {
    let mut server = TestLspServer::new();
    let content = "---\ntitle: Hello\n---\n\nBody text.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    assert!(
        server
            .linked_editing_range("file:///test.qmd", 1, 2)
            .is_none(),
        "YAML frontmatter has no linked editing ranges"
    );
}
