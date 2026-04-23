//! Tests for completion (citation completion).

use super::helpers::*;
use std::fs;
use tempfile::TempDir;
use tower_lsp_server::ls_types::CompletionResponse;
use tower_lsp_server::ls_types::Uri;

#[tokio::test]
async fn test_completion_without_citation_context() {
    let server = TestLspServer::new();

    // Open a document without citation context
    let content = "Just plain text.";
    server
        .open_document("file:///test.md", content, "markdown")
        .await;

    // Request completion in plain text
    let result = server.completion("file:///test.md", 0, 5).await;

    // Should return None when not in citation context
    assert!(
        result.is_none(),
        "Should not provide completions outside citation context"
    );
}

#[tokio::test]
async fn test_completion_in_citation_without_bibliography() {
    let server = TestLspServer::new();

    // Open a document with citation syntax but no bibliography configured
    let content = "Text with [@] citation.";
    server
        .open_document("file:///test.md", content, "markdown")
        .await;

    // Request completion at @ position
    let result = server
        .completion(
            "file:///test.md",
            0,
            12, // Position after [@
        )
        .await;

    // Should return None when no bibliography is configured
    assert!(
        result.is_none(),
        "Should not provide completions without bibliography"
    );
}

#[tokio::test]
async fn test_completion_with_project_bibliography() {
    let server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    fs::write(root.join("_quarto.yml"), "bibliography: refs.bib\n").unwrap();
    fs::write(root.join("refs.bib"), "@book{known,}\n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str()).await;

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(doc_path).expect("doc uri");
    let content = "Text [@] citation.";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    let result = server.completion(doc_uri.as_str(), 0, 7).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "known"),
        "Expected bibliography key completion"
    );
}

#[tokio::test]
async fn test_completion_preserves_bibliography_key_case() {
    let server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    fs::write(root.join("_quarto.yml"), "bibliography: refs.bib\n").unwrap();
    fs::write(root.join("refs.bib"), "@article{Eddelbuettel:2011,}\n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str()).await;

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(doc_path).expect("doc uri");
    let content = "Text [@] citation.";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    let result = server.completion(doc_uri.as_str(), 0, 7).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "Eddelbuettel:2011"
            && item.insert_text.as_deref() == Some("Eddelbuettel:2011")),
        "Expected completion to preserve original bibliography key casing"
    );
}

#[tokio::test]
async fn test_completion_with_inline_references() {
    let server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str()).await;

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\nreferences:\n  - id: inline\n    title: Inline\n---\n\nText [@] citation.";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    let result = server.completion(doc_uri.as_str(), 6, 7).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "inline"),
        "Expected inline reference completion"
    );
}

#[tokio::test]
async fn test_completion_with_csl_yaml_bibliography() {
    let server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("refs.yaml"), "- id: cslkey\n  title: Sample\n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str()).await;

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\nbibliography: refs.yaml\n---\n\nText [@] citation.";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    let result = server.completion(doc_uri.as_str(), 4, 7).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "cslkey"),
        "Expected CSL YAML bibliography completion"
    );
}

#[tokio::test]
async fn test_completion_with_csl_json_bibliography() {
    let server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(
        root.join("refs.json"),
        "[{\"id\":\"cslkey\",\"title\":\"Sample\"}]",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str()).await;

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\nbibliography: refs.json\n---\n\nText [@] citation.";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    let result = server.completion(doc_uri.as_str(), 4, 7).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "cslkey"),
        "Expected CSL JSON bibliography completion"
    );
}

#[tokio::test]
async fn test_completion_with_ris_bibliography() {
    let server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("refs.ris"), "TY  - JOUR\nID  - riskey\nER  - \n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str()).await;

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\nbibliography: refs.ris\n---\n\nText [@] citation.";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    let result = server.completion(doc_uri.as_str(), 4, 7).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "riskey"),
        "Expected RIS bibliography completion"
    );
}

#[tokio::test]
async fn test_completion_returns_none_for_invalid_yaml_frontmatter() {
    let server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("refs.yaml"), "- id: cslkey\n  title: Sample\n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str()).await;

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\nbibliography: [\n---\n\nText [@] citation.";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    let result = server.completion(doc_uri.as_str(), 4, 7).await;
    assert!(
        result.is_none(),
        "Expected no completion when YAML frontmatter is invalid"
    );
}

#[tokio::test]
async fn test_completion_returns_none_inside_yaml_frontmatter() {
    let server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    std::fs::write(root.join("refs.bib"), "@book{known,}\n").unwrap();

    let root_uri = Uri::from_file_path(root).expect("temp dir should be absolute");
    server.initialize(root_uri.as_str()).await;

    let doc_path = root.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let content = "---\ntitle: \"@\"\nbibliography: refs.bib\n---\n\nText [@] citation.";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    let result = server.completion(doc_uri.as_str(), 1, 9).await;
    assert!(
        result.is_none(),
        "Expected no citation completion when cursor is inside YAML frontmatter"
    );
}

#[tokio::test]
async fn test_completion_includes_only_crossrefable_chunk_labels() {
    let server = TestLspServer::new();

    let content = "```{r}\n#| label: setup\n1 + 1\n```\n\n```{r}\n#| label: fig-plot\n#| fig-cap: \"Plot\"\nplot(1:10)\n```\n\nSee @\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let result = server.completion("file:///test.qmd", 11, 6).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("Expected completion items");
    };

    assert!(
        items.iter().any(|item| item.label == "fig-plot"),
        "Expected Quarto figure crossref label completion"
    );
    assert!(
        !items.iter().any(|item| item.label == "setup"),
        "Expected non-crossrefable chunk labels to be excluded"
    );
}
