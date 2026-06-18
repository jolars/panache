//! The synchronous server state, modeled on rust-analyzer's `GlobalState`.
//!
//! The [`main loop`](crate::lsp::run) owns a single [`GlobalState`] and is the
//! only thread that mutates it — notably, it is the sole writer of the salsa
//! database, so writes are serialized by construction and need no lock. Heavy
//! reads (hover, completion, formatting, lint) are dispatched to the
//! [`TaskPool`] over a cheap [`StateSnapshot`] (a clone of the salsa handle plus
//! an `Arc` of the document map); their results return to the main loop as
//! [`Task`] values to be turned into responses or diagnostics publishes.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender};
use lsp_server::{Message, Notification, Request, RequestId, Response};
use lsp_types::{Diagnostic, MessageType, Uri};

use super::DocumentState;
use super::LspRuntimeSettings;
use super::config::{load_config, load_config_with_source};
use super::task_pool::{TaskPool, default_pool_size};
use crate::Config;
use crate::config::ConfigSource;
use crate::syntax::{ParsedYamlRegionSnapshot, SyntaxNode};

/// The owning map of open documents, keyed by URI string (the URI itself is used
/// only for protocol I/O).
pub(crate) type DocumentMap = HashMap<String, DocumentState>;

/// Idle time after the last edit before a debounced lint pass fires. Rapid
/// keystrokes keep resetting this window so only the final edit in a burst
/// triggers diagnostics, instead of every keystroke queuing a full lint.
pub(crate) const DIAGNOSTICS_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(200);

/// The latest diagnostics computed for one URI, retained for the pull model
/// (`textDocument/diagnostic` / `workspace/diagnostic`). The lint pipeline fills
/// this store instead of pushing when the client supports pull diagnostics; the
/// `result_id` lets the server answer a re-pull with an `unchanged` report.
#[derive(Clone)]
pub(crate) struct StoredDiagnostics {
    pub(crate) version: Option<i32>,
    pub(crate) items: Vec<Diagnostic>,
    pub(crate) result_id: String,
}

/// A clonable handle for sending messages back to the client.
///
/// Replaces tower-lsp's `Client`: it is just a wrapped `crossbeam` sender, so
/// worker threads and handlers can hold a clone and publish diagnostics / log
/// without any async machinery.
#[derive(Clone)]
pub(crate) struct ClientSender {
    sender: Sender<Message>,
}

impl ClientSender {
    pub(crate) fn new(sender: Sender<Message>) -> Self {
        Self { sender }
    }

    /// Publish diagnostics for a document (fire-and-forget notification).
    pub(crate) fn publish_diagnostics(
        &self,
        uri: Uri,
        diagnostics: Vec<Diagnostic>,
        version: Option<i32>,
    ) {
        self.notify::<lsp_types::notification::PublishDiagnostics>(
            lsp_types::PublishDiagnosticsParams {
                uri,
                diagnostics,
                version,
            },
        );
    }

    /// Send a `window/logMessage` notification.
    pub(crate) fn log_message(&self, typ: MessageType, message: impl Into<String>) {
        self.notify::<lsp_types::notification::LogMessage>(lsp_types::LogMessageParams {
            typ,
            message: message.into(),
        });
    }

    /// Send a typed notification to the client.
    pub(crate) fn notify<N: lsp_types::notification::Notification>(&self, params: N::Params) {
        self.send(Message::Notification(Notification::new(
            N::METHOD.to_owned(),
            params,
        )));
    }

    /// Send a raw message (response or server→client request) to the client.
    pub(crate) fn send(&self, message: Message) {
        if let Err(err) = self.sender.send(message) {
            log::warn!("LSP client channel closed; dropping message: {err}");
        }
    }
}

/// A cheap, `Send` read-only view of the server state dispatched to worker
/// threads.
///
/// `analysis` is a read-only [`Analysis`] view over a clone of the salsa handle
/// (an `Arc` bump; `Send` but `!Sync`, so each worker owns its own). It exposes
/// only shared (`&dyn Db`) access via [`StateSnapshot::db`], so a worker cannot
/// reach the `&mut` setters that only the main loop's `SalsaDb` handle has.
/// `document_map` is an `Arc` of the owning map. Crucially this never carries a
/// `rowan::SyntaxNode` (a `!Send` cursor) — only `GreenNode`s inside
/// `DocumentState`, from which workers build cursors locally.
///
/// [`Analysis`]: crate::salsa::Analysis
pub(crate) struct StateSnapshot {
    analysis: crate::salsa::Analysis,
    pub(crate) document_map: Arc<DocumentMap>,
    pub(crate) workspace_root: Option<PathBuf>,
}

