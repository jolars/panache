use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::ls_types::{Location, Range, Uri};

use crate::salsa::Db;

use super::conversions;

#[derive(Clone)]
pub(crate) struct IndexedDocument {
    pub(crate) uri: Uri,
    pub(crate) text: String,
    pub(crate) symbol_index: crate::salsa::SymbolUsageIndex,
}

pub(crate) async fn project_document_inputs(
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    doc_path: &Path,
    current_content: &str,
) -> Vec<(PathBuf, String)> {
    let db = salsa_db.lock().await;
    let mut doc_paths =
        crate::salsa::project_graph(&*db, salsa_file, salsa_config, doc_path.to_path_buf())
            .documents()
            .iter()
            .cloned()
            .collect::<Vec<_>>();
    if !doc_paths.contains(&doc_path.to_path_buf()) {
        doc_paths.push(doc_path.to_path_buf());
    }
    doc_paths.sort();
    doc_paths.dedup();

    let mut inputs = Vec::new();
    for path in doc_paths {
        let text = if path == doc_path {
            current_content.to_string()
        } else if let Some(file) = db.file_text(path.clone()) {
            file.text(&*db).clone()
        } else {
            continue;
        };
        inputs.push((path, text));
    }

    inputs
}

pub(crate) async fn project_symbol_documents(
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    doc_path: &Path,
    current_uri: &Uri,
    current_content: &str,
) -> Vec<IndexedDocument> {
    let inputs = project_document_inputs(
        salsa_db,
        salsa_file,
        salsa_config,
        doc_path,
        current_content,
    )
    .await;

    let db = salsa_db.lock().await;
    let mut docs = Vec::new();
    for (path, text) in inputs {
        let file = if path == doc_path {
            salsa_file
        } else if let Some(file) = crate::salsa::Db::file_text(&*db, path.clone()) {
            file
        } else {
            continue;
        };

        let symbol_index =
            crate::salsa::symbol_usage_index(&*db, file, salsa_config, path.clone()).clone();
        let uri = Uri::from_file_path(&path).unwrap_or_else(|| current_uri.clone());
        docs.push(IndexedDocument {
            uri,
            text,
            symbol_index,
        });
    }

    docs
}

pub(crate) fn location_from_range(uri: &Uri, text: &str, range: rowan::TextRange) -> Location {
    Location {
        uri: uri.clone(),
        range: Range {
            start: conversions::offset_to_position(text, range.start().into()),
            end: conversions::offset_to_position(text, range.end().into()),
        },
    }
}
