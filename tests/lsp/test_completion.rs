//! Tests for completion (citation completion).

use super::helpers::*;
use std::fs;
use tempfile::TempDir;
use tower_lsp_server::ls_types::{CompletionItemKind, CompletionResponse, Uri};

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

// --- Path completion in `![](…)` and `[](…)` destinations ---

fn open_doc_with_files(_server: &TestLspServer, files: &[(&str, &str)]) -> (TempDir, Uri) {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    for (rel, contents) in files {
        let abs = root.join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(abs, contents).unwrap();
    }
    let doc_uri = Uri::from_file_path(root.join("doc.md")).expect("doc uri");
    (temp_dir, doc_uri)
}

#[tokio::test]
async fn test_image_path_completion_lists_image_files_only() {
    let server = TestLspServer::new();
    let (_tmp, doc_uri) = open_doc_with_files(
        &server,
        &[
            ("images/foo.png", ""),
            ("images/bar.jpg", ""),
            ("images/notes.txt", ""),
        ],
    );

    let content = "![](images/)\n";
    server
        .open_document(doc_uri.as_str(), content, "markdown")
        .await;

    // Cursor between `images/` and `)`: line 0, char 11.
    let result = server.completion(doc_uri.as_str(), 0, 11).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "foo.png"), "labels: {labels:?}");
    assert!(labels.iter().any(|l| l == "bar.jpg"), "labels: {labels:?}");
    assert!(
        !labels.iter().any(|l| l == "notes.txt"),
        "txt files should be excluded in image context: {labels:?}"
    );
}

#[tokio::test]
async fn test_image_path_completion_includes_video_files() {
    let server = TestLspServer::new();
    let (_tmp, doc_uri) = open_doc_with_files(
        &server,
        &[
            ("media/clip.mp4", ""),
            ("media/clip.webm", ""),
            ("media/notes.txt", ""),
        ],
    );

    let content = "![](media/)\n";
    server
        .open_document(doc_uri.as_str(), content, "markdown")
        .await;

    // Cursor between `media/` and `)`: line 0, char 10.
    let result = server.completion(doc_uri.as_str(), 0, 10).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "clip.mp4"), "labels: {labels:?}");
    assert!(
        labels.iter().any(|l| l == "clip.webm"),
        "labels: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l == "notes.txt"),
        "txt files should be excluded in image context: {labels:?}"
    );
}

#[tokio::test]
async fn test_image_path_completion_includes_subdirectory() {
    let server = TestLspServer::new();
    let (_tmp, doc_uri) = open_doc_with_files(&server, &[("images/nested/keep.png", "")]);

    let content = "![](images/)\n";
    server
        .open_document(doc_uri.as_str(), content, "markdown")
        .await;

    let result = server.completion(doc_uri.as_str(), 0, 11).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let folder = items
        .iter()
        .find(|i| i.label == "nested/")
        .expect("nested/ directory");
    assert_eq!(folder.kind, Some(CompletionItemKind::FOLDER));
}

#[tokio::test]
async fn test_image_path_completion_filters_by_typed_prefix() {
    let server = TestLspServer::new();
    let (_tmp, doc_uri) =
        open_doc_with_files(&server, &[("images/foo.png", ""), ("images/bar.png", "")]);

    let content = "![](images/f)\n";
    server
        .open_document(doc_uri.as_str(), content, "markdown")
        .await;

    // Cursor between `f` and `)`: line 0, char 12.
    let result = server.completion(doc_uri.as_str(), 0, 12).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "foo.png"), "labels: {labels:?}");
    assert!(
        !labels.iter().any(|l| l == "bar.png"),
        "prefix `f` must exclude bar.png: {labels:?}"
    );
}

#[tokio::test]
async fn test_link_path_completion_includes_all_files() {
    let server = TestLspServer::new();
    let (_tmp, doc_uri) =
        open_doc_with_files(&server, &[("docs/intro.md", ""), ("docs/notes.txt", "")]);

    let content = "[see](docs/)\n";
    server
        .open_document(doc_uri.as_str(), content, "markdown")
        .await;

    // Cursor between `docs/` and `)`: line 0, char 11.
    let result = server.completion(doc_uri.as_str(), 0, 11).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "intro.md"), "labels: {labels:?}");
    assert!(
        labels.iter().any(|l| l == "notes.txt"),
        "labels: {labels:?}"
    );
}

#[tokio::test]
async fn test_no_path_completion_inside_image_alt_text() {
    let server = TestLspServer::new();
    let (_tmp, doc_uri) = open_doc_with_files(&server, &[("images/foo.png", "")]);

    let content = "![images/](images/)\n";
    server
        .open_document(doc_uri.as_str(), content, "markdown")
        .await;

    // Cursor inside the alt text `![images/]`, between `/` and `]`: line 0, char 9.
    let result = server.completion(doc_uri.as_str(), 0, 9).await;
    assert!(
        result.is_none(),
        "alt-text region must not trigger path completion"
    );
}

#[tokio::test]
async fn test_completion_capability_registers_path_trigger_characters() {
    let server = TestLspServer::new();
    let temp_dir = TempDir::new().unwrap();
    let root_uri = Uri::from_file_path(temp_dir.path()).expect("temp dir absolute");
    let init = server.initialize_result(root_uri.as_str()).await;
    let triggers = init
        .capabilities
        .completion_provider
        .expect("completion provider")
        .trigger_characters
        .expect("trigger_characters");
    assert!(triggers.iter().any(|t| t == "/"), "triggers: {triggers:?}");
    assert!(triggers.iter().any(|t| t == "("), "triggers: {triggers:?}");
}