impl StateSnapshot {
    /// Shared, read-only database handle for worker read queries.
    pub(crate) fn db(&self) -> &dyn crate::salsa::Db {
        self.analysis.db()
    }

    /// Clone the [`DocumentState`] for `uri`, if open.
    pub(crate) fn document_state(&self, uri: &Uri) -> Option<DocumentState> {
        self.document_map.get(&uri.to_string()).cloned()
    }

    /// The current text of `uri`, read from salsa.
    pub(crate) fn document_content(&self, uri: &Uri) -> Option<String> {
        let state = self.document_map.get(&uri.to_string())?;
        Some(state.salsa_file.content_or_empty(self.db()).to_string())
    }

    /// The current text and a freshly-rooted syntax tree for `uri`.
    pub(crate) fn document_content_and_tree(&self, uri: &Uri) -> Option<(String, SyntaxNode)> {
        let state = self.document_map.get(&uri.to_string())?;
        Some((
            state.salsa_file.content_or_empty(self.db()).to_string(),
            SyntaxNode::new_root(state.tree.clone()),
        ))
    }

    /// The salsa-cached syntax tree for `uri`, freshly rooted.
    ///
    /// This is the same parse hover/symbols read, so callers (e.g. formatting)
    /// can reuse it instead of parsing the document again. Returns `None` if the
    /// document isn't open.
    pub(crate) fn parsed_tree(&self, uri: &Uri) -> Option<SyntaxNode> {
        let state = self.document_map.get(&uri.to_string())?;
        Some(crate::salsa::parsed_tree_root(
            self.db(),
            state.salsa_file,
            state.salsa_config,
        ))
    }

    /// Load config with URI-based flavor detection.
    pub(crate) fn config(&self, uri: &Uri) -> Config {
        load_config(&self.workspace_root, Some(uri))
    }

    /// Document text + config in one call.
    pub(crate) fn document_and_config(&self, uri: &Uri) -> Option<(String, Config)> {
        let content = self.document_content(uri)?;
        Some((content, self.config(uri)))
    }

    /// Document text + config + [`ConfigSource`] + workspace root, for callers
    /// that resolve `exclude`/`extend_exclude` patterns.
    pub(crate) fn document_config_and_source(
        &self,
        uri: &Uri,
    ) -> Option<(String, Config, ConfigSource, Option<PathBuf>)> {
        let content = self.document_content(uri)?;
        let (config, source) = load_config_with_source(&self.workspace_root, Some(uri));
        Some((content, config, source, self.workspace_root.clone()))
    }

    /// Build a definition index for `uri` merged with all `include`d documents.
    pub(crate) fn definition_index_with_includes(
        &self,
        uri: &Uri,
    ) -> crate::salsa::DefinitionIndex {
        let Some(state) = self.document_map.get(&uri.to_string()) else {
            return crate::salsa::DefinitionIndex::default();
        };
        let (salsa_file, salsa_config) = (state.salsa_file, state.salsa_config);
        let db = self.db();
        let graph = crate::salsa::project_structure(db, salsa_file, salsa_config).clone();
        let mut index = crate::salsa::definition_index(db, salsa_file, salsa_config).clone();
        for path in graph.documents().iter() {
            if let Some(include_file) = db.file_text(path.clone()) {
                let include_index = crate::salsa::definition_index(db, include_file, salsa_config);
                index.merge_from(include_index);
            }
        }
        index
    }

    /// Borrowed YAML region snapshots for `uri`, derived (and memoized) by salsa
    /// from the document's parse tree. Empty slice if the document isn't open.
    ///
    /// This is the single source of truth: the regions are a pure projection of
    /// the CST, so we read salsa's `returns(ref)` memo rather than caching a copy
    /// on `DocumentState`.
    pub(crate) fn parsed_yaml_regions(&self, uri: &Uri) -> &[ParsedYamlRegionSnapshot] {
        let Some((file, config)) = self
            .document_map
            .get(&uri.to_string())
            .map(|state| (state.salsa_file, state.salsa_config))
        else {
            return &[];
        };
        crate::salsa::parsed_yaml_regions_for_file(self.db(), file, config)
    }
}

/// A unit of work completed on a worker thread, posted back to the main loop.
pub(crate) enum Task {
    /// A request answer ready to forward to the client.
    Response(Response),
    /// Diagnostics from a quiescent settle pass that re-lints *every* open
    /// document over one snapshot. `publishes` is the complete, merged set (one
    /// entry per URI, across all documents and project manifests); the main loop
    /// diffs it against the previous settle to clear URIs no longer reported.
    /// `generation` lets the main loop drop a pass superseded by a newer settle.
    /// `external_ran` is the set of URIs this pass ran external linters for, so
    /// the main loop retires exactly those from `external_pending` (a save queued
    /// after dispatch survives). A pass cancelled by a concurrent write returns
    /// nothing and is simply dropped — the cancelling write already armed the
    /// next settle, which re-lints everything.
    Diagnostics {
        generation: u64,
        publishes: Vec<(Uri, Option<i32>, Vec<Diagnostic>)>,
        external_ran: HashSet<Uri>,
    },
}

