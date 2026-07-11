//! The salsa writer.
//!
//! [`WriterState`] owns the master [`SalsaDb`](crate::salsa::SalsaDb), the
//! document map, and the config state that feeds them. [`WriterHandle`] is the
//! main loop's handle to that state, in one of two modes:
//!
//! - **Inline** (initial): the state lives inside the handle and is mutated
//!   synchronously on the calling thread. This is the mode the `LspTester`
//!   harness keeps forever, and the mode production uses during the
//!   `initialize`/`initialized` handshake.
//! - **Threaded** (after [`WriterHandle::spawn`]): the state moves onto the
//!   dedicated `panache-lsp-writer` thread. Writes are forwarded as
//!   [`WriteCommand`]s and applied entirely on the writer (it owns the
//!   diagnostics store and the settle machinery, so no effects round-trip);
//!   pooled reads are forwarded as [`ReadJob`]s (the writer mints the
//!   [`StateSnapshot`] and hands the job to the task pools); the debounced
//!   settle **self-times on the writer thread** (`recv_timeout` on its
//!   channel), so the referenced-file disk I/O write phase runs on the writer,
//!   off the main event loop. Settle results ride the task channel to the main
//!   loop, which forwards them back via
//!   [`WriterHandle::forward_settle_result`]; the refresh nudge returns as
//!   [`Task::RefreshDiagnostics`].
//!
//! **Concurrency invariant (validated by the test below).** A salsa write
//! (`db_mut()` → `zalsa_mut` → `cancel_others`) blocks until the live-clone count
//! is exactly 1. So the thread that owns the state must be the sole holder of a
//! live handle at write time: read snapshots have to be transient — minted by
//! the owner and dropped when the read finishes or cancels. A persistent clone
//! retained on the main loop would keep the count `>= 2` and deadlock the
//! writer, so reads are routed to the owner to mint an ephemeral snapshot
//! (fatou's model), not served from a long-lived main-loop clone.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use lsp_types::{Diagnostic, MessageType, Uri};

use crate::lsp::LspRuntimeSettings;
use crate::lsp::global_state::{
    ClientSender, DIAGNOSTICS_DEBOUNCE, DiagnosticCollection, DocumentMap, StateSnapshot, Task,
};
use crate::lsp::task_pool::TaskSpawner;
use crate::lsp::writer_command::WriteCommand;
use crate::salsa::{Analysis, SalsaDb};

/// Owns the master salsa database handle and the state that feeds it.
///
/// Reads clone the handle (`Analysis`, a cheap `Arc` bump over the shared
/// `salsa::Storage`); writes go through [`db_mut`](Self::db_mut). Config
/// resolution lives here too (workspace roots, the extend-chain watch set, the
/// toast-dedup record) because loading a document's config is a write-side
/// concern that feeds `FileConfig` inputs. The writer owns the whole document
/// map (salsa handles + trees + paths), not just the salsa-input side, so it
/// can mint a complete [`StateSnapshot`] without bouncing back to the main
/// loop. It also owns the diagnostics store and the settle machinery (deadline,
/// lint generations, pending externals), so a write's side effects apply here
/// with no main-loop round trip.
pub(crate) struct WriterState {
    db: SalsaDb,

    /// Open documents. `Arc` so snapshots clone it in O(1); writers use
    /// [`Arc::make_mut`] for copy-on-write single-writer semantics.
    document_map: Arc<DocumentMap>,

    /// Runtime settings pushed by the client (`initializationOptions`,
    /// `workspace/didChangeConfiguration`). Owned here because both writers of
    /// it are write-side (initialize, the configuration notification) and its
    /// only reader is `did_change`'s incremental-parsing gate.
    runtime_settings: LspRuntimeSettings,

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

    /// The current diagnostic set: push delivery, the pull store, and
    /// clear-on-fix bookkeeping unified behind one diff-based owner. Snapshots
    /// carry a cheap `Arc` view of it for the pull handlers.
    diagnostics: DiagnosticCollection,

    /// Single debounce timer for the whole workspace. `Some(t)` means a quiescent
    /// re-lint of *all* open documents is due at `t`; each salsa-input write
    /// pushes it out by [`DIAGNOSTICS_DEBOUNCE`]. The all-docs model needs no
    /// per-document deadlines and no cancel→re-arm net: any write that cancels an
    /// in-flight pass has, by construction, already armed the next settle. In
    /// threaded mode the writer thread's `recv_timeout` watches this; inline mode
    /// polls it via `GlobalState::dispatch_due_lints`.
    settle_deadline: Option<Instant>,

    /// One global lint generation, bumped per dispatched settle pass. The pass
    /// result is tagged with it and dropped in [`Self::apply_settle_result`] if
    /// a newer settle has since been dispatched.
    lint_generation: u64,

    /// The highest lint generation whose settle result has actually been applied
    /// to the store. Lags `lint_generation` while a dispatched pass is still in
    /// flight; equal to it once that pass lands. The test harness's `pump` uses
    /// the gap to know a settle is still pending.
    last_applied_lint_generation: u64,

    /// URIs whose next settle pass must also run external linters (the expensive
    /// on-save/-open signal). Built-ins run for every open doc each settle;
    /// externals run only for these. Retired once the pass that ran them
    /// completes, so a save queued after dispatch survives a cancellation.
    external_pending: HashSet<Uri>,

    /// Whether the client advertised pull-diagnostics support at `initialize`
    /// (set pre-spawn). When `true` the store never pushes
    /// `textDocument/publishDiagnostics`; pull handlers serve it instead
    /// (mode-switch — no double reporting).
    supports_pull_diagnostics: bool,

    /// Whether the client advertised `textDocument.diagnostic.relatedDocumentSupport`
    /// (set pre-spawn); carried onto snapshots for the per-document pull handler.
    supports_related_documents: bool,

    /// The in-flight harvest cycle, if any. Never set in inline mode (no
    /// harvester); at most one cycle (one outstanding harvester batch) exists
    /// at a time.
    harvest_cycle: Option<HarvestCycle>,

    /// Monotonic id source for harvest cycles. Ids tag every harvester
    /// request and ride back on the batch, so a late batch from a superseded
    /// (aborted) cycle is rejected instead of misattributed to the current
    /// one — the same staleness contract settle results get from
    /// `lint_generation`.
    harvest_cycle_counter: u64,

    /// Client channel for diagnostics publishes and the one-shot
    /// config-parse-error toast.
    sender: ClientSender,
}

impl WriterState {
    /// A fresh writer state over a default (empty) database.
    pub(crate) fn new(sender: ClientSender) -> Self {
        Self {
            db: SalsaDb::default(),
            document_map: Arc::new(DocumentMap::new()),
            runtime_settings: LspRuntimeSettings::default(),
            workspace_folders: Vec::new(),
            watched_config_files: HashSet::new(),
            config_error_reports: HashMap::new(),
            diagnostics: DiagnosticCollection::default(),
            settle_deadline: None,
            lint_generation: 0,
            last_applied_lint_generation: 0,
            external_pending: HashSet::new(),
            supports_pull_diagnostics: false,
            supports_related_documents: false,
            harvest_cycle: None,
            harvest_cycle_counter: 0,
            sender,
        }
    }

    /// Start a harvest cycle, returning its id: from here until
    /// [`Self::end_harvest_cycle`], direct disk-syncs record their paths so a
    /// staler in-flight batch can't regress them, and every harvester request
    /// carries the id so only this cycle's batches apply.
    pub(crate) fn begin_harvest_cycle(&mut self) -> u64 {
        self.harvest_cycle_counter += 1;
        let id = self.harvest_cycle_counter;
        self.harvest_cycle = Some(HarvestCycle {
            id,
            requested: HashMap::new(),
            shield: HashSet::new(),
            last_progress: Instant::now(),
        });
        id
    }

    /// End the harvest cycle (completed or aborted), dropping its request
    /// stamps and shield. Called by [`Self::complete_settle`]; abort paths
    /// call it directly. A batch still in flight for the ended cycle is
    /// rejected by its id.
    pub(crate) fn end_harvest_cycle(&mut self) {
        self.harvest_cycle = None;
    }

    /// When the in-flight cycle last made progress (started, or applied a
    /// batch) plus [`HARVEST_CYCLE_TIMEOUT`]: the instant after which the
    /// writer assumes the cycle's outstanding batch was lost. `None` when no
    /// cycle is in flight. The writer thread folds this into its wait
    /// deadline, so a stalled cycle is detected even when no settle deadline
    /// is armed (an idle session must still recover).
    pub(crate) fn harvest_expiry(&self) -> Option<Instant> {
        self.harvest_cycle
            .as_ref()
            .map(|cycle| cycle.last_progress + HARVEST_CYCLE_TIMEOUT)
    }

    /// Record that `path`'s cached text now reflects the disk (a direct
    /// watcher sync or referenced-file reload), so an in-flight harvest batch
    /// — read before this write — must not apply to it. No-op outside a cycle.
    pub(crate) fn shield_from_harvest(&mut self, path: &std::path::Path) {
        if let Some(cycle) = &mut self.harvest_cycle {
            cycle.shield.insert(path.to_owned());
        }
    }

    /// Every open document's salsa inputs and backing path (documents without
    /// a backing path are skipped).
    pub(crate) fn open_documents(
        &self,
    ) -> Vec<(crate::salsa::FileText, crate::salsa::FileConfig, PathBuf)> {
        self.document_map()
            .values()
            .filter_map(|state| Some((state.salsa_file, state.salsa_config, state.path.clone()?)))
            .collect()
    }

