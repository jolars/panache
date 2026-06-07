//! Tests for LSP-side config discovery: nearest-config-wins per file and
//! the `.git`-anchored project boundary on the ancestor walk.

use super::helpers::*;
use lsp_types::Uri;
use std::fs;
use tempfile::TempDir;

/// When two `panache.toml` files exist — one at the workspace root and one in
/// a subdirectory — opening a document inside the subdirectory must pick up
/// the closer config, not the workspace-root one.
#[test]
fn lsp_picks_nearest_panache_toml_per_file() {
    let mut server = TestLspServer::new();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Mark this directory as a project root so discovery doesn't ascend
    // into the host's filesystem.
    fs::create_dir_all(root.join(".git")).unwrap();

    // Workspace-root config pins line-width = 80.
    fs::write(root.join("panache.toml"), "line-width = 80\n").unwrap();

    // Inner subdir overrides line-width = 40.
    let inner = root.join("chapter");
    fs::create_dir_all(&inner).unwrap();
    fs::write(inner.join("panache.toml"), "line-width = 40\n").unwrap();

    let doc_path = inner.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");
    server.initialize(root_uri.as_str());

    // Open a document with a paragraph longer than 40 but shorter than 80
    // chars; the formatter wraps according to the *nearest* config, so we
    // expect the result to be wrapped (line-width=40 from the inner config).
    let long = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi";
    server.open_document(doc_uri.as_str(), long, "quarto");
    let edits = server
        .format_document(doc_uri.as_str())
        .expect("format edits");
    let new_text = edits
        .iter()
        .map(|e| e.new_text.as_str())
        .collect::<String>();
    let max_line_len = new_text.lines().map(str::len).max().unwrap_or(0);
    assert!(
        max_line_len <= 40,
        "expected nearest config (line-width=40) to win, got max line {max_line_len}\nformatted:\n{new_text}"
    );
}

/// A `.git` directory at the workspace root makes that directory the project
/// boundary: a stray `panache.toml` above it must not be inherited.
#[test]
fn lsp_does_not_inherit_panache_toml_above_git_root() {
    let mut server = TestLspServer::new();
    // The outer dir simulates an unrelated `/tmp/panache.toml`.
    let outer = TempDir::new().unwrap();
    fs::write(outer.path().join("panache.toml"), "flavor = \"quarto\"\n").unwrap();

    // Workspace is nested inside outer and marked as its own git repo.
    let workspace = outer.path().join("ws");
    fs::create_dir_all(workspace.join(".git")).unwrap();

    let doc_path = workspace.join("doc.md");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let root_uri = Uri::from_file_path(&workspace).expect("root uri");
    server.initialize(root_uri.as_str());

    // If the LSP wrongly inherited the outer config, this `.md` file would be
    // treated as Quarto and shortcode completion would fire. The `.git`
    // boundary keeps shortcode completion off.
    let content = "{{< include _ >}}\n";
    server.open_document(doc_uri.as_str(), content, "markdown");
    let result = server.completion(doc_uri.as_str(), 0, 13);
    assert!(
        result.is_none(),
        "discovery must not ascend above the .git boundary"
    );
}
