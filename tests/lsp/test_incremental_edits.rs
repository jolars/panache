//! Tests for incremental document synchronization.

use super::helpers::*;
use lsp_types::{DocumentSymbolResponse, GotoDefinitionResponse, Uri};
use serde_json::json;

#[test]
fn test_incremental_edit_simple() {
    let mut server = TestLspServer::new();

    // Open a document
    server.open_document("file:///test.qmd", "# Title\n\nOld text.", "quarto");
    // Make an incremental edit (replace "Old" with "New")
    server.edit_document(
        "file:///test.qmd",
        vec![incremental_change(2, 0, 2, 3, "New")],
    );

    // Verify the edit was applied
    let content = server.get_document_content("file:///test.qmd");
    assert_eq!(content, Some("# Title\n\nNew text.".to_string()));

    let tree_after = server
        .get_document_tree("file:///test.qmd")
        .expect("tree after edit");
    let expected = panache::parse("# Title\n\nNew text.", None);
    assert_eq!(tree_after.text_range(), expected.text_range());
    assert_eq!(tree_after.to_string(), expected.to_string());
}

#[test]
fn test_experimental_incremental_parsing_setting_defaults_to_off() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root_uri = Uri::from_file_path(temp_dir.path()).unwrap();
    let mut server = TestLspServer::new();

    server.initialize(root_uri.as_str());
    assert!(!server.experimental_incremental_parsing_enabled());
}

#[test]
fn test_experimental_incremental_parsing_setting_can_be_enabled() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root_uri = Uri::from_file_path(temp_dir.path()).unwrap();
    let mut server = TestLspServer::new();

    server.initialize_with_options(
        root_uri.as_str(),
        Some(json!({
            "settings": {
                "panache": {
                    "experimental": {
                        "incrementalParsing": true
                    }
                }
            }
        })),
    );

    assert!(server.experimental_incremental_parsing_enabled());
}

#[test]
fn test_incremental_edit_updates_dependents() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    std::fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let doc1_path = root.join("doc1.qmd");
    let doc2_path = root.join("doc2.qmd");
    std::fs::write(&doc1_path, "See [link][ref].\n").unwrap();
    std::fs::write(&doc2_path, "[ref]: https://example.com\n").unwrap();

    let doc1_uri = Uri::from_file_path(&doc1_path).unwrap();
    let doc2_uri = Uri::from_file_path(&doc2_path).unwrap();

    let mut server = TestLspServer::new();
    server.initialize(Uri::from_file_path(root).unwrap().as_str());
    server.open_document(
        doc1_uri.as_str(),
        &std::fs::read_to_string(&doc1_path).unwrap(),
        "quarto",
    );
    server.open_document(
        doc2_uri.as_str(),
        &std::fs::read_to_string(&doc2_path).unwrap(),
        "quarto",
    );

    server.edit_document(
        doc2_uri.as_str(),
        vec![full_document_change("[ref]: https://example.org\n")],
    );

    let result = server.goto_definition(doc1_uri.as_str(), 0, 12);
    let Some(GotoDefinitionResponse::Scalar(location)) = result else {
        panic!("Expected scalar location response");
    };
    assert_eq!(location.uri, doc2_uri);
}

#[test]
fn test_incremental_edit_multiple_changes_single_notification() {
    let mut server = TestLspServer::new();

    // Open a document
    server.open_document("file:///test.qmd", "Line 1\nLine 2\nLine 3", "quarto");
    // Apply multiple changes in a single notification
    server.edit_document(
        "file:///test.qmd",
        vec![
            incremental_change(0, 0, 0, 0, "Inserted line 1\nInserted line 2\n"),
            incremental_change(4, 0, 4, 6, "Line 3 updated"),
        ],
    );

    let content = server.get_document_content("file:///test.qmd");
    assert_eq!(
        content,
        Some("Inserted line 1\nInserted line 2\nLine 1\nLine 2\nLine 3 updated".to_string())
    );

    let tree_after = server
        .get_document_tree("file:///test.qmd")
        .expect("tree after edit");
    let expected = panache::parse(
        "Inserted line 1\nInserted line 2\nLine 1\nLine 2\nLine 3 updated",
        None,
    );
    assert_eq!(tree_after.text_range(), expected.text_range());
    assert_eq!(tree_after.to_string(), expected.to_string());
}

#[test]
fn test_incremental_edit_multiline() {
    let mut server = TestLspServer::new();

    // Open a document
    server.open_document(
        "file:///test.qmd",
        "Line 1\nLine 2\nLine 3\nLine 4",
        "quarto",
    );

    // Delete lines 2-3
    server.edit_document("file:///test.qmd", vec![incremental_change(1, 0, 3, 0, "")]);

    // Verify the edit
    let content = server.get_document_content("file:///test.qmd");
    assert_eq!(content, Some("Line 1\nLine 4".to_string()));
}

