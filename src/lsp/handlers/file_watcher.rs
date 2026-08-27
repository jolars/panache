//! Handler for `workspace/didChangeWatchedFiles`: keeps already-tracked file
//! text in sync, releases deleted files' cached text, reloads config, and
//! re-lints documents referencing a changed bibliography or manifest.
//!
//! The watch globs cover every JSON/YAML/TOML/bib/ris file in the workspace,
//! so events arrive for far more files than any document references (a build
//! writing artifacts, for one). This handler must therefore never *load* a
//! file's contents into the database --- referenced files are loaded precisely
//! by `reload_open_documents_referenced_files`; everything else stays an
//! absent input, or unbounded watch traffic becomes unbounded memory.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use salsa::Durability;

use lsp_types::{DidChangeWatchedFilesParams, FileChangeType, MessageType, Uri};

use super::super::helpers;
use crate::lsp::DocumentState;
use crate::lsp::global_state::GlobalState;
use crate::lsp::uri_ext::UriExt;

pub(crate) fn did_change_watched_files(gs: &mut GlobalState, params: DidChangeWatchedFilesParams) {
    // The last event for a path decides its fate: an atomic save can deliver
    // DELETED followed by CREATED for the same path within one batch.
    let mut final_change: HashMap<PathBuf, FileChangeType> = HashMap::new();
    let mut changed_paths: Vec<PathBuf> = Vec::new();
    for change in &params.changes {
        let Some(path) = change.uri.to_file_path().map(|p| p.into_owned()) else {
            continue;
        };
        final_change.insert(path.clone(), change.typ);
        changed_paths.push(path);
    }

    // Only files whose contents were already loaded when the event arrived get
    // the cached-text sync and the deletion eviction below; everything else
    // stays an absent input, or unbounded watch traffic becomes unbounded
    // memory. The snapshot must precede
    // `reload_open_documents_referenced_files`, which loads newly referenced
    // files: taken afterwards it would fold those in and re-read from disk
    // what that reload just read.
    let loaded_paths: HashSet<PathBuf> = changed_paths
        .iter()
        .filter(|path| gs.salsa.file_text_is_loaded(path))
        .cloned()
        .collect();

    // Filesystem probes are not Salsa inputs, so intern changed paths to make
    // newly created references visible to `project_graph` through `FileSet`.
    for (path, change_type) in &final_change {
        if *change_type != FileChangeType::DELETED {
            gs.salsa.intern_file(Some(path.clone()));
        }
    }

    // A deleted file's cached text is released so the database does not retain
    // it for the life of the process; the path is then re-interned with a
    // fresh absent input so `project_graph` observes the absence (mirroring
    // `did_delete_files`). Only a loaded path is evicted: eviction tombstones
    // the vfs slot and re-interning mints an input salsa never drops, so doing
    // this for a merely interned path would leak a slot and an input per
    // delete event --- and rewrite `FileSet` twice, invalidating
    // `project_graph` for every open document --- to release nothing. An open
    // document's buffer is authoritative, so its path is never evicted.
    let open_paths = crate::lsp::documents::open_document_paths(gs);
    for (path, change_type) in &final_change {
        if *change_type != FileChangeType::DELETED
            || open_paths.contains(path)
            || !loaded_paths.contains(path)
        {
            continue;
        }
        if gs.salsa.evict_file_text(path) {
            gs.salsa.intern_file(Some(path.clone()));
        }
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

        // Keep salsa's cached file text in sync --- but only for files whose
        // contents were already tracked before this event (see `loaded_paths`
        // above). Everything else stays an absent input: files a document
        // actually references are loaded precisely by
        // `reload_open_documents_referenced_files`, never by watch traffic.
        if loaded_paths.contains(&path)
            && let Ok(contents) = std::fs::read_to_string(&path)
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
            let Some(doc_path) = crate::salsa::Db::path_of_id(&gs.salsa, state.file_id) else {
                continue;
            };
            let Ok(uri) = uri_str.parse::<Uri>() else {
                continue;
            };

            let mut relint = false;
            if is_bibliography {
                let parsed_yaml_regions = crate::salsa::parsed_yaml_regions_for_file(
                    &gs.salsa,
                    state.salsa_file,
                    state.salsa_config,
                );
                if helpers::is_yaml_frontmatter_valid(parsed_yaml_regions) {
                    let metadata =
                        crate::salsa::metadata(&gs.salsa, state.salsa_file, state.salsa_config);
                    if let Some(bib_info) = metadata.bibliography.as_ref()
                        && bib_info.paths.iter().any(|p| p == &path)
                    {
                        relint = true;
                    }
                }
            }
            if !relint && is_manifest {
                let graph = crate::salsa::project_structure(
                    &gs.salsa,
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
