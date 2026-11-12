//! Standard notebook synchronization; cell identity is independent of parent paths.
use super::{documents, global_state::GlobalState, uri_ext::UriExt};
use lsp_types::*;

pub(crate) fn config_uri(folders: &[std::path::PathBuf], parent: &Uri) -> Uri {
    if parent.to_file_path().is_some() {
        return parent.clone();
    }
    let root = folders
        .first()
        .cloned()
        .or_else(|| std::env::current_dir().ok());
    root.and_then(|r| Uri::from_file_path(r.join("__untitled.ipynb")))
        .unwrap_or_else(|| parent.clone())
}

fn open_cells(gs: &mut GlobalState, parent: &Uri, items: Vec<TextDocumentItem>) {
    for item in items {
        let known_cell = gs
            .notebooks
            .get(parent)
            .is_some_and(|n| n.cells.iter().any(|c| c.document == item.uri));
        if known_cell && item.language_id == "markdown" {
            documents::open_document(
                gs,
                DidOpenTextDocumentParams {
                    text_document: item,
                },
                Some(parent.clone()),
            );
        }
    }
}
pub(crate) fn did_open(gs: &mut GlobalState, params: DidOpenNotebookDocumentParams) {
    let parent = params.notebook_document.uri.clone();
    gs.notebooks
        .insert(parent.clone(), params.notebook_document);
    open_cells(gs, &parent, params.cell_text_documents);
}
pub(crate) fn did_change(gs: &mut GlobalState, params: DidChangeNotebookDocumentParams) {
    let parent = params.notebook_document.uri;
    let Some(book) = gs.notebooks.get_mut(&parent) else {
        return;
    };
    book.version = params.notebook_document.version;
    let Some(cells) = params.change.cells else {
        return;
    };
    let mut opened = Vec::new();
    let mut closed = Vec::new();
    if let Some(structure) = cells.structure {
        let start = structure.array.start as usize;
        let end = start + structure.array.delete_count as usize;
        if end > book.cells.len() {
            log::warn!("Invalid notebook cell splice");
            return;
        }
        book.cells
            .splice(start..end, structure.array.cells.unwrap_or_default());
        opened = structure.did_open.unwrap_or_default();
        closed = structure.did_close.unwrap_or_default();
    }
    if let Some(data) = cells.data {
        for cell in data {
            if let Some(old) = book.cells.iter_mut().find(|c| c.document == cell.document) {
                *old = cell;
            }
        }
    }
    let retained: std::collections::HashSet<_> =
        book.cells.iter().map(|c| c.document.clone()).collect();
    let kinds: Vec<_> = book
        .cells
        .iter()
        .map(|c| (c.document.clone(), c.kind.clone()))
        .collect();
    for (uri, kind) in kinds {
        if let Some(state) = gs.document_map_mut().get_mut(uri.as_str()) {
            state.notebook_cell_kind = Some(kind);
        }
    }
    closed.extend(
        gs.document_map
            .iter()
            .filter(|(_, s)| s.notebook_uri.as_ref() == Some(&parent))
            .filter(|(u, _)| u.parse::<Uri>().is_ok_and(|u| !retained.contains(&u)))
            .filter_map(|(u, _)| u.parse().ok().map(|uri| TextDocumentIdentifier { uri })),
    );
    for cell in closed {
        documents::did_close(
            gs,
            DidCloseTextDocumentParams {
                text_document: cell,
            },
        );
    }
    open_cells(gs, &parent, opened);
    for text in cells.text_content.unwrap_or_default() {
        if gs.document_map.contains_key(text.document.uri.as_str()) {
            documents::did_change(
                gs,
                DidChangeTextDocumentParams {
                    text_document: text.document,
                    content_changes: text.changes,
                },
            );
        }
    }
}
pub(crate) fn did_save(gs: &mut GlobalState, params: DidSaveNotebookDocumentParams) {
    let cells: Vec<Uri> = gs
        .document_map
        .iter()
        .filter(|(_, s)| s.notebook_uri.as_ref() == Some(&params.notebook_document.uri))
        .filter_map(|(u, _)| u.parse().ok())
        .collect();
    for uri in cells {
        documents::refresh_document_config(gs, uri.as_str(), &uri);
    }
    // Config refreshes can cancel diagnostics reads for other documents.
    gs.arm_settle();
}
pub(crate) fn did_close(gs: &mut GlobalState, params: DidCloseNotebookDocumentParams) {
    let cells: Vec<Uri> = gs
        .document_map
        .iter()
        .filter(|(_, s)| s.notebook_uri.as_ref() == Some(&params.notebook_document.uri))
        .filter_map(|(u, _)| u.parse().ok())
        .collect();
    for uri in cells {
        documents::did_close(
            gs,
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
            },
        );
    }
    gs.notebooks.remove(&params.notebook_document.uri);
}
