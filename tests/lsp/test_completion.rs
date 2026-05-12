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
    assert!(triggers.iter().any(|t| t == "<"), "triggers: {triggers:?}");
}

// --- Path completion inside Quarto shortcodes ---

fn open_quarto_doc_with_files(
    _server: &TestLspServer,
    files: &[(&str, &str)],
    doc_rel: &str,
) -> (TempDir, Uri) {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    for (rel, contents) in files {
        let abs = root.join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(abs, contents).unwrap();
    }
    let doc_uri = Uri::from_file_path(root.join(doc_rel)).expect("doc uri");
    (temp_dir, doc_uri)
}

#[tokio::test]
async fn test_shortcode_include_completes_quarto_files() {
    let server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(
        &server,
        &[("_intro.qmd", ""), ("_setup.R", ""), ("scratch.txt", "")],
        "doc.qmd",
    );
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str()).await;

    let content = "{{< include _ >}}\n";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    // Cursor between `_` and ` `: line 0, char 13.
    let result = server.completion(doc_uri.as_str(), 0, 13).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(
        labels.iter().any(|l| l == "_intro.qmd"),
        "labels: {labels:?}"
    );
    assert!(labels.iter().any(|l| l == "_setup.R"), "labels: {labels:?}");
    assert!(
        !labels.iter().any(|l| l == "scratch.txt"),
        ".txt should be filtered out for include: {labels:?}"
    );
}

#[tokio::test]
async fn test_shortcode_include_resolves_absolute_path_against_workspace_root() {
    let server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(
        &server,
        &[("chapters/_intro.qmd", ""), ("subdir/doc.qmd", "")],
        "subdir/doc.qmd",
    );
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str()).await;

    let content = "{{< include /chapters/ >}}\n";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    // Cursor between `/chapters/` and ` `: line 0, char 22.
    let result = server.completion(doc_uri.as_str(), 0, 22).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(
        labels.iter().any(|l| l == "_intro.qmd"),
        "expected workspace-rooted match: {labels:?}"
    );
}

#[tokio::test]
async fn test_shortcode_embed_filters_to_notebooks() {
    let server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(
        &server,
        &[("nb.ipynb", ""), ("sibling.qmd", ""), ("notes.md", "")],
        "doc.qmd",
    );
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str()).await;

    let content = "{{< embed  >}}\n";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    // Cursor between `embed ` and ` >}}`: line 0, char 11.
    let result = server.completion(doc_uri.as_str(), 0, 11).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "nb.ipynb"), "labels: {labels:?}");
    assert!(
        labels.iter().any(|l| l == "sibling.qmd"),
        "labels: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l == "notes.md"),
        ".md should be filtered out for embed: {labels:?}"
    );
}

#[tokio::test]
async fn test_shortcode_embed_returns_none_after_hash() {
    let server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(&server, &[("nb.ipynb", "")], "doc.qmd");
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str()).await;

    let content = "{{< embed nb.ipynb# >}}\n";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    // Cursor after `#`: line 0, char 19.
    let result = server.completion(doc_uri.as_str(), 0, 19).await;
    assert!(
        result.is_none(),
        "cell-id completion is out of scope for v1"
    );
}

#[tokio::test]
async fn test_shortcode_video_filters_to_video_extensions() {
    let server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(
        &server,
        &[("clip.mp4", ""), ("clip.webm", ""), ("thumb.png", "")],
        "doc.qmd",
    );
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str()).await;

    let content = "{{< video  >}}\n";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    // Cursor between `video ` and ` >}}`: line 0, char 11.
    let result = server.completion(doc_uri.as_str(), 0, 11).await;
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
        !labels.iter().any(|l| l == "thumb.png"),
        "images should be filtered out for video: {labels:?}"
    );
}

#[tokio::test]
async fn test_shortcode_video_returns_none_for_url_prefix() {
    let server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(&server, &[("clip.mp4", "")], "doc.qmd");
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str()).await;

    let content = "{{< video https:// >}}\n";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    // Cursor after `https://`: line 0, char 18.
    let result = server.completion(doc_uri.as_str(), 0, 18).await;
    assert!(
        result.is_none(),
        "URL prefixes should not produce filesystem suggestions"
    );
}

#[tokio::test]
async fn test_shortcode_placeholder_filters_to_images() {
    let server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(
        &server,
        &[("pic.png", ""), ("vector.svg", ""), ("clip.mp4", "")],
        "doc.qmd",
    );
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str()).await;

    let content = "{{< placeholder  >}}\n";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    // Cursor between `placeholder ` and ` >}}`: line 0, char 17.
    let result = server.completion(doc_uri.as_str(), 0, 17).await;
    let Some(CompletionResponse::Array(items)) = result else {
        panic!("expected completion items");
    };
    let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "pic.png"), "labels: {labels:?}");
    assert!(
        labels.iter().any(|l| l == "vector.svg"),
        "labels: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l == "clip.mp4"),
        "video files should be filtered out for placeholder: {labels:?}"
    );
}

#[tokio::test]
async fn test_shortcode_unknown_name_returns_none() {
    let server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(&server, &[("a.qmd", "")], "doc.qmd");
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str()).await;

    let content = "{{< lipsum  >}}\n";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    // Cursor between `lipsum ` and ` >}}`: line 0, char 12.
    let result = server.completion(doc_uri.as_str(), 0, 12).await;
    assert!(
        result.is_none(),
        "unknown shortcodes should not trigger path completion"
    );
}

#[tokio::test]
async fn test_shortcode_completion_skipped_in_plain_markdown() {
    let server = TestLspServer::new();
    // `.git/HEAD` anchors the project boundary so config discovery doesn't
    // leak in from an ancestor `panache.toml` on the host.
    let (tmp, doc_uri) =
        open_quarto_doc_with_files(&server, &[(".git/HEAD", ""), ("_intro.qmd", "")], "doc.md");
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str()).await;

    let content = "{{< include _ >}}\n";
    server
        .open_document(doc_uri.as_str(), content, "markdown")
        .await;

    let result = server.completion(doc_uri.as_str(), 0, 13).await;
    assert!(
        result.is_none(),
        "shortcode completion should be Quarto-only"
    );
}

#[tokio::test]
async fn test_shortcode_completion_skipped_on_named_arg() {
    let server = TestLspServer::new();
    let (tmp, doc_uri) = open_quarto_doc_with_files(&server, &[("nb.ipynb", "")], "doc.qmd");
    let root_uri = Uri::from_file_path(tmp.path()).expect("workspace uri");
    server.initialize(root_uri.as_str()).await;

    let content = "{{< embed nb.ipynb echo=t >}}\n";
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    // Cursor inside `echo=t|`: line 0, char 25 (just after the `t`).
    let result = server.completion(doc_uri.as_str(), 0, 25).await;
    assert!(
        result.is_none(),
        "named args should not trigger path completion"
    );
}
