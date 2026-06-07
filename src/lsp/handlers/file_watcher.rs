//! File watcher handler for bibliography files.

use salsa::Durability;

use lsp_types::{DidChangeWatchedFilesParams, MessageType, Uri};

use super::super::helpers;
use crate::lsp::DocumentState;
use crate::lsp::global_state::GlobalState;
use crate::lsp::uri_ext::UriExt;

pub(crate) fn did_change_watched_files(gs: &mut GlobalState, params: DidChangeWatchedFilesParams) {
    for change in params.changes {
        let Some(path) = change.uri.to_file_path().map(|p| p.into_owned()) else {
            continue;
        };

        let extension = path.extension().and_then(|e| e.to_str());
        let is_bibliography = matches!(
            extension,
            Some("bib") | Some("json") | Some("yaml") | Some("yml") | Some("ris")
        );

        // Always keep salsa's cached file text in sync when possible.
        if let Ok(contents) = std::fs::read_to_string(&path)
            && gs.salsa.update_file_text_if_cached_with_durability(
                &path,
                contents,
                Durability::MEDIUM,
            )
        {
            gs.sender.log_message(
                MessageType::INFO,
                format!("Updated cached file: {}", path.display()),
            );
        }

        if !is_bibliography {
            continue;
        }

        gs.sender.log_message(
            MessageType::INFO,
            format!("Bibliography file changed: {}", path.display()),
        );

        // Find all documents that reference this bibliography file and re-lint
        // them. Consult salsa metadata so bib watcher updates take effect
        // immediately.
        let states: Vec<(String, DocumentState)> = gs
            .document_map
            .iter()
            .map(|(uri_str, state)| (uri_str.clone(), state.clone()))
            .collect();

        let affected_documents: Vec<Uri> = states
            .into_iter()
            .filter_map(|(uri_str, state)| {
                let doc_path = state.path?;
                if !helpers::is_yaml_frontmatter_valid(&state.parsed_yaml_regions) {
                    return None;
                }
                let metadata = crate::salsa::metadata(
                    &gs.salsa,
                    state.salsa_file,
                    state.salsa_config,
                    doc_path,
                )
                .clone();
                let bib_info = metadata.bibliography.as_ref()?;
                if bib_info.paths.iter().any(|p| p == &path) {
                    uri_str.parse::<Uri>().ok()
                } else {
                    None
                }
            })
            .collect();

        // A bibliography change is infrequent, so run the full pass (external
        // linters included) for each affected document.
        for uri in affected_documents {
            gs.spawn_lint(uri, false, true);
        }
    }
}
