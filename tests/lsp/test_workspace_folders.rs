//! Tests for multi-root workspace support and `workspace/didChangeWorkspaceFolders`:
//! config is resolved against the folder that contains each document, and folder
//! add/remove notifications re-resolve open documents live.

use super::helpers::*;
use lsp_types::Uri;
use std::fs;
use tempfile::TempDir;

/// A paragraph longer than 40 chars but wrappable, used to observe which
/// `line-width` config actually applied by inspecting the formatted line length.
const LONG: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi";

fn max_line_len(edits: &[lsp_types::TextEdit]) -> usize {
    let new_text = edits
        .iter()
        .map(|e| e.new_text.as_str())
        .collect::<String>();
    new_text.lines().map(str::len).max().unwrap_or(0)
}

/// Build a git-anchored workspace folder with its own `panache.toml` and one
/// `doc.qmd`. Returns `(folder_path, doc_uri)`.
fn make_folder(parent: &std::path::Path, name: &str, line_width: u32) -> (std::path::PathBuf, Uri) {
    let folder = parent.join(name);
    fs::create_dir_all(folder.join(".git")).unwrap();
    fs::write(
        folder.join("panache.toml"),
        format!("line-width = {line_width}\n"),
    )
    .unwrap();
    let doc_path = folder.join("doc.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    (folder, doc_uri)
}

/// With two workspace folders, each document resolves config against *its own*
/// folder's `panache.toml`, not the first folder's. Proves the latent
/// single-root bug is fixed.
#[test]
fn multi_root_resolves_config_per_folder() {
    let mut server = TestLspServer::new();
    let tmp = TempDir::new().unwrap();

    let (folder_a, doc_a) = make_folder(tmp.path(), "a", 80);
    let (folder_b, doc_b) = make_folder(tmp.path(), "b", 40);

    let root_a = Uri::from_file_path(&folder_a).unwrap();
    let root_b = Uri::from_file_path(&folder_b).unwrap();
    server.initialize_with_folders(&[root_a.as_str(), root_b.as_str()]);

    server.open_document(doc_a.as_str(), LONG, "quarto");
    server.open_document(doc_b.as_str(), LONG, "quarto");

    // Folder A: line-width = 80, so the paragraph fits on one line (no wrap).
    let edits_a = server.format_document(doc_a.as_str());
    if let Some(edits) = edits_a {
        assert!(
            max_line_len(&edits) > 40,
            "folder A (line-width=80) should not wrap at 40"
        );
    }

    // Folder B: line-width = 40, so the paragraph wraps.
    let edits_b = server
        .format_document(doc_b.as_str())
        .expect("folder B format edits");
    assert!(
        max_line_len(&edits_b) <= 40,
        "folder B (line-width=40) config must win"
    );
}

/// Adding a workspace folder makes documents inside it resolve against that
/// folder's config; the open document is re-resolved live.
#[test]
fn adding_folder_reresolves_open_document_config() {
    let mut server = TestLspServer::new();
    let tmp = TempDir::new().unwrap();

    let (folder_a, _doc_a) = make_folder(tmp.path(), "a", 80);
    let (folder_b, doc_b) = make_folder(tmp.path(), "b", 40);

    // Start with only folder A registered.
    let root_a = Uri::from_file_path(&folder_a).unwrap();
    server.initialize_with_folders(&[root_a.as_str()]);

    // Open a document living under B before B is a workspace folder. B has a
    // `.git`, so discovery still finds B's own config on the ancestor walk, but
    // add the folder to exercise the notification path regardless.
    server.open_document(doc_b.as_str(), LONG, "quarto");

    let root_b = Uri::from_file_path(&folder_b).unwrap();
    server.did_change_workspace_folders(&[root_b.as_str()], &[]);

    let edits_b = server
        .format_document(doc_b.as_str())
        .expect("format edits after adding folder B");
    assert!(
        max_line_len(&edits_b) <= 40,
        "after adding folder B, its line-width=40 config must apply"
    );
}

/// Removing a workspace folder does not panic and leaves the remaining folder
/// resolving config correctly.
#[test]
fn removing_folder_falls_back_cleanly() {
    let mut server = TestLspServer::new();
    let tmp = TempDir::new().unwrap();

    let (folder_a, doc_a) = make_folder(tmp.path(), "a", 40);
    let (folder_b, _doc_b) = make_folder(tmp.path(), "b", 80);

    let root_a = Uri::from_file_path(&folder_a).unwrap();
    let root_b = Uri::from_file_path(&folder_b).unwrap();
    server.initialize_with_folders(&[root_a.as_str(), root_b.as_str()]);

    server.open_document(doc_a.as_str(), LONG, "quarto");

    // Remove B; A stays and its document keeps resolving line-width = 40.
    server.did_change_workspace_folders(&[], &[root_b.as_str()]);

    let edits_a = server
        .format_document(doc_a.as_str())
        .expect("format edits after removing folder B");
    assert!(
        max_line_len(&edits_a) <= 40,
        "folder A config must still apply after removing B"
    );
}