/// The synchronous, single-threaded-mutation server state.
pub(crate) struct GlobalState {
    pub(crate) sender: ClientSender,

    /// Open documents. `Arc` so snapshots clone it in O(1); writers use
    /// [`Arc::make_mut`] for copy-on-write single-writer semantics.
    pub(crate) document_map: Arc<DocumentMap>,
    pub(crate) workspace_root: Option<PathBuf>,
    pub(crate) runtime_settings: LspRuntimeSettings,

    /// Whether the client advertised support for the pull diagnostics model at
    /// `initialize`. When `true` the server stops pushing
    /// `textDocument/publishDiagnostics` and serves diagnostics from
    /// `diagnostics_store` on demand instead (mode-switch — no double reporting).
    pub(crate) supports_pull_diagnostics: bool,
    /// Whether the client supports `workspace/diagnostic/refresh`. Lets the
    /// server nudge the client to re-pull when an async pass (save-time external
    /// linters, cross-file dependents) updates the store.
    pub(crate) supports_diagnostic_refresh: bool,
    /// Latest diagnostics per URI, the source of truth for pull responses.
    pub(crate) diagnostics_store: HashMap<Uri, StoredDiagnostics>,
    /// Monotonic counter stamped into each stored `result_id`.
    pub(crate) diagnostics_result_seq: u64,

    /// The master salsa handle, mutated only on the main thread.
    pub(crate) salsa: crate::salsa::SalsaDb,

    pub(crate) pool: TaskPool<Task>,
    /// Dedicated single-thread pool for formatting requests. Matches
    /// rust-analyzer's split: external formatters can block for hundreds of
    /// milliseconds, and isolating them from the main pool keeps hover /
    /// completion latency stable. Both pools post on the same `task_receiver`.
    pub(crate) fmt_pool: TaskPool<Task>,
    pub(crate) task_receiver: Receiver<Task>,

    /// In-flight incoming request ids (for `$/cancelRequest`).
    pub(crate) in_flight: HashSet<RequestId>,
    pub(crate) cancelled: HashSet<RequestId>,
    /// Outgoing server→client request ids → method, for logging replies.
    pub(crate) outgoing: HashMap<RequestId, &'static str>,
    pub(crate) next_outgoing_id: i32,

    /// Single debounce timer for the whole workspace. `Some(t)` means a quiescent
    /// re-lint of *all* open documents is due at `t`; each salsa-input write
    /// pushes it out by [`DIAGNOSTICS_DEBOUNCE`]. The all-docs model needs no
    /// per-document deadlines and no cancel→re-arm net: any write that cancels an
    /// in-flight pass has, by construction, already armed the next settle.
    pub(crate) settle_deadline: Option<Instant>,
    /// One global lint generation, bumped per dispatched settle pass. The pass
    /// result is tagged with it and dropped in `on_task` if a newer settle has
    /// since been dispatched.
    pub(crate) lint_generation: u64,
    /// URIs whose next settle pass must also run external linters (the expensive
    /// on-save/-open signal). Built-ins run for every open doc each settle;
    /// externals run only for these. Retired in `on_task` once the pass that ran
    /// them completes, so a save queued after dispatch survives a cancellation.
    pub(crate) external_pending: HashSet<Uri>,
    /// Every URI the last applied settle published to (documents + project
    /// manifests). The authoritative clear-tracker: a URI present here but absent
    /// from the next settle's complete set is cleared (published empty / dropped
    /// from the pull store). Subsumes the old per-document manifest
    /// reference-counting — the all-docs pass produces the complete set directly.
    pub(crate) last_published_uris: HashSet<Uri>,
}

impl GlobalState {
    pub(crate) fn new(sender: ClientSender) -> Self {
        let (task_tx, task_receiver) = crossbeam_channel::unbounded::<Task>();
        let pool = TaskPool::new(task_tx.clone(), default_pool_size());
        let fmt_pool = TaskPool::new(task_tx, 1);
        Self {
            sender,
            document_map: Arc::new(DocumentMap::new()),
            workspace_root: None,
            runtime_settings: LspRuntimeSettings::default(),
            supports_pull_diagnostics: false,
            supports_diagnostic_refresh: false,
            diagnostics_store: HashMap::new(),
            diagnostics_result_seq: 0,
            salsa: crate::salsa::SalsaDb::default(),
            pool,
            fmt_pool,
            task_receiver,
            in_flight: HashSet::new(),
            cancelled: HashSet::new(),
            outgoing: HashMap::new(),
            next_outgoing_id: 1,
            settle_deadline: None,
            lint_generation: 0,
            external_pending: HashSet::new(),
            last_published_uris: HashSet::new(),
        }
    }