    /// The backing paths of all open documents. An open path has
    /// buffer-authoritative content: it must never be read from (or synced
    /// to) disk, or an unsaved edit would be clobbered.
    pub(crate) fn open_document_paths(&self) -> HashSet<PathBuf> {
        self.document_map()
            .values()
            .filter_map(|state| state.path.clone())
            .collect()
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

    /// The client channel, for write handlers that log or toast.
    pub(crate) fn sender(&self) -> &ClientSender {
        &self.sender
    }

    /// The client-pushed runtime settings.
    pub(crate) fn runtime_settings(&self) -> &LspRuntimeSettings {
        &self.runtime_settings
    }

    /// Mutable access to the runtime settings (initialize, configuration push).
    pub(crate) fn runtime_settings_mut(&mut self) -> &mut LspRuntimeSettings {
        &mut self.runtime_settings
    }

    /// Mint a complete [`StateSnapshot`] from writer-owned state.
    pub(crate) fn mint_snapshot(&self) -> StateSnapshot {
        // Test hook for the writer loop's mint-panic answer path.
        #[cfg(test)]
        if PANIC_ON_NEXT_MINT_SNAPSHOT.swap(false, std::sync::atomic::Ordering::SeqCst) {
            panic!("test-injected mint panic");
        }
        StateSnapshot::assemble(
            self.analysis(),
            self.document_map_arc(),
            self.workspace_folders.clone(),
            self.diagnostics.shared(),
            self.supports_pull_diagnostics,
            self.supports_related_documents,
        )
    }

    /// Apply one database-mutating notification against writer-owned state,
    /// including its side effects (settle arming, external marking, immediate
    /// diagnostics drops) — all writer-owned, so handlers call them directly
    /// on `self`.
    ///
    /// This is the function the writer thread runs per received command; inline
    /// mode calls it synchronously via [`WriterHandle::forward_write`].
    pub(crate) fn apply_write(&mut self, cmd: WriteCommand) {
        use crate::lsp::{documents, handlers};

        match cmd {
            WriteCommand::DidOpen(params) => documents::did_open(self, params),
            WriteCommand::DidChange(params) => documents::did_change(self, params),
            WriteCommand::DidSave(params) => documents::did_save(self, params),
            WriteCommand::DidClose(params) => documents::did_close(self, params),
            WriteCommand::DidChangeConfiguration(params) => {
                handlers::configuration::did_change_configuration(self, params)
            }
            WriteCommand::DidChangeWatchedFiles(params) => {
                handlers::file_watcher::did_change_watched_files(self, params)
            }
            WriteCommand::DidChangeWorkspaceFolders(params) => {
                handlers::workspace_folders::did_change_workspace_folders(self, params)
            }
            WriteCommand::DidCreateFiles(params) => {
                handlers::file_operations::did_create_files(self, params)
            }
            WriteCommand::DidRenameFiles(params) => {
                handlers::file_operations::did_rename_files(self, params)
            }
            WriteCommand::DidDeleteFiles(params) => {
                handlers::file_operations::did_delete_files(self, params)
            }
            #[cfg(test)]
            WriteCommand::PanicForTest => panic!("writer test panic"),
        }
    }

    /// Drop `uri` from the diagnostics store immediately (closed/deleted
    /// documents), ahead of the settle's clear-on-fix diff.
    pub(crate) fn drop_diagnostics(&mut self, uri: &Uri) {
        self.diagnostics
            .drop_uri(uri, &self.sender, self.supports_pull_diagnostics);
    }

    /// Arm (or push out) the single workspace settle timer. All lint dispatch
    /// funnels through here so the expensive all-docs re-lint runs once, at a
    /// quiescent point after the edit burst's writes have settled (rust-analyzer
    /// recomputes diagnostics the same way, after `process_changes`).
    pub(crate) fn arm_settle(&mut self) {
        self.settle_deadline = Some(Instant::now() + DIAGNOSTICS_DEBOUNCE);
    }

    /// Arm the settle timer and mark `uri` as needing external linters on the
    /// next pass (the on-open/-save/referenced-file-change signal).
    pub(crate) fn arm_settle_external(&mut self, uri: Uri) {
        self.external_pending.insert(uri);
        self.arm_settle();
    }

    /// The armed settle deadline, if any. The writer thread's `recv_timeout`
    /// watches this; inline mode feeds it into the main loop's `select!` timeout.
    pub(crate) fn settle_deadline(&self) -> Option<Instant> {
        self.settle_deadline
    }

    /// Pull an armed settle deadline up to *now* (test harness: makes the next
    /// dispatch immediate without waiting out the debounce window).
    pub(crate) fn expedite_settle(&mut self) {
        if self.settle_deadline.is_some() {
            self.settle_deadline = Some(Instant::now());
        }
    }

    /// Whether a dispatched settle pass has not yet landed back in the store.
    /// The test harness's `pump` uses this to keep draining; production blocks
    /// on the task channel instead and never needs it.
    pub(crate) fn settle_in_flight(&self) -> bool {
        self.last_applied_lint_generation != self.lint_generation
    }

    /// Push an armed settle deadline out to `deadline` (no-op when none is
    /// armed). Harvest-cycle bookkeeping: an in-flight cycle's completion pass
    /// covers every write applied so far, so a deadline firing mid-cycle only
    /// needs to re-fire at the cycle's expiry — where a cycle whose batch was
    /// lost is detected and aborted instead of swallowing settles forever.
    pub(crate) fn defer_settle_until(&mut self, deadline: Instant) {
        if self.settle_deadline.is_some() {
            self.settle_deadline = Some(deadline);
        }
    }

    /// Clear the settle deadline if it has elapsed. `true` means a settle
    /// should begin now.
    pub(crate) fn take_due_settle(&mut self) -> bool {
        let Some(deadline) = self.settle_deadline else {
            return false;
        };
        if deadline > Instant::now() {
            return false;
        }
        self.settle_deadline = None;
        true
    }

    /// If the settle deadline has elapsed, run the settle's **write phase**
    /// synchronously and prepare its read pass: load every open document's
    /// referenced files (includes/bibliographies) so the pass observes fresh
    /// content, then mint the snapshot the pass runs over. Returns `None` when
    /// no settle is due.
    ///
    /// Inline mode only: the write phase is disk I/O on the calling thread
    /// (timed so a perceptible stall names its own culprit in the log). The
    /// writer thread splits it instead — [`Self::harvest_round`] discovers,
    /// the harvester thread reads, [`Self::apply_harvest`] applies — so the
    /// disk never blocks queued writes and reads.
    pub(crate) fn begin_due_settle(&mut self) -> Option<PreparedSettle> {
        if !self.take_due_settle() {
            return None;
        }

        let reload_start = Instant::now();
        crate::lsp::documents::reload_open_documents_referenced_files(self);
        let reload_elapsed = reload_start.elapsed();
        if reload_elapsed >= Duration::from_millis(50) {
            log::warn!(
                "settle write-phase (referenced-file reload) blocked {reload_elapsed:?} for {} open doc(s)",
                self.document_map().len()
            );
        }

        Some(self.complete_settle())
    }

    /// Finish a settle's write phase: bump the lint generation, take the
    /// pending externals, and mint the snapshot its read pass runs over.
    ///
    /// Also clears the settle deadline: every write applied before this mint is
    /// covered by the pass it feeds, so a deadline those writes armed (possible
    /// on the writer thread, where writes interleave with an in-flight harvest
    /// cycle) would only schedule a redundant re-lint.
    pub(crate) fn complete_settle(&mut self) -> PreparedSettle {
        self.end_harvest_cycle();
        self.settle_deadline = None;
        self.lint_generation += 1;
        let generation = self.lint_generation;
        let external = self.external_pending.clone();
        let uris: Vec<Uri> = self
            .document_map()
            .keys()
            .filter_map(|key| key.parse::<Uri>().ok())
            .collect();
        PreparedSettle {
            generation,
            external,
            snap: self.mint_snapshot(),
            uris,
        }
    }

    /// One discovery round of the settle harvest: compute every open document's
    /// referenced set (interning new paths so `project_graph` re-runs — db
    /// work, no disk), and return the paths the harvester should read this
    /// round — referenced, not open in the editor (buffer-authoritative
    /// content must never be clobbered from disk), and not already requested
    /// this cycle for the same input (the cycle's request map accumulates
    /// across rounds, so the cycle terminates: each round only adds
    /// newly-discovered paths or paths whose input was replaced since their
    /// stamp was taken — evicted and re-interned mid-cycle, whose fresh
    /// `None` input only a re-read can populate).
    ///
    /// Returns an empty round when no cycle is in flight (the guard-abort
    /// path may have torn it down).
    ///
    /// Reading every referenced file once per settle is the self-heal for
    /// clients whose file watching is incomplete — nvim emits no watch event
    /// for a bibliography open in an unrelated buffer — mirroring the
    /// synchronous reload's resync (see
    /// [`documents::reload_open_documents_referenced_files`](crate::lsp::documents::reload_open_documents_referenced_files)).
    pub(crate) fn harvest_round(&mut self) -> Vec<PathBuf> {
        // Take/put-back: the round mutates both the cycle's request map and
        // the database, which a field borrow can't split. A panic mid-round
        // (caught by the loop's `guard`) leaves no cycle behind, so the abort
        // path starts from a clean slate.
        let Some(mut cycle) = self.harvest_cycle.take() else {
            return Vec::new();
        };
        let open_docs = self.open_documents();
        let open_paths = self.open_document_paths();
        let mut tracked: HashSet<PathBuf> = HashSet::new();
        for (salsa_file, salsa_config, path) in open_docs {
            tracked.extend(
                self.db_mut()
                    .discover_referenced_files(salsa_file, salsa_config, path),
            );
        }
        let round = tracked
            .into_iter()
            .filter(|path| !open_paths.contains(path))
            .filter(|path| {
                // Stamp the request with the input handle it reads for; the
                // stamp is what [`Self::apply_harvest`] checks to reject a
                // batch entry whose input was evicted since. Re-request a
                // path whose input was replaced (evicted and re-interned
                // mid-cycle): its stamp no longer matches.
                let Some(handle) = self.db().file_text_if_cached(path) else {
                    // Discovery interned every tracked path; unreachable.
                    return false;
                };
                cycle.requested.insert(path.clone(), handle) != Some(handle)
            })
            .collect();
        self.harvest_cycle = Some(cycle);
        round
    }

    /// Apply one harvested batch: for each `(path, content)` the harvester
    /// read, populate an absent input or refresh a changed cached one
    /// ([`SalsaDb::apply_harvested_file_text`]).
    ///
    /// Returns `false` — dropping the batch wholesale — when `cycle_id` does
    /// not name the in-flight cycle: a batch from a superseded cycle (aborted
    /// by the lost-batch backstop while its read was merely slow) predates
    /// every write the current cycle covers, and applying it would regress
    /// fresh inputs and steal the current cycle's completion.
    ///
    /// Per entry, the stale-input guard runs first: the path's live input
    /// must still be the one stamped at request time. Evicted since
    /// (`did_close`) means the input is gone; evicted *and re-interned*
    /// (`did_delete_files`, rename) means a different handle holding fresh
    /// `None` text. Either way the batch content predates the eviction and
    /// must not resurrect it — and no bookkeeping is needed here, because
    /// [`Self::harvest_round`] re-requests a replaced input via its stamp.
    ///
    /// A path opened as a document since the harvest was requested is skipped
    /// (buffer-authoritative), as is an unreadable file (`None` content: a
    /// missing file keeps its last-known content rather than being wiped) and
    /// a path a mid-cycle direct sync already made disk-fresh (the batch
    /// content predates that sync — see [`Self::shield_from_harvest`]).
    pub(crate) fn apply_harvest(
        &mut self,
        cycle_id: u64,
        batch: Vec<(PathBuf, Option<String>)>,
    ) -> bool {
        let Some(mut cycle) = self.harvest_cycle.take() else {
            return false;
        };
        if cycle.id != cycle_id {
            self.harvest_cycle = Some(cycle);
            return false;
        }
        // Test hook for the writer loop's guard-abort path: the panic unwinds
        // past the put-back, so no cycle survives it (as a real apply bug
        // would behave).
        #[cfg(test)]
        if PANIC_ON_NEXT_HARVEST_APPLY.swap(false, std::sync::atomic::Ordering::SeqCst) {
            panic!("test-injected harvest apply panic");
        }
        // The batch is this cycle's progress signal: reset the lost-batch
        // backstop so a healthy multi-round cycle (deep include chains, slow
        // disks) is measured per round, not against its total age.
        cycle.last_progress = Instant::now();
        let open_paths = self.open_document_paths();
        for (path, content) in batch {
            if cycle.requested.get(&path).copied() != self.db().file_text_if_cached(&path) {
                continue;
            }
            if open_paths.contains(&path) || cycle.shield.contains(&path) {
                continue;
            }
            let Some(content) = content else {
                continue;
            };
            self.db_mut().apply_harvested_file_text(&path, content);
        }
        self.harvest_cycle = Some(cycle);
        true
    }

    /// Apply one settle pass result to the store. Returns `true` when the pass
    /// was current and applied (the caller should nudge pull clients to
    /// re-pull); a pass superseded by a newer settle is dropped wholesale — no
    /// delivery, no clear, no external retirement.
    pub(crate) fn apply_settle_result(
        &mut self,
        generation: u64,
        publishes: Vec<(Uri, Option<i32>, Vec<Diagnostic>)>,
        external_ran: HashSet<Uri>,
    ) -> bool {
        if generation != self.lint_generation {
            return false;
        }
        // Record that this generation's pass has landed, so a waiter (the test
        // harness's `pump`) can tell the settle is no longer in flight.
        self.last_applied_lint_generation = generation;
        // Hand the complete merged set to the collection: it upserts the URIs
        // whose diagnostics changed, leaves unchanged ones untouched (no
        // redundant push, stable pull `result_id`), and clears every URI the
        // previous settle held but this one omits — clear-on-fix for fixed
        // manifests, closed docs, and resolved cross-file diagnostics alike. A
        // shared `_quarto.yml` stays flagged as long as any open doc still
        // reports it.
        self.diagnostics
            .apply(publishes, &self.sender, self.supports_pull_diagnostics);
        // Retire exactly the externals this pass ran (a save queued after
        // dispatch stays pending for the next settle).
        self.external_pending
            .retain(|uri| !external_ran.contains(uri));
        true
    }

    /// Record the client's pull-diagnostics capabilities (set once at
    /// `initialize`, pre-spawn).
    pub(crate) fn set_pull_capabilities(&mut self, pull: bool, related: bool) {
        self.supports_pull_diagnostics = pull;
        self.supports_related_documents = related;
    }

    /// Whether the client is served via pull diagnostics (push suppressed).
    pub(crate) fn supports_pull_diagnostics(&self) -> bool {
        self.supports_pull_diagnostics
    }

    /// The stored diagnostics collection (test-only inspection).
    #[cfg(test)]
    pub(crate) fn diagnostics(&self) -> &DiagnosticCollection {
        &self.diagnostics
    }

    /// The pending-externals set (test-only inspection).
    #[cfg(test)]
    pub(crate) fn external_pending(&self) -> &HashSet<Uri> {
        &self.external_pending
    }

    /// Force the lint generation (test-only: simulates prior settles).
    #[cfg(test)]
    pub(crate) fn set_lint_generation(&mut self, generation: u64) {
        self.lint_generation = generation;
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

/// Test hook: makes the next [`WriterState::apply_harvest`] panic, exercising
/// the writer loop's guard-abort path.
#[cfg(test)]
static PANIC_ON_NEXT_HARVEST_APPLY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Test hook: makes the next [`WriterState::mint_snapshot`] panic, exercising
/// the writer loop's answer-the-request path for forwarded reads.
#[cfg(test)]
static PANIC_ON_NEXT_MINT_SNAPSHOT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// One in-flight settle harvest cycle: the writer discovers referenced paths
/// ([`WriterState::harvest_round`]), the harvester thread reads them, and the
/// batch comes back tagged with `id` to be applied
/// ([`WriterState::apply_harvest`]). All of the cycle's staleness bookkeeping
/// lives here so beginning/ending a cycle is one assignment — there is no
/// second flag to keep in lockstep.
struct HarvestCycle {
    /// This cycle's id (from `harvest_cycle_counter`); batches carrying any
    /// other id are stale and dropped.
    id: u64,

    /// Paths requested this cycle, each stamped with the [`FileText`] input
    /// handle it was read for. Accumulates across rounds (which is what makes
    /// the cycle terminate); a live input differing from its stamp means the
    /// path was evicted (and possibly re-interned) since the read was
    /// requested, so the batch entry is stale and the path needs a re-read.
    ///
    /// [`FileText`]: crate::salsa::FileText
    requested: HashMap<PathBuf, crate::salsa::FileText>,

    /// Paths whose cached text a mid-cycle direct write made disk-fresh
    /// (watcher sync, referenced-file reload or load). The in-flight batch
    /// read those paths *before* that write, so a differing batch entry is
    /// stale: applying it would regress the input and the settle pass would
    /// compute over the older content.
    shield: HashSet<PathBuf>,

    /// When the cycle last made progress (began, or applied a batch); the
    /// lost-batch backstop fires [`HARVEST_CYCLE_TIMEOUT`] after this.
    last_progress: Instant,
}

/// A due settle's write phase completed and its read pass prepared: the
/// generation tag, the externals to run, the snapshot the pass reads, and the
/// open-document URIs it lints. Handed to
/// [`settle_task`](crate::lsp::dispatch::settle_task) by both the inline
/// dispatcher and the writer thread.
pub(crate) struct PreparedSettle {
    pub(crate) generation: u64,
    pub(crate) external: HashSet<Uri>,
    pub(crate) snap: StateSnapshot,
    pub(crate) uris: Vec<Uri>,
}

/// Which worker pool a [`ReadJob`] runs on: the shared `Main` pool for
/// interactive reads, or the single-thread `Fmt` pool that isolates slow
/// external formatters from hover/completion latency.
pub(crate) enum ReadPool {
    Main,
    Fmt,
}

/// A pooled read forwarded to the writer: the writer mints the snapshot (it
/// owns the db, the document map, and the diagnostics store) and hands `run`
/// to the requested pool.
pub(crate) struct ReadJob {
    pub(crate) pool: ReadPool,
    /// The request this read answers. Its id is already in the dispatcher's
    /// `in_flight` set, so if the job cannot run (a snapshot-mint panic), the
    /// writer must answer it with an error — the client would otherwise wait
    /// on it forever.
    pub(crate) id: lsp_server::RequestId,
    pub(crate) run: Box<dyn FnOnce(StateSnapshot) + Send>,
}

/// Spawn handles onto the main loop's task pools, given to the writer thread so
/// it can dispatch read work without owning the pools.
pub(crate) struct PoolSpawners {
    pub(crate) main: TaskSpawner,
    pub(crate) fmt: TaskSpawner,
}

/// A message from the main loop to the writer thread.
enum WriterMsg {
    Write(WriteCommand),
    Read(ReadJob),
    /// A completed settle pass, forwarded back by the main loop's `on_task`
    /// (the pool posts results on the task channel; routing them through the
    /// main loop spares the writer a self-referential sender that would keep
    /// its own channel from ever disconnecting on shutdown).
    SettleResult {
        generation: u64,
        publishes: Vec<(Uri, Option<i32>, Vec<Diagnostic>)>,
        external_ran: HashSet<Uri>,
    },
    /// One harvested batch of referenced-file contents, forwarded back by the
    /// main loop's `on_task` (same routing rationale as `SettleResult`: the
    /// harvester posts on the task channel, not into the writer's own channel).
    /// `cycle` echoes the id the request carried, so a batch from a
    /// superseded cycle is rejected instead of misattributed.
    Harvested {
        cycle: u64,
        batch: Vec<(PathBuf, Option<String>)>,
    },
}

/// The main loop's handle to the writer state; see the module docs for the
/// inline/threaded split.
pub(crate) struct WriterHandle {
    mode: WriterMode,
}

enum WriterMode {
    /// State lives here, mutated synchronously on the calling thread. The
    /// `LspTester` harness stays in this mode forever — never delete this path.
    Inline(Box<WriterState>),
    /// State lives on the `panache-lsp-writer` thread; `tx` is the only way in.
    Threaded { tx: Sender<WriterMsg> },
}

impl WriterHandle {
    /// A fresh inline-mode writer over a default (empty) database.
    pub(crate) fn new(sender: ClientSender) -> Self {
        Self {
            mode: WriterMode::Inline(Box::new(WriterState::new(sender))),
        }
    }

    /// Whether the state has moved onto the writer thread.
    pub(crate) fn is_threaded(&self) -> bool {
        matches!(self.mode, WriterMode::Threaded { .. })
    }

    /// A threaded-mode handle whose writer thread is already gone (tests
    /// only): every forward fails, exercising the closed-channel paths.
    #[cfg(test)]
    pub(crate) fn threaded_disconnected() -> Self {
        let (tx, _) = crossbeam_channel::unbounded();
        Self {
            mode: WriterMode::Threaded { tx },
        }
    }

    /// Direct access to the state. Inline mode only: panics after
    /// [`spawn`](Self::spawn), when the state lives on the writer thread.
    pub(crate) fn state(&self) -> &WriterState {
        match &self.mode {
            WriterMode::Inline(state) => state,
            WriterMode::Threaded { .. } => {
                panic!("writer state accessed on the main loop after spawn")
            }
        }
    }

    /// Mutable access to the state; same inline-mode-only contract as
    /// [`state`](Self::state).
    pub(crate) fn state_mut(&mut self) -> &mut WriterState {
        match &mut self.mode {
            WriterMode::Inline(state) => state,
            WriterMode::Threaded { .. } => {
                panic!("writer state accessed on the main loop after spawn")
            }
        }
    }

    /// Move the state onto the dedicated writer thread. From here on, writes,
    /// reads, and settles must be forwarded; direct state access panics.
    ///
    /// The `JoinHandle` is deliberately dropped (the thread is detached): the
    /// thread exits when the channel disconnects — i.e. when this handle (and
    /// thus `GlobalState`) drops — the same lifecycle as the pool workers, and
    /// what lets the client connection's sender count reach zero on shutdown.
    pub(crate) fn spawn(&mut self, pools: PoolSpawners, task_tx: Sender<Task>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        let prev = std::mem::replace(&mut self.mode, WriterMode::Threaded { tx });
        let WriterMode::Inline(mut state) = prev else {
            panic!("writer thread already spawned");
        };
        // The writer must not carry a direct clone of the connection's sender
        // across the spawn: `io_threads.join()` waits for every clone, and a
        // writer wedged in a blocking salsa write (it only observes shutdown
        // in `recv`) would hold its clone past `GlobalState`'s drop, leaving a
        // zombie process after the client disconnects. Its client traffic
        // rides the task channel instead ([`Task::SendToClient`]).
        state.sender = ClientSender::relayed(task_tx.clone());
        std::thread::Builder::new()
            .name("panache-lsp-writer".to_owned())
            .spawn(move || writer_thread(*state, rx, pools, task_tx))
            .expect("failed to spawn LSP writer thread");
    }

    /// Route a write to the state: applied now (with its side effects) in
    /// inline mode, forwarded to the writer thread in threaded mode. Either
    /// way nothing comes back — the writer owns every effect target.
    pub(crate) fn forward_write(&mut self, cmd: WriteCommand) {
        match &mut self.mode {
            WriterMode::Inline(state) => state.apply_write(cmd),
            WriterMode::Threaded { tx } => {
                if tx.send(WriterMsg::Write(cmd)).is_err() {
                    log::warn!("LSP writer channel closed; dropping write");
                }
            }
        }
    }

    /// Route a completed settle pass to the store owner. Inline: applied now;
    /// returns whether the pass was current (the caller sends the refresh
    /// nudge). Threaded: forwarded; the nudge returns later as
    /// [`Task::RefreshDiagnostics`], so this returns `false`.
    pub(crate) fn forward_settle_result(
        &mut self,
        generation: u64,
        publishes: Vec<(Uri, Option<i32>, Vec<Diagnostic>)>,
        external_ran: HashSet<Uri>,
    ) -> bool {
        match &mut self.mode {
            WriterMode::Inline(state) => {
                state.apply_settle_result(generation, publishes, external_ran)
            }
            WriterMode::Threaded { tx } => {
                if tx
                    .send(WriterMsg::SettleResult {
                        generation,
                        publishes,
                        external_ran,
                    })
                    .is_err()
                {
                    log::warn!("LSP writer channel closed; dropping settle result");
                }
                false
            }
        }
    }

    /// Route a harvested batch of referenced-file contents back to the writer
    /// thread. Threaded mode only in practice — the harvester exists only
    /// there — so an inline arrival is a routing bug; log and drop.
    pub(crate) fn forward_harvest(&mut self, cycle: u64, batch: Vec<(PathBuf, Option<String>)>) {
        match &mut self.mode {
            WriterMode::Inline(_) => {
                log::warn!("harvest batch arrived in inline writer mode; dropping");
            }
            WriterMode::Threaded { tx } => {
                if tx.send(WriterMsg::Harvested { cycle, batch }).is_err() {
                    log::warn!("LSP writer channel closed; dropping harvest batch");
                }
            }
        }
    }

    /// Forward a pooled read to the writer thread, which mints the snapshot.
    /// Threaded mode only: inline callers mint snapshots synchronously and
    /// spawn onto the pools themselves.
    ///
    /// Returns `false` when the writer thread is gone (channel closed) and the
    /// job was dropped — the caller must answer the request itself (an id
    /// already in `in_flight` would otherwise never receive any response).
    #[must_use]
    pub(crate) fn submit_read(&self, job: ReadJob) -> bool {
        let WriterMode::Threaded { tx } = &self.mode else {
            panic!("submit_read is threaded-mode-only");
        };
        if tx.send(WriterMsg::Read(job)).is_err() {
            log::warn!("LSP writer channel closed; dropping read");
            return false;
        }
        true
    }

    // --- inline-mode convenience delegates (main loop pre-spawn + tests) ---

    /// Shared read access to the database (inline mode only).
    pub(crate) fn db(&self) -> &SalsaDb {
        self.state().db()
    }

    /// Shared read access to the open-document map (inline mode only).
    pub(crate) fn document_map(&self) -> &DocumentMap {
        self.state().document_map()
    }

    /// The client-pushed runtime settings (inline mode only).
    pub(crate) fn runtime_settings(&self) -> &LspRuntimeSettings {
        self.state().runtime_settings()
    }

    /// Mutable runtime settings, for `initialize` (pre-spawn).
    pub(crate) fn runtime_settings_mut(&mut self) -> &mut LspRuntimeSettings {
        self.state_mut().runtime_settings_mut()
    }

    /// Set the workspace roots, for `initialize` (pre-spawn).
    pub(crate) fn set_workspace_folders(&mut self, folders: Vec<PathBuf>) {
        self.state_mut().set_workspace_folders(folders);
    }
}

/// Spawn a prepared settle's read pass onto the main pool.
fn spawn_settle_pass(pools: &PoolSpawners, task_tx: &Sender<Task>, prepared: PreparedSettle) {
    pools.main.spawn(crate::lsp::dispatch::settle_task(
        prepared.snap,
        prepared.uris,
        prepared.generation,
        prepared.external,
        task_tx.clone(),
    ));
}

/// Complete the pending settle over current state and spawn its read pass: the
/// common tail of every writer-loop arm that ends a cycle without another
/// harvest round (discovery converged, or a guard aborted the round mid-cycle).
fn complete_and_spawn(state: &mut WriterState, pools: &PoolSpawners, task_tx: &Sender<Task>) {
    spawn_settle_pass(pools, task_tx, state.complete_settle());
}

/// Dispatch one harvest round for `cycle`: run discovery (guarded) and either
/// hand the newly-referenced paths to the harvester, complete the settle when
/// discovery converged, or fall back to a synchronous settle if the harvester
/// channel is gone. A discovery panic degrades to a settle over current state —
/// the deadline was consumed when the cycle began, so the pass must still be
/// spawned or the settle's diagnostics silently vanish.
fn dispatch_harvest_round(
    state: &mut WriterState,
    cycle: u64,
    pools: &PoolSpawners,
    harvest_tx: &Sender<(u64, Vec<PathBuf>)>,
    task_tx: &Sender<Task>,
) {
    let Some(round) = guard("harvest discovery", || state.harvest_round()) else {
        complete_and_spawn(state, pools, task_tx);
        return;
    };
    if round.is_empty() {
        complete_and_spawn(state, pools, task_tx);
    } else if harvest_tx.send((cycle, round)).is_err() {
        // Unreachable while the writer loop holds `harvest_tx`, but don't wedge
        // the cycle if it ever happens: fall back to the synchronous reload.
        log::warn!("LSP harvester channel closed; settling synchronously");
        settle_synchronously(state, pools, task_tx);
    }
}

/// How long an in-flight harvest cycle may run before the writer assumes its
/// batch was lost (harvester death, a dropped relay) and aborts the cycle. A
/// `Some` cycle defers every settle deadline, so without this backstop one
/// lost batch would freeze diagnostics for the whole session. Generous: a
/// healthy cycle is disk reads plus two channel hops, well under a second.
const HARVEST_CYCLE_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_millis(500)
} else {
    Duration::from_secs(30)
};

/// The dedicated `panache-lsp-harvester` thread: reads each requested batch of
/// referenced files from disk (the settle write phase's only slow part) and
/// posts the contents on the task channel, from where the main loop forwards
/// them back to the writer as [`WriterMsg::Harvested`]. Owns no state — the
/// writer decides what to read (discovery) and what to keep (compare-and-set).
///
/// Every requested path gets an entry in the posted batch, even if its read
/// panics (that path degrades to `None`, the same "unreadable" signal a genuine
/// read error yields). The per-path guard matters: a batch-wide `catch_unwind`
/// would drop *every* path on a single bad read, and because the writer already
/// stamped them all as requested, the completing cycle would never re-read them
/// — the settle would then report spurious missing-file diagnostics over inputs
/// still interned `None`. A path left `None` here is re-read on the next cycle
/// (a fresh `requested` set).
///
/// Exits when the writer drops its request sender or the task channel closes.
fn harvester_thread(rx: Receiver<(u64, Vec<PathBuf>)>, task_tx: Sender<Task>) {
    for (cycle, paths) in rx {
        let read_start = Instant::now();
        let count = paths.len();
        let batch: Vec<(PathBuf, Option<String>)> = paths
            .into_iter()
            .map(|path| {
                let content = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    std::fs::read_to_string(&path).ok()
                }))
                .unwrap_or_else(|_| {
                    log::error!(
                        "LSP harvester panicked reading {}; treating it as unreadable",
                        path.display()
                    );
                    None
                });
                (path, content)
            })
            .collect();
        log::debug!(
            "settle harvest read {count} referenced file(s) in {:?}",
            read_start.elapsed()
        );
        if task_tx.send(Task::Harvested { cycle, batch }).is_err() {
            break;
        }
    }
}

