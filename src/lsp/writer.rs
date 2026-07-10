//! The salsa writer.
//!
//! [`WriterHandle`] owns the master [`SalsaDb`](crate::salsa::SalsaDb). Today it
//! is a thin pass-through wrapper still living inside [`GlobalState`], mutated on
//! the main event loop exactly as before. It exists to concentrate every
//! salsa-touching access behind one type so a later phase can relocate the
//! database onto a dedicated writer thread without re-threading call sites.
//!
//! **Concurrency invariant (validated by the test below).** A salsa write
//! (`db_mut()` → `zalsa_mut` → `cancel_others`) blocks until the live-clone count
//! is exactly 1. So when the database moves off-thread, the owning thread must be
//! the sole holder of a live handle at write time: read snapshots have to be
//! transient — minted by the owner and dropped when the read finishes or
//! cancels. A persistent clone retained on the main loop would keep the count
//! `>= 2` and deadlock the writer, so reads are routed to the owner to mint an
//! ephemeral snapshot (fatou's model, and panache's current main-loop model),
//! not served from a long-lived main-loop clone.
//!
//! [`GlobalState`]: crate::lsp::global_state::GlobalState

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use lsp_types::{MessageType, Uri};

use crate::lsp::global_state::{ClientSender, DocumentMap};
use crate::salsa::{Analysis, SalsaDb};

/// Owns the master salsa database handle and the config state that feeds it.
///
/// Reads clone the handle (`Analysis`, a cheap `Arc` bump over the shared
/// `salsa::Storage`); writes go through [`db_mut`](Self::db_mut). Config
/// resolution lives here too (workspace roots, the extend-chain watch set, the
/// toast-dedup record) because loading a document's config is a write-side
/// concern that feeds `FileConfig` inputs. The writer owns the whole document
/// map (salsa handles + trees + paths), not just the salsa-input side, so it
/// can mint a complete `StateSnapshot` without bouncing back to the main loop.
/// The settle machinery migrates here in a later phase.
pub(crate) struct WriterHandle {
    db: SalsaDb,

    /// Open documents. `Arc` so snapshots clone it in O(1); writers use
    /// [`Arc::make_mut`] for copy-on-write single-writer semantics.
    document_map: Arc<DocumentMap>,

    /// Workspace roots, for per-document (longest-prefix) config resolution.
    workspace_folders: Vec<PathBuf>,

    /// Canonical paths of config files reached via `extend` by some open
    /// document's config. The config-name globs only match `panache.toml` /
    /// `.panache.toml`, so a differently-named base (`base.toml`) would go
    /// unwatched; the file watcher consults this set to reload open documents
    /// when such a base changes.
    watched_config_files: HashSet<PathBuf>,

    /// The last config parse error toasted per config-file path, so a broken
    /// `panache.toml` raises a `window/showMessage` once (not on every keystroke
    /// that reloads config). Cleared when the file parses again, so a later
    /// breakage re-notifies.
    config_error_reports: HashMap<PathBuf, String>,

    /// Client channel for the one-shot config-parse-error toast.
    sender: ClientSender,
}

impl WriterHandle {
    /// A fresh writer over a default (empty) database.
    pub(crate) fn new(sender: ClientSender) -> Self {
        Self {
            db: SalsaDb::default(),
            document_map: Arc::new(DocumentMap::new()),
            workspace_folders: Vec::new(),
            watched_config_files: HashSet::new(),
            config_error_reports: HashMap::new(),
            sender,
        }
    }

    /// Shared read access to the database.
    pub(crate) fn db(&self) -> &SalsaDb {
        &self.db
    }

    /// Exclusive write access to the database. The `&mut` borrow is what salsa
    /// uses to cancel any in-flight reads on cloned handles.
    pub(crate) fn db_mut(&mut self) -> &mut SalsaDb {
        &mut self.db
    }

    /// Mint a cheap read-only snapshot of the database for a worker thread.
    pub(crate) fn analysis(&self) -> Analysis {
        Analysis::new(self.db.clone())
    }

    /// Shared read access to the open-document map.
    pub(crate) fn document_map(&self) -> &DocumentMap {
        &self.document_map
    }

    /// Mutable access to the document map (copy-on-write if a snapshot still
    /// holds the previous `Arc`).
    pub(crate) fn document_map_mut(&mut self) -> &mut DocumentMap {
        Arc::make_mut(&mut self.document_map)
    }

    /// A cheap `Arc` clone of the document map, for snapshot assembly.
    pub(crate) fn document_map_arc(&self) -> Arc<DocumentMap> {
        Arc::clone(&self.document_map)
    }

    /// The current workspace roots.
    pub(crate) fn workspace_folders(&self) -> &[PathBuf] {
        &self.workspace_folders
    }

    /// Replace the workspace roots (set once at `initialize`).
    pub(crate) fn set_workspace_folders(&mut self, folders: Vec<PathBuf>) {
        self.workspace_folders = folders;
    }

    /// Apply a `didChangeWorkspaceFolders` delta: drop removed roots, append new
    /// ones (deduplicated).
    pub(crate) fn update_workspace_folders(&mut self, removed: &[PathBuf], added: Vec<PathBuf>) {
        self.workspace_folders
            .retain(|folder| !removed.contains(folder));
        for path in added {
            if !self.workspace_folders.contains(&path) {
                self.workspace_folders.push(path);
            }
        }
    }

