//! The synchronous server state, modeled on rust-analyzer's `GlobalState`.
//!
//! The [`main loop`](crate::lsp::run) owns a single [`GlobalState`]: the LSP
//! transport (client sender, in-flight/cancelled request ids) and the task
//! pools. The salsa database, document map, config state, diagnostics store,
//! and settle machinery live on the writer
//! ([`WriterState`](crate::lsp::writer::WriterState)) — inline during the
//! handshake and in tests, on the dedicated writer thread in production. Heavy
//! reads (hover, completion, formatting, lint) are dispatched to the
//! [`TaskPool`] over a cheap [`StateSnapshot`] (a clone of the salsa handle plus
//! an `Arc` of the document map); their results return to the main loop as
//! [`Task`] values to be turned into responses or forwarded to the writer.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use lsp_server::{Message, Notification, Request, RequestId, Response};
use lsp_types::{Diagnostic, MessageType, Uri};

use super::DocumentState;
use super::config::load_config;
use super::task_pool::{TaskPool, default_pool_size};
use crate::Config;
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

/// Single owner of the server's current diagnostic set, unifying push delivery,
/// the pull store, and clear-on-fix bookkeeping (rust-analyzer's
/// `DiagnosticCollection`). Owned by the writer
/// ([`WriterState`](crate::lsp::writer::WriterState)), which applies every
/// settle result. Each applied settle pass hands [`Self::apply`] the
/// complete merged map — every URI across all open documents and project
/// manifests — and it diffs that against the retained set to publish/store only
/// what changed and clear what disappeared. The retained keys *are* the
/// clear-tracker, so a shared manifest stays reported until no settle reports it.
#[derive(Default)]
pub(crate) struct DiagnosticCollection {
    // `Arc` so a pooled pull handler can read the store off-thread (for
    // `related_documents` and `workspace/diagnostic`) via the cheap clone every
    // `StateSnapshot` carries; mutation is copy-on-write through `Arc::make_mut`,
    // which is uncontended because the writer is the sole mutator.
    current: Arc<HashMap<Uri, StoredDiagnostics>>,
    result_seq: u64,
}

impl DiagnosticCollection {
    /// Apply a complete settle result. Upserts each URI whose diagnostics changed
    /// (bumping its `result_id`), leaves unchanged URIs untouched (so a re-pull
    /// still answers `unchanged` and push clients see no redundant notification),
    /// and clears every URI the previous set held but this one omits.
    pub(crate) fn apply(
        &mut self,
        publishes: Vec<(Uri, Option<i32>, Vec<Diagnostic>)>,
        sender: &ClientSender,
        pull: bool,
    ) {
        let mut next: HashSet<Uri> = HashSet::with_capacity(publishes.len());
        for (uri, version, items) in publishes {
            next.insert(uri.clone());
            let unchanged = self
                .current
                .get(&uri)
                .is_some_and(|entry| entry.items == items);
            if unchanged {
                continue;
            }
            self.result_seq += 1;
            let result_id = self.result_seq.to_string();
            if !pull {
                sender.publish_diagnostics(uri.clone(), items.clone(), version);
            }
            Arc::make_mut(&mut self.current).insert(
                uri,
                StoredDiagnostics {
                    version,
                    items,
                    result_id,
                },
            );
        }
        let stale: Vec<Uri> = self
            .current
            .keys()
            .filter(|uri| !next.contains(*uri))
            .cloned()
            .collect();
        for uri in stale {
            self.drop_uri(&uri, sender, pull);
        }
    }

    /// Remove a single URI immediately (a closed document), clearing it on push
    /// clients so a pull issued before the next settle no longer reports it.
    pub(crate) fn drop_uri(&mut self, uri: &Uri, sender: &ClientSender, pull: bool) {
        if Arc::make_mut(&mut self.current).remove(uri).is_some() && !pull {
            sender.publish_diagnostics(uri.clone(), Vec::new(), None);
        }
    }

    /// The stored diagnostics for `uri` (test-only: production pulls read the
    /// store via the snapshot's `HashMap` on the pool).
    #[cfg(test)]
    pub(crate) fn get(&self, uri: &Uri) -> Option<&StoredDiagnostics> {
        self.current.get(uri)
    }

    /// A cheap, shareable handle to the current store, carried on every
    /// [`StateSnapshot`] so the pooled pull handlers (`workspace/diagnostic`,
    /// `related_documents`) read it off-thread.
    pub(crate) fn shared(&self) -> Arc<HashMap<Uri, StoredDiagnostics>> {
        Arc::clone(&self.current)
    }
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