/// Run one writer-thread step, mapping a panic to `None` so a buggy handler
/// can't take the thread down (mirrors the pool workers' `catch_unwind`).
/// Pre-guard, a handler panic killed the detached writer and zombified the
/// server: every later write was silently dropped and every forwarded read
/// left its request unanswered. The panicking step's partial state is the
/// price of staying up; the error log names the step.
fn guard<T>(what: &str, f: impl FnOnce() -> T) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(panic) => {
            let msg = crate::lsp::helpers::panic_message(panic.as_ref());
            log::error!("LSP writer step ({what}) panicked: {msg}");
            None
        }
    }
}

/// The writer thread's event loop: apply writes (side effects and all — the
/// writer owns every effect target), mint snapshots for reads, self-time the
/// debounced settle (`recv_timeout` against the writer-owned deadline, so no
/// main-loop timer is involved), and apply forwarded settle results to the
/// store, posting the [`Task::RefreshDiagnostics`] nudge for applied ones.
///
/// The settle's write phase runs as a **harvest cycle** so its disk I/O never
/// blocks this thread: per round the writer discovers and interns the
/// referenced set (db work), the harvester thread reads the new paths, and the
/// contents come back as [`WriterMsg::Harvested`] to be compare-and-set
/// applied; rounds repeat until discovery finds nothing new (a freshly loaded
/// include can reference further files), then the read pass is spawned over a
/// snapshot minted at completion — so writes served *during* the cycle are
/// covered by the very pass the cycle feeds.
///
/// Exits when every `WriterMsg` sender drops (server shutdown) or the task
/// channel closes (main loop gone).
fn writer_thread(
    mut state: WriterState,
    rx: Receiver<WriterMsg>,
    pools: PoolSpawners,
    task_tx: Sender<Task>,
) {
    // The harvester lives exactly as long as this thread: `harvest_tx` drops
    // when this function returns, disconnecting the harvester's receiver. Its
    // results ride the task channel (not `rx`), so this thread's own exit
    // condition — `rx` disconnecting — stays intact.
    let (harvest_tx, harvest_rx) = crossbeam_channel::unbounded::<(u64, Vec<PathBuf>)>();
    std::thread::Builder::new()
        .name("panache-lsp-harvester".to_owned())
        .spawn({
            let task_tx = task_tx.clone();
            move || harvester_thread(harvest_rx, task_tx)
        })
        .expect("failed to spawn LSP harvester thread");

    loop {
        // Wait for the next message at most until whichever fires first: the
        // armed settle deadline (a write arriving first pushes it out —
        // debounce) or the in-flight harvest cycle's lost-batch expiry. The
        // expiry must feed the wait directly: the deadline that started the
        // cycle was consumed at dispatch, so an idle session (no later
        // writes) would otherwise block in `recv` forever and a lost batch
        // would freeze that settle's diagnostics.
        let deadline = [state.settle_deadline(), state.harvest_expiry()]
            .into_iter()
            .flatten()
            .min();
        let msg = match deadline {
            Some(deadline) => {
                let timeout = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(timeout) {
                    Ok(msg) => Some(msg),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            None => match rx.recv() {
                Ok(msg) => Some(msg),
                Err(_) => break,
            },
        };
        match msg {
            Some(WriterMsg::Write(cmd)) => {
                let heal = cmd.heal_target();
                if guard("apply_write", || state.apply_write(cmd)).is_none()
                    && let Some((uri, version)) = heal
                {
                    // The handler may have died between its salsa text write
                    // and the tree/version update, leaving reads serving a
                    // tree that no longer matches the text queries compute
                    // over. Rebuild the document from current salsa state.
                    if guard("post-panic document resync", || {
                        crate::lsp::documents::resync_document_after_panic(
                            &mut state, &uri, version,
                        )
                    })
                    .is_none()
                    {
                        // The rebuild hit the same bug; drop the document so
                        // nothing serves torn state. Reopening it heals.
                        log::error!(
                            "dropping document after a failed post-panic resync: {}",
                            uri.as_str()
                        );
                        state.document_map_mut().remove(&uri.to_string());
                        state.drop_diagnostics(&uri);
                    }
                }
            }
            Some(WriterMsg::Read(job)) => {
                // Minting is pure clones, so a panic here means a real bug —
                // but the job's request id is already in the dispatcher's
                // `in_flight` set, so answer it instead of dropping the job
                // (the client would hang on the request forever, unlike the
                // pool path, whose handler panics become `InternalError`).
                let Some(snap) = guard("mint_snapshot", || state.mint_snapshot()) else {
                    let response = lsp_server::Response::new_err(
                        job.id,
                        lsp_server::ErrorCode::InternalError as i32,
                        "internal error: LSP writer failed to mint a snapshot".to_owned(),
                    );
                    if task_tx.send(Task::Response(response)).is_err() {
                        break;
                    }
                    continue;
                };
                let spawner = match job.pool {
                    ReadPool::Main => &pools.main,
                    ReadPool::Fmt => &pools.fmt,
                };
                let run = job.run;
                spawner.spawn(move || run(snap));
            }
            Some(WriterMsg::SettleResult {
                generation,
                publishes,
                external_ran,
            }) => {
                let applied = guard("apply_settle_result", || {
                    state.apply_settle_result(generation, publishes, external_ran)
                });
                if applied == Some(true) && task_tx.send(Task::RefreshDiagnostics).is_err() {
                    break;
                }
            }
            // A harvested batch: apply it, then either request the next round
            // (discovery found new references in the freshly loaded content)
            // or complete the settle and spawn its read pass.
            Some(WriterMsg::Harvested { cycle, batch }) => {
                match guard("harvest apply", || state.apply_harvest(cycle, batch)) {
                    Some(true) => {}
                    Some(false) => {
                        // A batch from a superseded cycle (aborted while its
                        // read was merely slow); the current cycle's own
                        // batch is still due.
                        log::warn!("harvest batch from a superseded cycle; dropped");
                        continue;
                    }
                    None => {
                        // Panic mid-apply (the take/put-back left no cycle
                        // behind). Serve the settle over current state rather
                        // than dropping it: its deadline was consumed when
                        // the cycle began, so with no later write it would
                        // otherwise never fire — a save's external lint
                        // results would silently vanish.
                        complete_and_spawn(&mut state, &pools, &task_tx);
                        continue;
                    }
                }
                dispatch_harvest_round(&mut state, cycle, &pools, &harvest_tx, &task_tx);
            }
            // Deadline elapsed: start the settle's harvest cycle (or finish
            // immediately when there is nothing to read — no open on-disk
            // documents, or every referenced path is an open buffer).
            None => {
                if let Some(expiry) = state.harvest_expiry() {
                    // A cycle is already in flight; the pass it will spawn
                    // reads a snapshot minted at completion, so every write
                    // applied so far — including whichever armed this deadline
                    // — is covered. Defer the deadline to the cycle's expiry
                    // instead of clearing it: a healthy cycle completes first
                    // (completion clears the deadline), while a cycle whose
                    // batch was lost re-fires here and is aborted below.
                    if Instant::now() < expiry {
                        state.defer_settle_until(expiry);
                        continue;
                    }
                    log::warn!(
                        "harvest cycle made no progress for {HARVEST_CYCLE_TIMEOUT:?}; \
                         assuming its batch was lost and settling synchronously"
                    );
                    settle_synchronously(&mut state, &pools, &task_tx);
                    continue;
                }
                if !state.take_due_settle() {
                    continue;
                }
                let cycle = state.begin_harvest_cycle();
                dispatch_harvest_round(&mut state, cycle, &pools, &harvest_tx, &task_tx);
            }
        }
    }
}

/// Abort/fallback path: serve the pending settle via the synchronous
/// referenced-file reload on the writer thread (the pre-harvester behavior),
/// then spawn its read pass. Ends any in-flight cycle first — a batch still
/// in flight for it is rejected by its id.
///
/// The reload runs guarded: it executes a superset of the discovery code the
/// harvest rounds guard (plus on-thread disk reads), so a panic degrades to a
/// pass over current state instead of killing the detached writer thread —
/// which would silently drop every later write for the session.
fn settle_synchronously(state: &mut WriterState, pools: &PoolSpawners, task_tx: &Sender<Task>) {
    state.end_harvest_cycle();
    guard("synchronous settle reload", || {
        crate::lsp::documents::reload_open_documents_referenced_files(state);
    });
    spawn_settle_pass(pools, task_tx, state.complete_settle());
}

#[cfg(test)]
mod tests {
    use salsa::Durability;

    use super::{PoolSpawners, ReadJob, ReadPool, WriterHandle, WriterState};
    use crate::lsp::global_state::{ClientSender, Task};
    use crate::lsp::helpers::catch_cancelled;
    use crate::lsp::task_pool::TaskPool;
    use crate::lsp::writer_command::WriteCommand;

    fn client_sender() -> ClientSender {
        let (tx, _rx) = crossbeam_channel::unbounded();
        ClientSender::new(tx)
    }

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
        let mut writer = WriterState::new(client_sender());
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

    /// The harvest primitives split the settle reload correctly: discovery
    /// requests exactly the referenced non-open paths (once per cycle), the
    /// applied batch resyncs an out-of-band edit into salsa, and a path open
    /// as a document is never read from disk (buffer-authoritative).
    #[test]
    fn harvest_rounds_resync_referenced_files() {
        use crate::lsp::uri_ext::UriExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let doc_path = dir.path().join("main.qmd");
        let bib_path = dir.path().join("refs.bib");
        let doc_text = "---\nbibliography: refs.bib\n---\n\n# Heading\n\nCite [@key].\n";
        std::fs::write(&doc_path, doc_text).expect("write doc");
        std::fs::write(&bib_path, "@article{key, title={One}}\n").expect("write bib");

        let mut state = WriterState::new(client_sender());
        let uri = lsp_types::Uri::from_file_path(&doc_path).expect("uri");
        state.apply_write(WriteCommand::DidOpen(
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri,
                    language_id: "quarto".into(),
                    version: 0,
                    text: doc_text.to_owned(),
                },
            },
        ));

        // Out-of-band edit after `did_open` cached the bibliography.
        std::fs::write(&bib_path, "@article{key, title={Two}}\n").expect("rewrite bib");

        let cycle = state.begin_harvest_cycle();
        let round = state.harvest_round();
        assert!(
            round.contains(&bib_path),
            "referenced bibliography requested: {round:?}"
        );
        assert!(
            !round.contains(&doc_path),
            "open buffer path must never be harvested from disk"
        );

        // Simulate the harvester, then apply.
        let batch: Vec<_> = round
            .iter()
            .map(|path| (path.clone(), std::fs::read_to_string(path).ok()))
            .collect();
        assert!(state.apply_harvest(cycle, batch), "current cycle's batch");

        // Fixpoint: a second round over the same cycle requests nothing new.
        assert!(state.harvest_round().is_empty());

        // The out-of-band edit is now visible in salsa.
        let text = state
            .db()
            .file_text_if_cached(&bib_path)
            .expect("bib cached")
            .content_or_empty(state.db())
            .to_string();
        assert!(text.contains("Two"), "resynced content, got: {text}");
    }

    /// A harvest batch read from disk *before* a direct write landed must not
    /// regress the input: the harvester reads v1, the disk changes to v2, a
    /// watcher event syncs v2 into salsa on the writer — then the v1 batch
    /// arrives (it round-trips through the main loop) and, compare-and-set
    /// seeing v2 != v1, would "refresh" the input back to stale v1. The settle
    /// pass then computes over v1 and the wrong diagnostics persist until an
    /// unrelated event arms a fresh cycle.
    #[test]
    fn stale_harvest_batch_does_not_regress_watcher_synced_content() {
        use crate::lsp::uri_ext::UriExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let doc_path = dir.path().join("main.qmd");
        let bib_path = dir.path().join("refs.bib");
        let doc_text = "---\nbibliography: refs.bib\n---\n\n# Heading\n\nCite [@key].\n";
        std::fs::write(&doc_path, doc_text).expect("write doc");
        std::fs::write(&bib_path, "@article{key, title={One}}\n").expect("write bib");

        let mut state = WriterState::new(client_sender());
        let uri = lsp_types::Uri::from_file_path(&doc_path).expect("uri");
        state.apply_write(WriteCommand::DidOpen(
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri,
                    language_id: "quarto".into(),
                    version: 0,
                    text: doc_text.to_owned(),
                },
            },
        ));

        // Start a harvest cycle and "read" the bibliography as the harvester
        // would — before the disk changes.
        let cycle = state.begin_harvest_cycle();
        state.harvest_round();
        let stale_batch = vec![(
            bib_path.clone(),
            Some(std::fs::read_to_string(&bib_path).expect("read bib")),
        )];

        // The disk changes and a watcher event syncs the new content into
        // salsa on the writer, mid-cycle.
        std::fs::write(&bib_path, "@article{key, title={Two}}\n").expect("edit bib");
        let bib_uri = lsp_types::Uri::from_file_path(&bib_path).expect("bib uri");
        state.apply_write(WriteCommand::DidChangeWatchedFiles(
            lsp_types::DidChangeWatchedFilesParams {
                changes: vec![lsp_types::FileEvent {
                    uri: bib_uri,
                    typ: lsp_types::FileChangeType::CHANGED,
                }],
            },
        ));

        // The stale batch lands last; it must not clobber the fresher sync.
        state.apply_harvest(cycle, stale_batch);
        let text = state
            .db()
            .file_text_if_cached(&bib_path)
            .expect("bib cached")
            .content_or_empty(state.db())
            .to_string();
        assert!(
            text.contains("Two"),
            "watcher-synced content survived the stale batch, got: {text}"
        );
    }

    /// A mid-cycle project load (`did_open`/`did_save` of a document) must
    /// not shield referenced paths it did not actually read:
    /// `load_referenced_files` populates *absent* inputs only, so an
    /// already-cached bibliography keeps its stale content through the load.
    /// Blanket-shielding the whole tracked set would mark that stale input
    /// "disk-fresh" and discard the in-flight harvest batch carrying the real
    /// disk content — the settle pass then lints over the stale text, and
    /// `complete_settle` clears the deadline, so the wrong diagnostics
    /// persist until an unrelated later event.
    #[test]
    fn project_load_shields_only_freshly_read_paths() {
        use crate::lsp::uri_ext::UriExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let a_path = dir.path().join("a.qmd");
        let b_path = dir.path().join("b.qmd");
        let bib_path = dir.path().join("refs.bib");
        let doc_text = "---\nbibliography: refs.bib\n---\n\nCite [@key].\n";
        std::fs::write(&a_path, doc_text).expect("write a");
        std::fs::write(&b_path, doc_text).expect("write b");
        std::fs::write(&bib_path, "@article{key, title={One}}\n").expect("write bib");

        let mut state = WriterState::new(client_sender());
        let open = |path: &std::path::Path| {
            WriteCommand::DidOpen(lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: lsp_types::Uri::from_file_path(path).expect("uri"),
                    language_id: "quarto".into(),
                    version: 0,
                    text: doc_text.to_owned(),
                },
            })
        };
        // Opening A caches the bibliography (v1) from disk.
        state.apply_write(open(&a_path));

        // Out-of-band edit: the disk moves to v2, salsa still holds v1.
        std::fs::write(&bib_path, "@article{key, title={Two}}\n").expect("edit bib");

        // The settle's harvest cycle reads v2 off-thread.
        let cycle = state.begin_harvest_cycle();
        state.harvest_round();
        let batch = vec![(
            bib_path.clone(),
            Some(std::fs::read_to_string(&bib_path).expect("read bib")),
        )];

        // Mid-cycle: B (also referencing refs.bib) opens. Its project load
        // finds the bibliography already cached and does NOT re-read it, so
        // it must not shield it either.
        state.apply_write(open(&b_path));

        // The batch's fresh v2 content must still apply.
        state.apply_harvest(cycle, batch);
        let text = state
            .db()
            .file_text_if_cached(&bib_path)
            .expect("bib cached")
            .content_or_empty(state.db())
            .to_string();
        assert!(
            text.contains("Two"),
            "harvest heal survived the cached-path project load, got: {text}"
        );
    }

    /// A path evicted mid-cycle (`did_close`) and then re-referenced by
    /// another document (`did_change`) must be harvested again in the same
    /// cycle. The batch's content is skipped on apply (the eviction must not
    /// be resurrected), but the cycle's `requested` dedup set would otherwise
    /// suppress the re-read forever: discovery re-interns the path as a fresh
    /// `None` input, the settle pass reports a load error for a file that
    /// exists on disk, and `complete_settle` clears the deadline those writes
    /// armed — the wrong diagnostic persists until an unrelated event.
    #[test]
    fn evicted_then_rereferenced_path_is_harvested_again() {
        use crate::lsp::uri_ext::UriExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let a_path = dir.path().join("a.qmd");
        let b_path = dir.path().join("b.qmd");
        let bib_path = dir.path().join("refs.bib");
        let a_text = "---\nbibliography: refs.bib\n---\n\nCite [@key].\n";
        let b_text = "# Standalone\n";
        std::fs::write(&a_path, a_text).expect("write a");
        std::fs::write(&b_path, b_text).expect("write b");
        std::fs::write(&bib_path, "@article{key, title={One}}\n").expect("write bib");

        let mut state = WriterState::new(client_sender());
        let open = |path: &std::path::Path, text: &str| {
            WriteCommand::DidOpen(lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: lsp_types::Uri::from_file_path(path).expect("uri"),
                    language_id: "quarto".into(),
                    version: 0,
                    text: text.to_owned(),
                },
            })
        };
        state.apply_write(open(&a_path, a_text));
        state.apply_write(open(&b_path, b_text));

        // Start the cycle: the bibliography is requested and "read".
        let cycle = state.begin_harvest_cycle();
        let first = state.harvest_round();
        assert!(
            first.contains(&bib_path),
            "cycle requests the referenced bibliography: {first:?}"
        );
        let batch = vec![(
            bib_path.clone(),
            Some(std::fs::read_to_string(&bib_path).expect("read bib")),
        )];

        // Mid-cycle: close A (evicts the bib — B does not reference it yet),
        // then B starts referencing it.
        state.apply_write(WriteCommand::DidClose(
            lsp_types::DidCloseTextDocumentParams {
                text_document: lsp_types::TextDocumentIdentifier {
                    uri: lsp_types::Uri::from_file_path(&a_path).expect("uri"),
                },
            },
        ));
        assert!(
            state.db().file_text_if_cached(&bib_path).is_none(),
            "closing A must evict the bibliography"
        );
        let b_new = "---\nbibliography: refs.bib\n---\n\nCite [@key].\n";
        state.apply_write(WriteCommand::DidChange(
            lsp_types::DidChangeTextDocumentParams {
                text_document: lsp_types::VersionedTextDocumentIdentifier {
                    uri: lsp_types::Uri::from_file_path(&b_path).expect("uri"),
                    version: 1,
                },
                content_changes: vec![lsp_types::TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: b_new.to_owned(),
                }],
            },
        ));

        // The batch applies to an evicted path: skipped, and the next round
        // re-requests it for B (its stamp no longer matches the fresh input).
        state.apply_harvest(cycle, batch);
        let next = state.harvest_round();
        assert!(
            next.contains(&bib_path),
            "re-referenced bibliography must be re-requested: {next:?}"
        );

        // The re-read populates the fresh input; the settle pass sees content.
        state.apply_harvest(
            cycle,
            vec![(
                bib_path.clone(),
                Some(std::fs::read_to_string(&bib_path).expect("read bib")),
            )],
        );
        let text = state
            .db()
            .file_text_if_cached(&bib_path)
            .expect("bib re-interned")
            .content_or_empty(state.db())
            .to_string();
        assert!(text.contains("One"), "bib content loaded, got: {text}");
    }

    /// A late batch from a superseded cycle must be rejected wholesale. The
    /// harvester is single-threaded and FIFO: when a cycle is aborted by the
    /// lost-batch backstop while its read was merely *slow*, the next cycle's
    /// request queues behind it and the old batch is guaranteed to arrive
    /// first, inside the new cycle. Without the id check it would be
    /// misattributed: its stale contents compare-and-set over the abort's
    /// fresh synchronous reload, and the new cycle's genuine batch dropped.
    #[test]
    fn stale_batch_from_superseded_cycle_is_rejected() {
        use crate::lsp::uri_ext::UriExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let doc_path = dir.path().join("main.qmd");
        let bib_path = dir.path().join("refs.bib");
        let doc_text = "---\nbibliography: refs.bib\n---\n\nCite [@key].\n";
        std::fs::write(&doc_path, doc_text).expect("write doc");
        std::fs::write(&bib_path, "@article{key, title={One}}\n").expect("write bib");

        let mut state = WriterState::new(client_sender());
        state.apply_write(WriteCommand::DidOpen(
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: lsp_types::Uri::from_file_path(&doc_path).expect("uri"),
                    language_id: "quarto".into(),
                    version: 0,
                    text: doc_text.to_owned(),
                },
            },
        ));

        // Cycle 1 requests the bibliography; its "read" observes v1.
        let superseded = state.begin_harvest_cycle();
        state.harvest_round();
        let stale_batch = vec![(
            bib_path.clone(),
            Some(std::fs::read_to_string(&bib_path).expect("read bib")),
        )];

        // The backstop aborts cycle 1 and settles synchronously: the disk
        // has moved to v2 and the reload resyncs it.
        state.end_harvest_cycle();
        std::fs::write(&bib_path, "@article{key, title={Two}}\n").expect("edit bib");
        crate::lsp::documents::reload_open_documents_referenced_files(&mut state);

        // Cycle 2 begins; cycle 1's late batch lands first (FIFO harvester).
        let current = state.begin_harvest_cycle();
        state.harvest_round();
        assert!(
            !state.apply_harvest(superseded, stale_batch),
            "a superseded cycle's batch must be dropped"
        );
        let text = state
            .db()
            .file_text_if_cached(&bib_path)
            .expect("bib cached")
            .content_or_empty(state.db())
            .to_string();
        assert!(
            text.contains("Two"),
            "the reload's fresh content survived the stale batch, got: {text}"
        );

        // The current cycle's own batch still applies.
        assert!(state.apply_harvest(
            current,
            vec![(
                bib_path.clone(),
                Some(std::fs::read_to_string(&bib_path).expect("read bib")),
            )],
        ));
    }

    /// An applied batch is the cycle's progress signal: it must push the
    /// lost-batch expiry out, so a healthy multi-round cycle (deep include
    /// chains on a slow disk) is measured per round, not against its total
    /// age — a false abort would run the synchronous reload on the writer
    /// thread against the same slow disk, with a genuine batch still in
    /// flight.
    #[test]
    fn applied_batch_resets_harvest_expiry() {
        let mut state = WriterState::new(client_sender());
        let cycle = state.begin_harvest_cycle();
        let initial = state.harvest_expiry().expect("cycle in flight");
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(state.apply_harvest(cycle, Vec::new()));
        assert!(
            state.harvest_expiry().expect("cycle still in flight") > initial,
            "an applied batch must reset the lost-batch backstop"
        );
    }

    /// The evicted-then-re-referenced re-harvest must survive a mid-cycle
    /// shield on the same path: a watcher sync shields the bibliography
    /// (disk-fresh), then `did_close` evicts it, then another document
    /// re-references it. The batch entry must be recognized as stale (the
    /// input it was read for is gone) even though the path is shielded, and
    /// the next round must re-request it — otherwise the re-interned input
    /// stays `None` and the settle reports a load error for a file that
    /// exists on disk.
    #[test]
    fn shielded_then_evicted_path_is_harvested_again() {
        use crate::lsp::uri_ext::UriExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let a_path = dir.path().join("a.qmd");
        let b_path = dir.path().join("b.qmd");
        let bib_path = dir.path().join("refs.bib");
        let a_text = "---\nbibliography: refs.bib\n---\n\nCite [@key].\n";
        let b_text = "# Standalone\n";
        std::fs::write(&a_path, a_text).expect("write a");
        std::fs::write(&b_path, b_text).expect("write b");
        std::fs::write(&bib_path, "@article{key, title={One}}\n").expect("write bib");

        let mut state = WriterState::new(client_sender());
        let open = |path: &std::path::Path, text: &str| {
            WriteCommand::DidOpen(lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: lsp_types::Uri::from_file_path(path).expect("uri"),
                    language_id: "quarto".into(),
                    version: 0,
                    text: text.to_owned(),
                },
            })
        };
        state.apply_write(open(&a_path, a_text));
        state.apply_write(open(&b_path, b_text));

        let cycle = state.begin_harvest_cycle();
        let first = state.harvest_round();
        assert!(
            first.contains(&bib_path),
            "bibliography requested: {first:?}"
        );
        let batch = vec![(
            bib_path.clone(),
            Some(std::fs::read_to_string(&bib_path).expect("read bib")),
        )];

        // Mid-cycle: a watcher sync shields the bibliography, then A closes
        // (evicting it), then B starts referencing it.
        state.apply_write(WriteCommand::DidChangeWatchedFiles(
            lsp_types::DidChangeWatchedFilesParams {
                changes: vec![lsp_types::FileEvent {
                    uri: lsp_types::Uri::from_file_path(&bib_path).expect("bib uri"),
                    typ: lsp_types::FileChangeType::CHANGED,
                }],
            },
        ));
        state.apply_write(WriteCommand::DidClose(
            lsp_types::DidCloseTextDocumentParams {
                text_document: lsp_types::TextDocumentIdentifier {
                    uri: lsp_types::Uri::from_file_path(&a_path).expect("uri"),
                },
            },
        ));
        assert!(
            state.db().file_text_if_cached(&bib_path).is_none(),
            "closing A must evict the bibliography"
        );
        let b_new = "---\nbibliography: refs.bib\n---\n\nCite [@key].\n";
        state.apply_write(WriteCommand::DidChange(
            lsp_types::DidChangeTextDocumentParams {
                text_document: lsp_types::VersionedTextDocumentIdentifier {
                    uri: lsp_types::Uri::from_file_path(&b_path).expect("uri"),
                    version: 1,
                },
                content_changes: vec![lsp_types::TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: b_new.to_owned(),
                }],
            },
        ));

        // The batch entry is stale (its input was evicted); the shield must
        // not mask that, and the next round must re-request the path.
        state.apply_harvest(cycle, batch);
        let next = state.harvest_round();
        assert!(
            next.contains(&bib_path),
            "re-referenced bibliography must be re-requested despite the shield: {next:?}"
        );
    }

    /// A file deleted mid-cycle must not be resurrected by its in-flight
    /// batch entry. `did_delete_files` evicts *and immediately re-interns*
    /// the path (so `project_graph` re-runs and observes the absence), which
    /// leaves an input that exists with `None` text — indistinguishable, by
    /// state alone, from a pending harvest. The batch content read before
    /// the delete must still be recognized as stale; applying it would pin
    /// pre-delete content at HIGH durability with nothing ever healing it
    /// (the file dropped out of the project graph, so no re-read happens).
    #[test]
    fn deleted_file_is_not_resurrected_by_inflight_batch() {
        use crate::lsp::uri_ext::UriExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let doc_path = dir.path().join("main.qmd");
        let child_path = dir.path().join("child.qmd");
        let doc_text = "# Main\n\n{{< include child.qmd >}}\n";
        std::fs::write(&doc_path, doc_text).expect("write doc");
        std::fs::write(&child_path, "# Child\n").expect("write child");

        let mut state = WriterState::new(client_sender());
        state.apply_write(WriteCommand::DidOpen(
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: lsp_types::Uri::from_file_path(&doc_path).expect("uri"),
                    language_id: "quarto".into(),
                    version: 0,
                    text: doc_text.to_owned(),
                },
            },
        ));

        let cycle = state.begin_harvest_cycle();
        let first = state.harvest_round();
        assert!(
            first.contains(&child_path),
            "included document requested: {first:?}"
        );
        let batch = vec![(
            child_path.clone(),
            Some(std::fs::read_to_string(&child_path).expect("read child")),
        )];

        // Mid-cycle: the file is deleted (explorer operation). The handler
        // evicts and re-interns; the include existence-probe drops the path
        // from the project graph, so the reload does not shield it.
        std::fs::remove_file(&child_path).expect("delete child");
        state.apply_write(WriteCommand::DidDeleteFiles(lsp_types::DeleteFilesParams {
            files: vec![lsp_types::FileDelete {
                uri: lsp_types::Uri::from_file_path(&child_path)
                    .expect("uri")
                    .to_string(),
            }],
        }));

        // The pre-delete batch entry must not repopulate the input.
        state.apply_harvest(cycle, batch);
        let resurrected = state
            .db()
            .file_text_if_cached(&child_path)
            .map(|input| input.content_or_empty(state.db()).to_string());
        assert_eq!(
            resurrected.as_deref(),
            Some(""),
            "deleted file's content must stay absent, got: {resurrected:?}"
        );
    }

    /// After `spawn`, the writer must hold no direct clone of the client
    /// sender: `io_threads.join()` blocks until every clone drops, and a
    /// writer wedged in a blocking salsa write (its only exit signal is
    /// channel disconnect, observed only in `recv`) would hold its clone past
    /// `GlobalState`'s drop — a zombie process after the client disconnects.
    /// Its client traffic must ride the task channel instead.
    #[test]
    fn threaded_writer_relays_client_traffic_via_task_channel() {
        let timeout = std::time::Duration::from_secs(10);
        let (client_tx, client_rx) = crossbeam_channel::unbounded();
        let (task_tx, task_rx) = crossbeam_channel::unbounded::<Task>();
        let pool = TaskPool::new(task_tx.clone(), 1);

        let mut writer = WriterHandle::new(ClientSender::new(client_tx));
        writer.spawn(
            PoolSpawners {
                main: pool.spawner(),
                fmt: pool.spawner(),
            },
            task_tx,
        );

        // `did_open` logs "Opened document" through the writer's sender.
        writer.forward_write(WriteCommand::DidOpen(
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: "file:///nonexistent-spike/doc.qmd".parse().unwrap(),
                    language_id: "quarto".into(),
                    version: 0,
                    text: "# Heading\n".into(),
                },
            },
        ));

        loop {
            match task_rx
                .recv_timeout(timeout)
                .expect("relayed client message")
            {
                Task::SendToClient(lsp_server::Message::Notification(n))
                    if n.method == "window/logMessage" =>
                {
                    break;
                }
                _ => continue,
            }
        }
        assert!(
            client_rx.try_recv().is_err(),
            "the writer must not send directly to the client after spawn"
        );
    }

    /// A lost harvest batch must not wedge diagnostics for the session.
    /// While a cycle is in flight every elapsed settle deadline is covered by
    /// the cycle's completion pass — but if the batch never lands (harvester
    /// death, a dropped relay), that pass never runs, and pre-backstop the
    /// deadline was simply swallowed: no timeout, no retry, diagnostics
    /// frozen until restart. The cycle timeout aborts a stalled cycle and
    /// serves the settle via the synchronous reload.
    #[test]
    fn lost_harvest_batch_recovers_via_cycle_timeout() {
        use crate::lsp::uri_ext::UriExt;

        let timeout = std::time::Duration::from_secs(10);
        let dir = tempfile::tempdir().expect("tempdir");
        let doc_path = dir.path().join("main.qmd");
        let bib_path = dir.path().join("refs.bib");
        let doc_text = "---\nbibliography: refs.bib\n---\n\n# Heading\n\nCite [@key].\n";
        std::fs::write(&doc_path, doc_text).expect("write doc");
        std::fs::write(&bib_path, "@article{key, title={One}}\n").expect("write bib");

        let (task_tx, task_rx) = crossbeam_channel::unbounded::<Task>();
        let pool = TaskPool::new(task_tx.clone(), 1);
        let mut writer = WriterHandle::new(client_sender());
        writer.spawn(
            PoolSpawners {
                main: pool.spawner(),
                fmt: pool.spawner(),
            },
            task_tx,
        );

        let uri = lsp_types::Uri::from_file_path(&doc_path).expect("uri");
        writer.forward_write(WriteCommand::DidOpen(
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "quarto".into(),
                    version: 0,
                    text: doc_text.to_owned(),
                },
            },
        ));

        // The self-timed settle starts a harvest cycle; drop its batch on the
        // floor instead of forwarding it (a lost relay). Relayed client
        // traffic (logs) shares the channel; skip it.
        loop {
            match task_rx.recv_timeout(timeout).expect("harvest batch") {
                Task::Harvested { .. } => break,
                Task::SendToClient(_) => continue,
                _ => panic!("expected the harvest batch first"),
            }
        }

        // A later edit arms a new deadline. Its firing is deferred to the
        // stalled cycle's expiry, which then aborts to the synchronous reload
        // — whose settle pass must land as `Task::Diagnostics`.
        writer.forward_write(WriteCommand::DidChange(
            lsp_types::DidChangeTextDocumentParams {
                text_document: lsp_types::VersionedTextDocumentIdentifier { uri, version: 1 },
                content_changes: vec![lsp_types::TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: format!("{doc_text}\nMore.\n"),
                }],
            },
        ));
        loop {
            match task_rx.recv_timeout(timeout) {
                Ok(Task::Diagnostics { .. }) => break,
                Ok(_) => continue,
                Err(err) => panic!("stalled cycle never recovered: {err}"),
            }
        }
    }

    /// The lost-batch backstop must fire even when the user goes idle after
    /// the settle that started the cycle: that settle's deadline was consumed
    /// at dispatch, so recovery cannot depend on a later write arming a new
    /// one. The cycle's own expiry must feed the writer's wait deadline —
    /// otherwise the loop blocks in `recv` forever and the settle's
    /// diagnostics are never published for the session.
    #[test]
    fn lost_harvest_batch_recovers_without_further_writes() {
        use crate::lsp::uri_ext::UriExt;

        let timeout = std::time::Duration::from_secs(10);
        let dir = tempfile::tempdir().expect("tempdir");
        let doc_path = dir.path().join("main.qmd");
        let bib_path = dir.path().join("refs.bib");
        let doc_text = "---\nbibliography: refs.bib\n---\n\n# Heading\n\nCite [@key].\n";
        std::fs::write(&doc_path, doc_text).expect("write doc");
        std::fs::write(&bib_path, "@article{key, title={One}}\n").expect("write bib");

        let (task_tx, task_rx) = crossbeam_channel::unbounded::<Task>();
        let pool = TaskPool::new(task_tx.clone(), 1);
        let mut writer = WriterHandle::new(client_sender());
        writer.spawn(
            PoolSpawners {
                main: pool.spawner(),
                fmt: pool.spawner(),
            },
            task_tx,
        );

        writer.forward_write(WriteCommand::DidOpen(
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: lsp_types::Uri::from_file_path(&doc_path).expect("uri"),
                    language_id: "quarto".into(),
                    version: 0,
                    text: doc_text.to_owned(),
                },
            },
        ));

        // Drop the cycle's batch on the floor (a lost relay), then go idle:
        // no further writes. The expiry alone must recover the settle.
        loop {
            match task_rx.recv_timeout(timeout).expect("harvest batch") {
                Task::Harvested { .. } => break,
                Task::SendToClient(_) => continue,
                _ => panic!("expected the harvest batch first"),
            }
        }
        loop {
            match task_rx.recv_timeout(timeout) {
                Ok(Task::Diagnostics { .. }) => break,
                Ok(_) => continue,
                Err(err) => panic!("idle session never recovered the lost batch: {err}"),
            }
        }
    }

    /// A panic while applying a harvest batch must not silently drop the
    /// settle the cycle was serving: its deadline was consumed when the
    /// cycle began, so without the abort path completing the settle (over
    /// current state), an idle session would never publish that edit
    /// burst's diagnostics — including any pending external lint results.
    #[test]
    fn panicking_harvest_apply_still_serves_the_settle() {
        use crate::lsp::uri_ext::UriExt;

        let timeout = std::time::Duration::from_secs(10);
        let dir = tempfile::tempdir().expect("tempdir");
        let doc_path = dir.path().join("main.qmd");
        let bib_path = dir.path().join("refs.bib");
        let doc_text = "---\nbibliography: refs.bib\n---\n\n# Heading\n\nCite [@key].\n";
        std::fs::write(&doc_path, doc_text).expect("write doc");
        std::fs::write(&bib_path, "@article{key, title={One}}\n").expect("write bib");

        let (task_tx, task_rx) = crossbeam_channel::unbounded::<Task>();
        let pool = TaskPool::new(task_tx.clone(), 1);
        let mut writer = WriterHandle::new(client_sender());
        writer.spawn(
            PoolSpawners {
                main: pool.spawner(),
                fmt: pool.spawner(),
            },
            task_tx,
        );

        writer.forward_write(WriteCommand::DidOpen(
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: lsp_types::Uri::from_file_path(&doc_path).expect("uri"),
                    language_id: "quarto".into(),
                    version: 0,
                    text: doc_text.to_owned(),
                },
            },
        ));

        // Forward the cycle's batch back with the apply rigged to panic,
        // then go idle: the abort must still complete the settle.
        loop {
            match task_rx.recv_timeout(timeout).expect("harvest batch") {
                Task::Harvested { cycle, batch } => {
                    super::PANIC_ON_NEXT_HARVEST_APPLY
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    writer.forward_harvest(cycle, batch);
                    break;
                }
                Task::SendToClient(_) => continue,
                _ => panic!("expected the harvest batch first"),
            }
        }
        loop {
            match task_rx.recv_timeout(timeout) {
                Ok(Task::Diagnostics { .. }) => break,
                Ok(_) => continue,
                Err(err) => panic!("panic-aborted cycle dropped its settle: {err}"),
            }
        }
    }

    /// A panicking write handler must not take the writer thread down: pool
    /// workers already `catch_unwind` their jobs, and the writer is the same
    /// kind of long-lived executor. Pre-guard, the panic killed the detached
    /// thread and every later write/read was silently dropped — a zombie
    /// server (writes lost, requests never answered) instead of a crash the
    /// editor could detect and restart from.
    #[test]
    fn writer_thread_survives_panicking_write() {
        let timeout = std::time::Duration::from_secs(10);
        let (task_tx, _task_rx) = crossbeam_channel::unbounded::<Task>();
        let pool = TaskPool::new(task_tx.clone(), 1);

        let mut writer = WriterHandle::new(client_sender());
        writer.spawn(
            PoolSpawners {
                main: pool.spawner(),
                fmt: pool.spawner(),
            },
            task_tx,
        );

        writer.forward_write(WriteCommand::PanicForTest);

        // FIFO channel: the read is handled strictly after the panicking
        // write, so a reply proves the thread survived it.
        let (seen_tx, seen_rx) = crossbeam_channel::bounded::<()>(1);
        assert!(
            writer.submit_read(ReadJob {
                pool: ReadPool::Main,
                id: lsp_server::RequestId::from(1),
                run: Box::new(move |_snap| {
                    let _ = seen_tx.send(());
                }),
            }),
            "writer channel must still be open after the panicking write"
        );
        seen_rx
            .recv_timeout(timeout)
            .expect("read ran on a writer thread that survived the panic");
    }

    /// `submit_read` must report a dead writer thread so the dispatcher can
    /// answer the request instead of leaving its id in flight forever.
    #[test]
    fn submit_read_reports_closed_channel() {
        let writer = WriterHandle::threaded_disconnected();
        let delivered = writer.submit_read(ReadJob {
            pool: ReadPool::Main,
            id: lsp_server::RequestId::from(1),
            run: Box::new(|_snap| {}),
        });
        assert!(!delivered, "a closed writer channel must be reported");
    }

    /// A panic mid-`did_change` (after the salsa text write, before the tree
    /// and version update) must not leave the document permanently divergent:
    /// reads would serve a cached tree that no longer matches the text salsa
    /// queries compute over, re-diverging on every keystroke if the panic is
    /// deterministic. The writer loop's heal rebuilds the tree from the
    /// current salsa text (the server's best knowledge of the buffer).
    #[test]
    fn panicking_did_change_heals_the_document() {
        let timeout = std::time::Duration::from_secs(10);
        let (task_tx, task_rx) = crossbeam_channel::unbounded::<Task>();
        let pool = TaskPool::new(task_tx.clone(), 1);
        let mut writer = WriterHandle::new(client_sender());
        writer.spawn(
            PoolSpawners {
                main: pool.spawner(),
                fmt: pool.spawner(),
            },
            task_tx,
        );

        let uri: lsp_types::Uri = "file:///nonexistent-spike/doc.qmd".parse().unwrap();
        writer.forward_write(WriteCommand::DidOpen(
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "quarto".into(),
                    version: 0,
                    text: "# Old\n".into(),
                },
            },
        ));

        // The change's handler dies after pushing the new text into salsa.
        crate::lsp::documents::PANIC_AFTER_DID_CHANGE_TEXT_WRITE
            .store(true, std::sync::atomic::Ordering::SeqCst);
        writer.forward_write(WriteCommand::DidChange(
            lsp_types::DidChangeTextDocumentParams {
                text_document: lsp_types::VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 1,
                },
                content_changes: vec![lsp_types::TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: "# New heading\n".to_owned(),
                }],
            },
        ));

        // FIFO: the read runs strictly after the panicking write (and its
        // heal). The cached tree must describe the new text.
        let (seen_tx, seen_rx) = crossbeam_channel::bounded::<(String, i32)>(1);
        let uri_string = uri.to_string();
        assert!(writer.submit_read(ReadJob {
            pool: ReadPool::Main,
            id: lsp_server::RequestId::from(1),
            run: Box::new(move |snap| {
                let doc = snap
                    .document_map
                    .get(&uri_string)
                    .expect("document still open");
                let tree_text = crate::syntax::SyntaxNode::new_root(doc.tree.clone())
                    .text()
                    .to_string();
                let _ = seen_tx.send((tree_text, doc.version));
            }),
        }));
        let (tree_text, version) = seen_rx
            .recv_timeout(timeout)
            .expect("read ran after the heal");
        assert!(
            tree_text.contains("New heading"),
            "cached tree must be rebuilt from the written text, got: {tree_text:?}"
        );
        assert_eq!(version, 1, "the client's version must be recorded");
        drop(task_rx);
    }

    /// A snapshot-mint panic must answer the forwarded read's request instead
    /// of dropping the job: the dispatcher already put the id in `in_flight`
    /// and delivery succeeded, so no other path will ever respond — the
    /// client would hang on the request forever.
    #[test]
    fn mint_snapshot_panic_answers_the_request() {
        let timeout = std::time::Duration::from_secs(10);
        let (task_tx, task_rx) = crossbeam_channel::unbounded::<Task>();
        let pool = TaskPool::new(task_tx.clone(), 1);
        let mut writer = WriterHandle::new(client_sender());
        writer.spawn(
            PoolSpawners {
                main: pool.spawner(),
                fmt: pool.spawner(),
            },
            task_tx,
        );

        super::PANIC_ON_NEXT_MINT_SNAPSHOT.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(writer.submit_read(ReadJob {
            pool: ReadPool::Main,
            id: lsp_server::RequestId::from(7),
            run: Box::new(|_snap| panic!("the job must not run without a snapshot")),
        }));

        loop {
            match task_rx.recv_timeout(timeout) {
                Ok(Task::Response(response)) => {
                    assert_eq!(response.id, lsp_server::RequestId::from(7));
                    assert!(
                        response.error.is_some(),
                        "the request must be answered with an error: {response:?}"
                    );
                    break;
                }
                Ok(_) => continue,
                Err(err) => panic!("mint panic left the request unanswered: {err}"),
            }
        }
    }

    /// End-to-end smoke test of threaded mode: a forwarded `didOpen` write
    /// applies on the writer thread (no effects round-trip — the writer owns
    /// the settle machinery it arms); a forwarded read observes the written
    /// document through a writer-minted snapshot; the debounced settle
    /// self-fires on the writer thread and posts a tagged `Task::Diagnostics`
    /// through the pool, and forwarding that result back yields the
    /// `Task::RefreshDiagnostics` nudge.
    #[test]
    fn threaded_writer_serves_writes_reads_and_settles() {
        let timeout = std::time::Duration::from_secs(10);
        let (task_tx, task_rx) = crossbeam_channel::unbounded::<Task>();
        let pool = TaskPool::new(task_tx.clone(), 1);

        let mut writer = WriterHandle::new(client_sender());
        writer.spawn(
            PoolSpawners {
                main: pool.spawner(),
                fmt: pool.spawner(),
            },
            task_tx,
        );

        // Write: forwarded and applied on the writer thread; it arms the
        // debounced settle (with externals for the opened uri) over there.
        let uri: lsp_types::Uri = "file:///nonexistent-spike/doc.qmd".parse().unwrap();
        let params = lsp_types::DidOpenTextDocumentParams {
            text_document: lsp_types::TextDocumentItem {
                uri: uri.clone(),
                language_id: "quarto".into(),
                version: 0,
                text: "# Heading\n\nBody text.\n".into(),
            },
        };
        writer.forward_write(WriteCommand::DidOpen(params));

        // Read: FIFO-ordered after the write; the writer mints the snapshot and
        // the job (run on the pool) observes the document written above.
        let read_uri = uri.clone();
        let (seen_tx, seen_rx) = crossbeam_channel::bounded::<Option<String>>(1);
        assert!(writer.submit_read(ReadJob {
            pool: ReadPool::Main,
            id: lsp_server::RequestId::from(1),
            run: Box::new(move |snap| {
                let _ = seen_tx.send(snap.document_content(&read_uri));
            }),
        }));
        let seen = seen_rx.recv_timeout(timeout).expect("read ran");
        assert_eq!(seen.as_deref(), Some("# Heading\n\nBody text.\n"));

        // Settle: the didOpen armed it; the writer self-fires after the
        // debounce window and spawns the read pass, whose result lands on the
        // task channel tagged with the writer's first generation. Relayed
        // client traffic (logs) shares the channel; skip it.
        let (generation, publishes, external_ran) = loop {
            match task_rx.recv_timeout(timeout).expect("settle result") {
                Task::Diagnostics {
                    generation,
                    publishes,
                    external_ran,
                } => break (generation, publishes, external_ran),
                Task::SendToClient(_) => continue,
                _ => panic!("expected Task::Diagnostics from the self-timed settle"),
            }
        };
        assert_eq!(generation, 1);
        assert!(
            external_ran.contains(&uri),
            "didOpen queued external linters for the opened doc"
        );

        // Forward the result back (as the main loop's `on_task` does): the
        // writer applies it to its store and posts the refresh nudge.
        assert!(
            !writer.forward_settle_result(generation, publishes, external_ran),
            "threaded mode must not apply settle results synchronously"
        );
        loop {
            match task_rx.recv_timeout(timeout).expect("refresh nudge") {
                Task::RefreshDiagnostics => break,
                Task::SendToClient(_) => continue,
                _ => panic!("expected Task::RefreshDiagnostics after the settle applied"),
            }
        }
    }

    /// End-to-end harvest cycle in threaded mode: a settle over a document with
    /// an on-disk bibliography routes the disk read through the harvester
    /// thread (`Task::Harvested` forwarded back, as the main loop does), and an
    /// out-of-band bibliography edit is resynced into salsa by the next settle.
    #[test]
    fn threaded_settle_harvests_referenced_files_off_thread() {
        use crate::lsp::uri_ext::UriExt;

        let timeout = std::time::Duration::from_secs(10);
        let dir = tempfile::tempdir().expect("tempdir");
        let doc_path = dir.path().join("main.qmd");
        let bib_path = dir.path().join("refs.bib");
        let doc_text = "---\nbibliography: refs.bib\n---\n\n# Heading\n\nCite [@key].\n";
        std::fs::write(&doc_path, doc_text).expect("write doc");
        std::fs::write(&bib_path, "@article{key, title={One}}\n").expect("write bib");

        let (task_tx, task_rx) = crossbeam_channel::unbounded::<Task>();
        let pool = TaskPool::new(task_tx.clone(), 1);
        let mut writer = WriterHandle::new(client_sender());
        writer.spawn(
            PoolSpawners {
                main: pool.spawner(),
                fmt: pool.spawner(),
            },
            task_tx,
        );

        let uri = lsp_types::Uri::from_file_path(&doc_path).expect("uri");
        writer.forward_write(WriteCommand::DidOpen(
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "quarto".into(),
                    version: 0,
                    text: doc_text.to_owned(),
                },
            },
        ));

        // Play the main loop: pump one settle to completion, forwarding
        // harvest batches back to the writer, and return the last batch seen.
        let pump_settle = |writer: &mut WriterHandle| -> Vec<(std::path::PathBuf, Option<String>)> {
            let mut last_batch = Vec::new();
            loop {
                match task_rx.recv_timeout(timeout).expect("settle activity") {
                    Task::Harvested { cycle, batch } => {
                        last_batch = batch.clone();
                        writer.forward_harvest(cycle, batch);
                    }
                    Task::Diagnostics { .. } => return last_batch,
                    _ => {}
                }
            }
        };

        // First settle: the harvest reads the bibliography off-thread.
        let batch = pump_settle(&mut writer);
        assert!(
            batch.iter().any(|(path, _)| path == &bib_path),
            "first settle harvested the bibliography: {batch:?}"
        );

        // Out-of-band edit; a save arms the next settle, whose harvest resyncs it.
        std::fs::write(&bib_path, "@article{key, title={Two}}\n").expect("rewrite bib");
        writer.forward_write(WriteCommand::DidSave(
            lsp_types::DidSaveTextDocumentParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                text: None,
            },
        ));
        let batch = pump_settle(&mut writer);
        assert!(
            batch.iter().any(|(path, content)| path == &bib_path
                && content.as_deref().is_some_and(|c| c.contains("Two"))),
            "second settle harvested the edited bibliography: {batch:?}"
        );

        // The resynced content is what reads now observe.
        let (seen_tx, seen_rx) = crossbeam_channel::bounded::<Option<String>>(1);
        let probe_path = bib_path.clone();
        assert!(writer.submit_read(ReadJob {
            pool: ReadPool::Main,
            id: lsp_server::RequestId::from(1),
            run: Box::new(move |snap| {
                let content = snap
                    .db()
                    .file_text(probe_path)
                    .map(|file| file.content_or_empty(snap.db()).to_string());
                let _ = seen_tx.send(content);
            }),
        }));
        let seen = seen_rx.recv_timeout(timeout).expect("read ran");
        assert!(
            seen.as_deref().is_some_and(|c| c.contains("Two")),
            "pooled read observes the resynced bibliography, got: {seen:?}"
        );
    }
}