    /// Config files reached via `extend`, watched so a renamed base config still
    /// triggers a reload.
    pub(crate) fn watched_config_files(&self) -> &HashSet<PathBuf> {
        &self.watched_config_files
    }

    /// Load config for `uri`, toasting once when a discovered `panache.toml`
    /// fails to parse and falling back to the flavor-detected default so the
    /// document still parses and lints.
    ///
    /// The persistent surface is the diagnostic the settle pass publishes on the
    /// config file (see [`crate::lsp::handlers::diagnostics::config_publishes`]);
    /// this adds a one-shot `window/showMessage` and clears the dedup record when
    /// the file parses again, so a later breakage re-notifies.
    pub(crate) fn load_config_notifying(&mut self, uri: &Uri) -> crate::Config {
        match crate::lsp::config::try_load_config_with_chain(&self.workspace_folders, Some(uri)) {
            Ok((config, source, chain)) => {
                if let Some(path) = source.path() {
                    self.config_error_reports.remove(path);
                }
                // Track every file in the extend chain so the watcher reloads
                // this document when a base config (any name/location) changes.
                self.watched_config_files.extend(chain);
                config
            }
            Err(err) => {
                if self.config_error_reports.get(&err.path) != Some(&err.message) {
                    self.sender
                        .show_message(MessageType::ERROR, format!("panache: {err}"));
                    self.config_error_reports
                        .insert(err.path.clone(), err.message.clone());
                }
                crate::lsp::config::default_config_for_uri(Some(uri))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use salsa::Durability;

    use super::WriterHandle;
    use crate::lsp::helpers::catch_cancelled;

    /// Pins down the salsa concurrency invariant the writer-thread port must
    /// obey. `zalsa_mut` (every write) runs `cancel_others`, which **blocks until
    /// the live-clone count is exactly 1** (see salsa `storage.rs`:
    /// `while *clones != 1 { cvar.wait() }`). Two consequences shape the design:
    ///
    /// 1. A *persistent* database clone held anywhere keeps the count `>= 2`
    ///    forever, so the owning thread's next write **deadlocks**. The main loop
    ///    therefore must NOT retain a long-lived read clone (`read_db`); read
    ///    snapshots must be transient — minted by the owning thread and dropped
    ///    when the read finishes or cancels. This is exactly fatou's model (and
    ///    panache's current main-loop model): the db owner mints an ephemeral
    ///    snapshot per read.
    /// 2. Given transient snapshots, cross-thread reads are safe: a reader on
    ///    another thread holds its snapshot only for the duration of one read,
    ///    then drops it, so the owner's write proceeds once the clone count
    ///    returns to one. A read racing a write either observes a valid revision
    ///    or unwinds to a cancellation that `catch_cancelled` maps to `None`.
    ///
    /// The test hands the reader a *fresh* snapshot per round and waits for it to
    /// drop before the next write, mirroring the "owner mints, reader drops"
    /// contract. It asserts liveness (no deadlock) and visibility (the reader
    /// observes each committed revision).
    #[test]
    fn transient_cross_thread_snapshots_stay_live_and_visible() {
        const ROUNDS: u64 = 200;
        let path = std::path::PathBuf::from("/spike/doc.qmd");

        // A throwaway client channel; this test never emits toasts.
        let (client_tx, _client_rx) = crossbeam_channel::unbounded();
        let mut writer = WriterHandle::new(crate::lsp::global_state::ClientSender::new(client_tx));
        let file = writer.db_mut().update_file_text_with_durability(
            path.clone(),
            "v0".to_string(),
            Durability::LOW,
        );
        let config = crate::salsa::FileConfig::new(writer.db(), crate::Config::default());

        // Owner (this thread) hands a transient snapshot to the reader thread each
        // round and receives back the value the reader observed (after it dropped
        // the snapshot). A rendezvous channel keeps at most one snapshot in the
        // reader's hands at a time.
        let (snap_tx, snap_rx) = crossbeam_channel::bounded::<crate::salsa::Analysis>(0);
        let (ack_tx, ack_rx) = crossbeam_channel::bounded::<Option<String>>(0);

        let reader = std::thread::spawn(move || {
            while let Ok(snap) = snap_rx.recv() {
                let value = catch_cancelled(|| {
                    crate::salsa::parsed_tree_root(snap.db(), file, config)
                        .text()
                        .to_string()
                });
                // Drop the snapshot BEFORE acking so the owner's next write sees
                // the clone count return to 1 and never blocks.
                drop(snap);
                if ack_tx.send(value).is_err() {
                    break;
                }
            }
        });

        for i in 1..=ROUNDS {
            writer.db_mut().update_file_text_with_durability(
                path.clone(),
                format!("v{i}"),
                Durability::LOW,
            );
            // Mint a transient snapshot on the owning thread and hand it off.
            snap_tx.send(writer.analysis()).expect("reader alive");
            let observed = ack_rx.recv().expect("reader acked");
            if let Some(text) = observed {
                let text = text.trim_end();
                assert!(
                    text.strip_prefix('v')
                        .is_some_and(|n| n.parse::<u64>().is_ok()),
                    "reader observed a torn/invalid parse: {text:?}"
                );
            }
        }

        drop(snap_tx);
        drop(ack_rx);
        reader.join().expect("reader thread panicked");

        // Visibility: after all rounds, the owner observes the final revision.
        let final_owned = crate::salsa::parsed_tree_root(writer.db(), file, config)
            .text()
            .to_string();
        assert_eq!(final_owned.trim_end(), format!("v{ROUNDS}"));
    }
}
