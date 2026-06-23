//! Tests for surfacing a broken `panache.toml` in the LSP: a discovered config
//! that fails to parse must not silently degrade to default formatting. It is
//! surfaced three ways — a diagnostic on the config file, a one-shot toast, and
//! refusal to format — and all three clear once the config parses again.

use super::helpers::*;
use lsp_types::{DiagnosticSeverity, FileChangeType, FileEvent, MessageType, Uri};
use std::fs;
use std::time::Duration;
use tempfile::TempDir;

/// Content the default formatter is guaranteed to rewrite (extra heading-marker
/// spaces and surplus blank lines), so "format produced no edits" can only mean
/// the server *refused*, not that the document was already well-formed.
const REFORMATTABLE: &str = "#   Title\n\n\n\nbody text\n";

/// A workspace rooted at a fresh `.git` boundary (so config discovery doesn't
/// ascend into the host filesystem) with `panache.toml` holding `contents`.
/// Returns the temp dir plus the doc and root URIs.
fn workspace_with_config(contents: &str) -> (TempDir, Uri, Uri, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    let config_path = root.join("panache.toml");
    fs::write(&config_path, contents).unwrap();
    let doc_uri = Uri::from_file_path(root.join("doc.qmd")).expect("doc uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");
    (tmp, doc_uri, root_uri, config_path)
}

/// A typo'd discovered `panache.toml` makes the server refuse to format rather
/// than silently apply default formatting. A control workspace with a valid
/// config proves the document content is otherwise reformattable.
#[test]
fn broken_config_refuses_to_format() {
    // Control: a valid config reformats the document.
    let (_valid_tmp, valid_doc, valid_root, _) = workspace_with_config("line-width = 80\n");
    let mut server = TestLspServer::new();
    server.initialize(valid_root.as_str());
    server.open_document(valid_doc.as_str(), REFORMATTABLE, "quarto");
    assert!(
        server.format_document(valid_doc.as_str()).is_some(),
        "control: a valid config must format the reformattable document"
    );

    // A typo'd key (`lin-width`) makes the server refuse.
    let (_tmp, doc, root, _) = workspace_with_config("lin-width = 80\n");
    let mut server = TestLspServer::new();
    server.initialize(root.as_str());
    server.open_document(doc.as_str(), REFORMATTABLE, "quarto");
    assert!(
        server.format_document(doc.as_str()).is_none(),
        "a broken config must make the server refuse to format, not silently use defaults"
    );
}

/// A broken `panache.toml` publishes an error diagnostic on the config file's
/// own URI, anchored at the offending key.
#[test]
fn broken_config_publishes_diagnostic_on_config_file() {
    let (_tmp, doc, root, config_path) = workspace_with_config("lin-width = 80\n");
    let config_uri = Uri::from_file_path(&config_path).unwrap();

    let mut server = TestLspServer::new();
    server.initialize(root.as_str());
    server.open_document(doc.as_str(), "# Title\n", "quarto");
    server.pump(Duration::from_secs(5));

    let config_diags: Vec<_> = server
        .drain_all_publish_diagnostics()
        .into_iter()
        .filter(|p| p.uri == config_uri)
        .flat_map(|p| p.diagnostics)
        .collect();

    let diag = config_diags
        .iter()
        .find(|d| d.message.contains("unknown field"))
        .expect("an error diagnostic must be published on panache.toml");
    assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(
        diag.range.start.line, 0,
        "diagnostic must anchor at the offending key on line 0"
    );
    assert!(
        diag.message.contains("lin-width"),
        "diagnostic must name the offending key: {}",
        diag.message
    );
}

/// Opening a document under a broken config raises a single `window/showMessage`
/// toast; a subsequent edit with the same error does not re-toast (dedup).
#[test]
fn broken_config_toasts_once() {
    let (_tmp, doc, root, _) = workspace_with_config("lin-width = 80\n");
    let mut server = TestLspServer::new();
    server.initialize(root.as_str());

    server.open_document(doc.as_str(), "# Title\n", "quarto");
    let toasts = server.drain_show_messages();
    let toast = toasts
        .iter()
        .find(|m| m.typ == MessageType::ERROR)
        .expect("opening under a broken config must toast once");
    assert!(
        toast.message.contains("unknown field"),
        "toast must describe the parse failure: {}",
        toast.message
    );

    // A keystroke reloads config but the same error must not re-toast.
    server.edit_document(
        doc.as_str(),
        vec![full_document_change("# Title\n\nmore\n")],
    );
    assert!(
        server.drain_show_messages().is_empty(),
        "the same config error must not toast again on every edit"
    );
}

/// Fixing the config on disk clears the config-file diagnostic and lets the
/// server format again.
#[test]
fn fixing_config_clears_diagnostic_and_resumes_formatting() {
    let (_tmp, doc, root, config_path) = workspace_with_config("lin-width = 80\n");
    let config_uri = Uri::from_file_path(&config_path).unwrap();

    let mut server = TestLspServer::new();
    server.initialize(root.as_str());
    server.open_document(doc.as_str(), REFORMATTABLE, "quarto");
    server.pump(Duration::from_secs(5));

    // Broken: diagnostic present, formatting refused.
    let broken: Vec<_> = server
        .drain_all_publish_diagnostics()
        .into_iter()
        .filter(|p| p.uri == config_uri)
        .flat_map(|p| p.diagnostics)
        .collect();
    assert!(
        !broken.is_empty(),
        "broken config must publish a diagnostic before the fix"
    );
    assert!(server.format_document(doc.as_str()).is_none());

    // Fix the config on disk and notify via a watcher event.
    fs::write(&config_path, "line-width = 80\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: config_uri.clone(),
        typ: FileChangeType::CHANGED,
    }]);
    server.pump(Duration::from_secs(5));

    // The config diagnostic is cleared (the latest publish for it is empty).
    let latest_config_publish = server
        .drain_all_publish_diagnostics()
        .into_iter()
        .rfind(|p| p.uri == config_uri);
    if let Some(publish) = latest_config_publish {
        assert!(
            publish.diagnostics.is_empty(),
            "fixing the config must clear its diagnostic, got {:?}",
            publish.diagnostics
        );
    }

    // Formatting resumes now that the config parses.
    assert!(
        server.format_document(doc.as_str()).is_some(),
        "formatting must resume once the config parses again"
    );
}
