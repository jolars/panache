//! Tests for `FileConfig` interning: documents that resolve to the same config
//! value must share one salsa input handle, so config-keyed queries
//! (`project_edges`, `parsed_tree_root`, `metadata`, ...) memoize once across a
//! project instead of recomputing per document. Distinct config values must
//! still get distinct handles.

use super::helpers::*;
use lsp_types::Uri;
use std::fs;
use tempfile::TempDir;

/// Two documents in the same project (same resolved config) share one interned
/// `FileConfig` handle. Without interning each `did_open` minted its own handle,
/// defeating cross-document salsa memoization — the dominant cold-open cost when
/// opening a whole book.
#[test]
fn shares_one_config_handle_across_project_documents() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join("panache.toml"), "line-width = 88\n").unwrap();

    let a_uri = Uri::from_file_path(root.join("a.qmd")).expect("a uri");
    let b_uri = Uri::from_file_path(root.join("b.qmd")).expect("b uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(a_uri.as_str(), "# A\n", "quarto");
    server.open_document(b_uri.as_str(), "# B\n", "quarto");

    let a_cfg = server
        .document_salsa_config(a_uri.as_str())
        .expect("a config");
    let b_cfg = server
        .document_salsa_config(b_uri.as_str())
        .expect("b config");
    assert!(
        a_cfg == b_cfg,
        "documents sharing a config must share one interned FileConfig handle"
    );
}

/// Documents that resolve to *different* config values keep distinct handles, so
/// interning stays keyed on the value and never conflates configs.
#[test]
fn distinct_config_values_get_distinct_handles() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();

    let dir_a = root.join("a");
    let dir_b = root.join("b");
    fs::create_dir_all(&dir_a).unwrap();
    fs::create_dir_all(&dir_b).unwrap();
    fs::write(dir_a.join("panache.toml"), "line-width = 80\n").unwrap();
    fs::write(dir_b.join("panache.toml"), "line-width = 100\n").unwrap();

    let a_uri = Uri::from_file_path(dir_a.join("doc.qmd")).expect("a uri");
    let b_uri = Uri::from_file_path(dir_b.join("doc.qmd")).expect("b uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(a_uri.as_str(), "# A\n", "quarto");
    server.open_document(b_uri.as_str(), "# B\n", "quarto");

    let a_cfg = server
        .document_salsa_config(a_uri.as_str())
        .expect("a config");
    let b_cfg = server
        .document_salsa_config(b_uri.as_str())
        .expect("b config");
    assert!(
        a_cfg != b_cfg,
        "documents with different config values must get distinct FileConfig handles"
    );
}

/// A text edit (`did_change`) with unchanged config keeps the document on its
/// shared interned handle, so editing one project document does not fracture the
/// cross-document memoization the interning establishes.
#[test]
fn text_edit_preserves_shared_config_handle() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join("panache.toml"), "line-width = 88\n").unwrap();

    let a_uri = Uri::from_file_path(root.join("a.qmd")).expect("a uri");
    let b_uri = Uri::from_file_path(root.join("b.qmd")).expect("b uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(a_uri.as_str(), "# A\n", "quarto");
    server.open_document(b_uri.as_str(), "# B\n", "quarto");

    server.edit_document(a_uri.as_str(), vec![full_document_change("# A edited\n")]);

    let a_cfg = server
        .document_salsa_config(a_uri.as_str())
        .expect("a config");
    let b_cfg = server
        .document_salsa_config(b_uri.as_str())
        .expect("b config");
    assert!(
        a_cfg == b_cfg,
        "an edit with unchanged config must keep the shared interned handle"
    );
}