    /// A cheap read snapshot for a worker thread.
    pub(crate) fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            analysis: crate::salsa::Analysis::new(self.salsa.clone()),
            document_map: Arc::clone(&self.document_map),
            workspace_root: self.workspace_root.clone(),
        }
    }

    /// Mutable access to the document map (copy-on-write if a snapshot still
    /// holds the previous `Arc`).
    pub(crate) fn document_map_mut(&mut self) -> &mut DocumentMap {
        Arc::make_mut(&mut self.document_map)
    }

    /// Send a successful or error response for `id` to the client and clear it
    /// from in-flight tracking.
    pub(crate) fn respond(&mut self, response: Response) {
        self.in_flight.remove(&response.id);
        self.cancelled.remove(&response.id);
        self.sender.send(Message::Response(response));
    }

    /// Issue a server→client request, tracking its id so we can log the reply.
    pub(crate) fn send_request<R: lsp_types::request::Request>(&mut self, params: R::Params) {
        let id = RequestId::from(self.next_outgoing_id);
        self.next_outgoing_id += 1;
        self.outgoing.insert(id.clone(), R::METHOD);
        self.sender.send(Message::Request(Request::new(
            id,
            R::METHOD.to_owned(),
            params,
        )));
    }

    /// Handle a reply to one of our server→client requests.
    pub(crate) fn on_client_response(&mut self, response: Response) {
        if let Some(method) = self.outgoing.remove(&response.id)
            && let Some(err) = response.error
        {
            log::warn!("server request {method} failed: {}", err.message);
        }
    }

    /// Arm (or push out) the single workspace settle timer. All lint dispatch
    /// funnels through here so the expensive all-docs re-lint runs once, at a
    /// quiescent point after the edit burst's writes have settled (rust-analyzer
    /// recomputes diagnostics the same way, after `process_changes`). Every
    /// salsa-input write must call this — directly or via [`Self::arm_settle_external`]
    /// — so a write that cancels an in-flight pass also schedules its recomputation.
    pub(crate) fn arm_settle(&mut self) {
        self.settle_deadline = Some(Instant::now() + DIAGNOSTICS_DEBOUNCE);
    }

    /// Arm the settle timer and mark `uri` as needing external linters on the
    /// next pass (the on-open/-save/referenced-file-change signal).
    pub(crate) fn arm_settle_external(&mut self, uri: Uri) {
        self.external_pending.insert(uri);
        self.arm_settle();
    }

    /// Deliver a diagnostics payload for `uri`, routed by the active model:
    /// publish a `textDocument/publishDiagnostics` notification (push), or update
    /// the pull store (pull). The push path is byte-for-byte the previous
    /// behavior; only pull-capable clients take the store branch.
    ///
    /// Callers batch a refresh via [`Self::send_diagnostic_refresh`] after a run
    /// of deliveries — this method never sends one itself.
    pub(crate) fn deliver_diagnostics(
        &mut self,
        uri: Uri,
        version: Option<i32>,
        diagnostics: Vec<Diagnostic>,
    ) {
        if self.supports_pull_diagnostics {
            self.store_diagnostics(uri, version, diagnostics);
        } else {
            self.sender.publish_diagnostics(uri, diagnostics, version);
        }
    }

    /// Clear a closed document's own diagnostics: publish an empty payload (push)
    /// or drop its store entry (pull) so it stops appearing in workspace pulls.
    pub(crate) fn drop_document_diagnostics(&mut self, uri: &Uri) {
        if self.supports_pull_diagnostics {
            self.diagnostics_store.remove(uri);
        } else {
            self.sender
                .publish_diagnostics(uri.clone(), Vec::new(), None);
        }
    }

    /// Upsert the stored diagnostics for `uri` with a fresh `result_id`.
    pub(crate) fn store_diagnostics(
        &mut self,
        uri: Uri,
        version: Option<i32>,
        items: Vec<Diagnostic>,
    ) {
        self.diagnostics_result_seq += 1;
        let result_id = self.diagnostics_result_seq.to_string();
        self.diagnostics_store.insert(
            uri,
            StoredDiagnostics {
                version,
                items,
                result_id,
            },
        );
    }

    /// Ask the client to re-pull diagnostics, if it supports refresh. A no-op in
    /// push mode (the flag is only set when the client advertised refresh).
    pub(crate) fn send_diagnostic_refresh(&mut self) {
        if self.supports_diagnostic_refresh {
            self.send_request::<lsp_types::request::WorkspaceDiagnosticRefresh>(());
        }
    }
}
