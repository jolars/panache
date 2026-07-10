//! File watcher handler for bibliography files.

use std::path::PathBuf;

use salsa::Durability;

use lsp_types::{DidChangeWatchedFilesParams, MessageType, Uri};

use super::super::helpers;
use crate::lsp::DocumentState;
use crate::lsp::global_state::GlobalState;
use crate::lsp::uri_ext::UriExt;

pub(crate) fn did_change_watched_files(gs: &mut GlobalState, params: DidChangeWatchedFilesParams) {
    // A watcher event means the filesystem changed in a way salsa cannot see
    // through its inputs: `collect_includes` / `find_project_documents` probe the
    // filesystem directly (residual G3 reads), so a newly-created include is
    // invisible to a memoized `project_graph`. Interning each changed path adds
    // any new file to the `FileSet`, which re-runs `project_graph` (the only
    // reader of the set) so those probes are re-evaluated --- a targeted, in-graph
    // replacement for the former global `CacheGeneration` bump, which also
    // invalidated every document's `metadata` memo (audit §3.3 / G3).
    let changed_paths: Vec<PathBuf> = params
        .changes
        .iter()
        .filter_map(|change| change.uri.to_file_path().map(|p| p.into_owned()))
        .collect();
    for path in &changed_paths {
        gs.writer.db_mut().intern_file(Some(path.clone()));
    }

    // A `panache.toml`/`.panache.toml` edit changes config for open documents
    // that don't get re-read until their next keystroke; refresh them all now.
    // Config files are matched by name because the `.toml` extension can't
    // distinguish a config file from any other TOML; a base reached via `extend`
    // can have any name, so it is matched instead against the tracked chain set
    // (canonicalized to compare with the client's possibly non-canonical path).
    // The trailing `arm_settle` re-lints.
    let config_changed = changed_paths.iter().any(|path| {
        matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some("panache.toml") | Some(".panache.toml")
        ) || gs
            .watched_config_files
            .contains(&path.canonicalize().unwrap_or_else(|_| path.clone()))
    });
    if config_changed {
        crate::lsp::documents::reload_open_documents_config(gs);
    }

    // Reloading the open documents' referenced files on the writer then loads any
    // newly-created file (flipping its `None`->`Some` text input) before the
    // cached-text sync and re-lint below, so both observe fresh content.
    crate::lsp::documents::reload_open_documents_referenced_files(gs);

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
            && gs
                .writer
                .db_mut()
                .update_file_text_if_cached_with_durability(&path, contents, Durability::MEDIUM)
        {
            gs.sender.log_message(
                MessageType::INFO,
                format!("Updated cached file: {}", path.display()),
            );
        }

        // `.yml`/`.yaml` can be a project manifest (`_quarto.yml`/`_metadata.yml`/
        // `_bookdown.yml`/`_output.yml` or a `metadata-files:` include) as well as
        // a bibliography. A manifest change won't match any document's
        // bibliography paths, so it needs its own reference check.
        let is_manifest = matches!(extension, Some("yaml") | Some("yml"));
        if !is_bibliography && !is_manifest {
            continue;
        }

        gs.sender.log_message(
            MessageType::INFO,
            format!("Referenced file changed: {}", path.display()),
        );

        // Find all open documents that reference the changed file — as a
        // bibliography or as a project manifest — and re-lint them so the change
        // takes effect immediately (bib indices refresh; manifest parse errors
        // re-publish on, or clear from, the manifest's own URI). Consult salsa so
        // the reads observe the freshly-synced content above.
        let states: Vec<(String, DocumentState)> = gs
            .document_map
            .iter()
            .map(|(uri_str, state)| (uri_str.clone(), state.clone()))
            .collect();

        let mut affected_documents: Vec<Uri> = Vec::new();
        for (uri_str, state) in states {
            // Only saved documents reference files on disk.
            let Some(doc_path) = state.path.clone() else {
                continue;
            };
            let Ok(uri) = uri_str.parse::<Uri>() else {
                continue;
            };

            let mut relint = false;
            if is_bibliography {
                let parsed_yaml_regions = crate::salsa::parsed_yaml_regions_for_file(
                    gs.writer.db(),
                    state.salsa_file,
                    state.salsa_config,
                );
                if helpers::is_yaml_frontmatter_valid(parsed_yaml_regions) {
                    let metadata = crate::salsa::metadata(
                        gs.writer.db(),
                        state.salsa_file,
                        state.salsa_config,
                    );
                    if let Some(bib_info) = metadata.bibliography.as_ref()
                        && bib_info.paths.iter().any(|p| p == &path)
                    {
                        relint = true;
                    }
                }
            }
            if !relint && is_manifest {
                let graph = crate::salsa::project_structure(
                    gs.writer.db(),
                    state.salsa_file,
                    state.salsa_config,
                );
                relint = graph
                    .dependencies(&doc_path, Some(crate::salsa::EdgeKind::ProjectConfig))
                    .into_iter()
                    .chain(
                        graph.dependencies(&doc_path, Some(crate::salsa::EdgeKind::MetadataFile)),
                    )
                    .any(|p| p == path);
            }
            if relint {
                affected_documents.push(uri);
            }
        }

        // A referenced-file change is infrequent, so run external linters for
        // each affected document on the next settle. The settle re-lints every
        // open document, so manifest parse errors re-publish on (or clear from)
        // the manifest's own URI even for documents not flagged here.
        for uri in affected_documents {
            gs.arm_settle_external(uri);
        }
    }

    // Any watched-file change can shift the database (FileSet interning, synced
    // text); arm the settle so the all-docs pass re-lints over the fresh state
    // even when no document was flagged for external linters above.
    if !changed_paths.is_empty() {
        gs.arm_settle();
    }
}