    /// Send a `window/showMessage` notification (a client-surfaced toast, unlike
    /// the log-only [`Self::log_message`]).
    pub(crate) fn show_message(&self, typ: MessageType, message: impl Into<String>) {
        self.notify::<lsp_types::notification::ShowMessage>(lsp_types::ShowMessageParams {
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
    pub(crate) workspace_folders: Vec<PathBuf>,
    /// Read-only view of the diagnostic store at snapshot time, so a pooled pull
    /// handler can attach `related_documents` without touching `GlobalState`.
    pub(crate) diagnostics: Arc<HashMap<Uri, StoredDiagnostics>>,
    /// Client capabilities the pull handler needs, copied so it runs off-thread.
    pub(crate) supports_pull_diagnostics: bool,
    pub(crate) supports_related_documents: bool,
}

impl StateSnapshot {
    /// Assemble a snapshot from the writer-owned parts. All fields come from
    /// [`WriterState`](crate::lsp::writer::WriterState) now that it owns the
    /// diagnostics store and the pull-capability flags.
    pub(crate) fn assemble(
        analysis: crate::salsa::Analysis,
        document_map: Arc<DocumentMap>,
        workspace_folders: Vec<PathBuf>,
        diagnostics: Arc<HashMap<Uri, StoredDiagnostics>>,
        supports_pull_diagnostics: bool,
        supports_related_documents: bool,
    ) -> Self {
        Self {
            analysis,
            document_map,
            workspace_folders,
            diagnostics,
            supports_pull_diagnostics,
            supports_related_documents,
        }
    }

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

    /// The workspace folder that best contains `uri` (longest-prefix match),
    /// falling back to the first folder. Drives multi-root config resolution.
    pub(crate) fn workspace_root_for(&self, uri: &Uri) -> Option<PathBuf> {
        crate::lsp::config::select_workspace_root(&self.workspace_folders, Some(uri))
    }

    /// Load config with URI-based flavor detection.
    pub(crate) fn config(&self, uri: &Uri) -> Config {
        load_config(&self.workspace_folders, Some(uri))
    }

    /// Document text + config in one call.
    pub(crate) fn document_and_config(&self, uri: &Uri) -> Option<(String, Config)> {
        let content = self.document_content(uri)?;
        Some((content, self.config(uri)))
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
    /// A pooled request answer that also streams ordered `$/progress` chunks: the
    /// main loop sends `response` first, then each notification (see the pull
    /// diagnostics handler). Kept distinct from [`Task::Response`] so the response/
    /// progress ordering the protocol requires stays on the main loop.
    StreamingResponse {
        response: Response,
        progress: Vec<lsp_server::Notification>,
    },
    /// Diagnostics from a quiescent settle pass that re-lints *every* open
    /// document over one snapshot. `publishes` is the complete, merged set (one
    /// entry per URI, across all documents and project manifests). The main loop
    /// forwards the result to the writer — the owner of the diagnostics store
    /// and the lint generation — which diffs it against the previous settle,
    /// drops a pass superseded by a newer settle, and retires exactly the
    /// `external_ran` URIs from its pending set (a save queued after dispatch
    /// survives). A pass cancelled by a concurrent write returns nothing and is
    /// simply dropped — the cancelling write already armed the next settle,
    /// which re-lints everything.
    Diagnostics {
        generation: u64,
        publishes: Vec<(Uri, Option<i32>, Vec<Diagnostic>)>,
        external_ran: HashSet<Uri>,
    },
    /// The writer applied a settle result to its store (threaded mode); the main
    /// loop should nudge pull clients to re-pull via
    /// `workspace/diagnostic/refresh` (a server→client request, so it needs the
    /// main loop's outgoing-id tracking).
    RefreshDiagnostics,
}

/// The synchronous, single-threaded-mutation server state.
pub(crate) struct GlobalState {
    pub(crate) sender: ClientSender,

    /// Whether the client supports `workspace/diagnostic/refresh`. Lets the
    /// server nudge the client to re-pull when an async pass (save-time external
    /// linters, cross-file dependents) updates the store. Main-loop-owned (the
    /// refresh is a server→client request with a tracked outgoing id), unlike
    /// the pull-capability flags, which live on the writer with the store.
    pub(crate) supports_diagnostic_refresh: bool,

    /// The salsa writer. Owns the master database handle, the document map,
    /// config state, the diagnostics store, and the settle machinery — inline
    /// until [`Self::spawn_writer`], on the dedicated writer thread after.
    pub(crate) writer: crate::lsp::writer::WriterHandle,

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
}

impl GlobalState {
    pub(crate) fn new(sender: ClientSender) -> Self {
        let (task_tx, task_receiver) = crossbeam_channel::unbounded::<Task>();
        let pool = TaskPool::new(task_tx.clone(), default_pool_size());
        let fmt_pool = TaskPool::new(task_tx, 1);
        Self {
            supports_diagnostic_refresh: false,
            writer: crate::lsp::writer::WriterHandle::new(sender.clone()),
            sender,
            pool,
            fmt_pool,
            task_receiver,
            in_flight: HashSet::new(),
            cancelled: HashSet::new(),
            outgoing: HashMap::new(),
            next_outgoing_id: 1,
        }
    }

    /// A cheap read snapshot for a worker thread. Inline mode only: in threaded
    /// mode snapshots are minted by the writer thread per
    /// [`ReadJob`](crate::lsp::writer::ReadJob).
    pub(crate) fn snapshot(&self) -> StateSnapshot {
        self.writer.state().mint_snapshot()
    }

    /// Move the writer state onto its dedicated thread. Called once after
    /// `initialize`/`initialized` (which configure the writer directly); from
    /// then on writes, reads, and settles are forwarded over channels.
    pub(crate) fn spawn_writer(&mut self) {
        let pools = crate::lsp::writer::PoolSpawners {
            main: self.pool.spawner(),
            fmt: self.fmt_pool.spawner(),
        };
        self.writer.spawn(pools, self.pool.result_sender());
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

    /// Ask the client to re-pull diagnostics, if it supports refresh. A no-op in
    /// push mode (the flag is only set when the client advertised refresh).
    pub(crate) fn send_diagnostic_refresh(&mut self) {
        if self.supports_diagnostic_refresh {
            self.send_request::<lsp_types::request::WorkspaceDiagnosticRefresh>(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Position, Range};

    fn sender() -> ClientSender {
        let (tx, _rx) = crossbeam_channel::unbounded();
        ClientSender::new(tx)
    }

    fn uri(s: &str) -> Uri {
        s.parse().expect("valid uri")
    }

    fn diag(line: u32) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position { line, character: 0 },
                end: Position { line, character: 1 },
            },
            message: "x".to_owned(),
            ..Default::default()
        }
    }

    /// A settle that re-reports identical diagnostics for a URI must not bump its
    /// `result_id` (so a re-pull still answers `unchanged`) — the no-op-settle
    /// case the all-docs model creates for every unchanged document.
    #[test]
    fn unchanged_uri_keeps_result_id_across_settles() {
        let sender = sender();
        let mut dc = DiagnosticCollection::default();
        let a = uri("file:///a.qmd");

        dc.apply(vec![(a.clone(), None, vec![diag(2)])], &sender, true);
        let first = dc.get(&a).expect("stored").result_id.clone();

        // Identical items on the next settle.
        dc.apply(vec![(a.clone(), None, vec![diag(2)])], &sender, true);
        assert_eq!(
            dc.get(&a).expect("still stored").result_id,
            first,
            "unchanged diagnostics must keep the same result_id"
        );

        // Changed items bump it.
        dc.apply(vec![(a.clone(), None, vec![diag(5)])], &sender, true);
        assert_ne!(
            dc.get(&a).expect("still stored").result_id,
            first,
            "changed diagnostics must get a fresh result_id"
        );
    }

    /// A URI present in one settle but omitted by the next is cleared from the
    /// collection (clear-on-fix for fixed manifests, closed docs, resolved
    /// cross-file diagnostics).
    #[test]
    fn omitted_uri_is_cleared() {
        let sender = sender();
        let mut dc = DiagnosticCollection::default();
        let (a, x) = (uri("file:///a.qmd"), uri("file:///x.qmd"));

        dc.apply(
            vec![
                (a.clone(), None, vec![diag(1)]),
                (x.clone(), None, vec![diag(1)]),
            ],
            &sender,
            true,
        );
        assert!(dc.get(&a).is_some() && dc.get(&x).is_some());

        dc.apply(vec![(a.clone(), None, vec![diag(1)])], &sender, true);
        assert!(dc.get(&a).is_some(), "still-reported uri retained");
        assert!(dc.get(&x).is_none(), "omitted uri cleared");
    }
}