#[test]
fn test_multiple_documents() {
    let mut server = TestLspServer::new();

    // Open two documents
    server.open_document("file:///doc1.qmd", "# Doc 1", "quarto");
    server.open_document("file:///doc2.qmd", "# Doc 2", "quarto");

    // Edit both
    server.edit_document(
        "file:///doc1.qmd",
        vec![full_document_change("# Modified 1")],
    );
    server.edit_document(
        "file:///doc2.qmd",
        vec![full_document_change("# Modified 2")],
    );

    // Verify both were updated independently
    assert_eq!(
        server.get_document_content("file:///doc1.qmd"),
        Some("# Modified 1".to_string())
    );
    assert_eq!(
        server.get_document_content("file:///doc2.qmd"),
        Some("# Modified 2".to_string())
    );
}

#[test]
fn test_incremental_edit_preserves_yaml_frontmatter_document_symbol() {
    let mut server = TestLspServer::new();
    let content = "---\ntitle: Old\n---\n\n# Heading\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let before = server
        .get_symbols("file:///test.qmd")
        .expect("symbols before edit");
    let DocumentSymbolResponse::Nested(before_symbols) = before else {
        panic!("Expected nested symbols");
    };
    assert!(
        before_symbols
            .iter()
            .any(|symbol| symbol.name == "YAML Frontmatter"),
        "Expected YAML frontmatter symbol before edit"
    );

    server.edit_document(
        "file:///test.qmd",
        vec![incremental_change(1, 7, 1, 10, "Updated Title")],
    );

    let after = server
        .get_symbols("file:///test.qmd")
        .expect("symbols after edit");
    let DocumentSymbolResponse::Nested(after_symbols) = after else {
        panic!("Expected nested symbols");
    };
    let yaml_symbol = after_symbols
        .iter()
        .find(|symbol| symbol.name == "YAML Frontmatter")
        .expect("yaml symbol after edit");
    assert_eq!(yaml_symbol.range.start.line, 0);
    assert!(
        yaml_symbol.range.end.line >= 2,
        "frontmatter symbol should still span the YAML block"
    );
}

#[test]
fn test_incremental_edit_multiple_changes_with_utf16_positions() {
    let mut server = TestLspServer::new();

    server.open_document("file:///utf16.qmd", "Title\nemoji: 😀\nRésumé\n", "quarto");

    server.edit_document(
        "file:///utf16.qmd",
        vec![
            incremental_change(1, 7, 1, 9, "😎"),
            incremental_change(2, 1, 2, 2, "e"),
        ],
    );

    let content = server.get_document_content("file:///utf16.qmd");
    assert_eq!(content, Some("Title\nemoji: 😎\nResumé\n".to_string()));

    let tree_after = server
        .get_document_tree("file:///utf16.qmd")
        .expect("tree after utf16 edit");
    let expected = panache::parse("Title\nemoji: 😎\nResumé\n", None);
    assert_eq!(tree_after.to_string(), expected.to_string());
}

#[test]
fn test_incremental_edit_multiple_changes_use_full_reparse() {
    let mut server = TestLspServer::new();

    server.open_document("file:///cap.qmd", "aaaaaaaaaa\n", "quarto");

    server.edit_document(
        "file:///cap.qmd",
        vec![
            incremental_change(0, 0, 0, 1, "b"),
            incremental_change(0, 1, 0, 2, "c"),
            incremental_change(0, 2, 0, 3, "d"),
            incremental_change(0, 3, 0, 4, "e"),
            incremental_change(0, 4, 0, 5, "f"),
            incremental_change(0, 5, 0, 6, "g"),
            incremental_change(0, 6, 0, 7, "h"),
            incremental_change(0, 7, 0, 8, "i"),
            incremental_change(0, 8, 0, 9, "j"),
        ],
    );

    let content = server.get_document_content("file:///cap.qmd");
    assert_eq!(content, Some("bcdefghija\n".to_string()));

    let tree_after = server
        .get_document_tree("file:///cap.qmd")
        .expect("tree after cap edit");
    let expected = panache::parse("bcdefghija\n", None);
    assert_eq!(tree_after.to_string(), expected.to_string());
}

#[test]
fn test_incremental_edit_multiple_changes_descending_coalesces_experimental() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root_uri = Uri::from_file_path(temp_dir.path()).unwrap();
    let mut server = TestLspServer::new();

    server.initialize_with_options(
        root_uri.as_str(),
        Some(json!({
            "settings": {
                "panache": {
                    "experimental": {
                        "incrementalParsing": true
                    }
                }
            }
        })),
    );

    server.open_document("file:///coalesce.qmd", "abcdef\n", "quarto");

    server.edit_document(
        "file:///coalesce.qmd",
        vec![
            incremental_change(0, 3, 0, 4, "X"),
            incremental_change(0, 1, 0, 2, "Y"),
        ],
    );

    let content = server.get_document_content("file:///coalesce.qmd");
    assert_eq!(content, Some("aYcXef\n".to_string()));

    let tree_after = server
        .get_document_tree("file:///coalesce.qmd")
        .expect("tree after coalesced multi edit");
    let expected = panache::parse("aYcXef\n", None);
    assert_eq!(tree_after.to_string(), expected.to_string());
}

#[test]
fn test_incremental_edit_setext_heading_transition_matches_full_parse() {
    let mut server = TestLspServer::new();

    server.open_document("file:///setext.qmd", "Intro\nSecond\n\nTail\n", "quarto");

    server.edit_document(
        "file:///setext.qmd",
        vec![incremental_change(1, 6, 1, 6, "\n-----")],
    );

    let content = server.get_document_content("file:///setext.qmd");
    assert_eq!(content, Some("Intro\nSecond\n-----\n\nTail\n".to_string()));

    let tree_after = server
        .get_document_tree("file:///setext.qmd")
        .expect("tree after setext edit");
    let expected = panache::parse("Intro\nSecond\n-----\n\nTail\n", None);
    assert_eq!(tree_after.to_string(), expected.to_string());
}

#[test]
fn test_incremental_edit_lazy_blockquote_transition_matches_full_parse() {
    let mut server = TestLspServer::new();

    server.open_document(
        "file:///blockquote.qmd",
        "> quoted\nlazy\n\nnext\n",
        "quarto",
    );

    server.edit_document(
        "file:///blockquote.qmd",
        vec![incremental_change(1, 0, 1, 4, "> line")],
    );

    let content = server.get_document_content("file:///blockquote.qmd");
    assert_eq!(content, Some("> quoted\n> line\n\nnext\n".to_string()));

    let tree_after = server
        .get_document_tree("file:///blockquote.qmd")
        .expect("tree after blockquote edit");
    let expected = panache::parse("> quoted\n> line\n\nnext\n", None);
    assert_eq!(tree_after.to_string(), expected.to_string());
}

#[test]
fn test_incremental_edit_frontmatter_delimiter_with_experimental_mode() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root_uri = Uri::from_file_path(temp_dir.path()).unwrap();
    let mut server = TestLspServer::new();

    server.initialize_with_options(
        root_uri.as_str(),
        Some(json!({
            "settings": {
                "panache": {
                    "experimental": {
                        "incrementalParsing": true
                    }
                }
            }
        })),
    );
    assert!(server.experimental_incremental_parsing_enabled());

    server.open_document(
        "file:///frontmatter.qmd",
        "---\ntitle: Demo\n---\n\n# Intro\n\nalpha\n",
        "quarto",
    );

    server.edit_document(
        "file:///frontmatter.qmd",
        vec![incremental_change(0, 0, 0, 3, "----")],
    );

    let expected_text = "----\ntitle: Demo\n---\n\n# Intro\n\nalpha\n";
    let content = server.get_document_content("file:///frontmatter.qmd");
    assert_eq!(content, Some(expected_text.to_string()));

    let tree_after = server
        .get_document_tree("file:///frontmatter.qmd")
        .expect("tree after frontmatter delimiter edit");
    let expected = panache::parse(expected_text, None);
    assert_eq!(tree_after.to_string(), expected.to_string());
}

#[test]
fn test_incremental_edit_deleting_heading_boundary_with_experimental_mode() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root_uri = Uri::from_file_path(temp_dir.path()).unwrap();
    let mut server = TestLspServer::new();

    server.initialize_with_options(
        root_uri.as_str(),
        Some(json!({
            "settings": {
                "panache": {
                    "experimental": {
                        "incrementalParsing": true
                    }
                }
            }
        })),
    );
    assert!(server.experimental_incremental_parsing_enabled());

    let initial = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\n# End\n\nomega\n";
    server.open_document("file:///headings.qmd", initial, "quarto");

    server.edit_document(
        "file:///headings.qmd",
        vec![incremental_change(8, 0, 10, 0, "")],
    );

    let expected_text = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\nomega\n";
    let content = server.get_document_content("file:///headings.qmd");
    assert_eq!(content, Some(expected_text.to_string()));

    let tree_after = server
        .get_document_tree("file:///headings.qmd")
        .expect("tree after heading boundary deletion");
    let expected = panache::parse(expected_text, None);
    assert_eq!(tree_after.to_string(), expected.to_string());
}
