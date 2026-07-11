// `lsp_types::Uri` (fluent_uri-backed) trips `clippy::mutable_key_type` when used
// as a `HashMap`/`HashSet` key, but we never mutate keys and the protocol type
// `WorkspaceEdit.changes` mandates `HashMap<Uri, _>`. Allow it module-wide.
#![allow(clippy::mutable_key_type)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossbeam_channel::select;
use lsp_server::{Connection, Message};
use lsp_types::InitializeParams;
use lsp_types::notification::Notification as _;
use rowan::GreenNode;

mod config;
mod context;
mod conversions;
mod dispatch;
mod documents;
pub(crate) mod global_state;
mod handlers;
mod helpers;
mod navigation;
mod symbols;
mod task_pool;
#[doc(hidden)]
pub mod testing;
mod uri_ext;
mod writer;
mod writer_command;

pub(crate) use global_state::{ClientSender, GlobalState};
#[doc(hidden)]
pub use testing::{LspTester, WorkspaceSymbolSummary};
#[doc(hidden)]
pub use uri_ext::UriExt;

/// State for a single document in the LSP.
#[derive(Clone)]
pub struct DocumentState {
    /// Canonical file path for this document (if it exists on disk).
    pub path: Option<PathBuf>,
    /// Salsa input for this document's text.
    pub salsa_file: crate::salsa::FileText,
    /// Salsa input for this document's config.
    pub salsa_config: crate::salsa::FileConfig,
    /// Cached syntax tree for incremental parsing.
    pub tree: GreenNode,
    /// The client's latest `textDocument` version (from `didOpen`/`didChange`).
    /// Tags diagnostics publishes so the client can discard a report computed
    /// against a buffer it has since edited.
    pub version: i32,
}

#[derive(Debug, Clone, Default)]
pub struct LspRuntimeSettings {
    pub experimental_incremental_parsing: bool,
}

fn to_io<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// Run the language server over stdio until the client disconnects.
pub fn run() -> std::io::Result<()> {
    let (connection, io_threads) = Connection::stdio();

    // Drive the handshake by hand (rather than `Connection::initialize`) so the
    // `InitializeResult` can carry `serverInfo` alongside capabilities; the
    // convenience helper hardcodes `serverInfo: null`.
    let (id, init_value) = connection.initialize_start().map_err(to_io)?;
    let init_result = serde_json::json!({
        "capabilities": dispatch::server_capabilities(),
        "serverInfo": dispatch::server_info(),
    });
    connection
        .initialize_finish(id, init_result)
        .map_err(to_io)?;
    let init_params: InitializeParams = serde_json::from_value(init_value).map_err(to_io)?;

    let mut gs = GlobalState::new(ClientSender::new(connection.sender.clone()));
    gs.on_initialize(init_params);
    gs.on_initialized();
    // Handshake done (it configures the writer state directly); move the salsa
    // writer onto its dedicated thread so the main loop never blocks on salsa
    // writes or referenced-file disk I/O.
    gs.spawn_writer();

    main_loop(&mut gs, &connection)?;

    // `gs` holds a clone of the connection's message sender; the writer IO thread
    // only stops once *every* sender is dropped. Drop `gs` before joining so the
    // writer's channel disconnects and the process can actually exit (otherwise
    // `join` blocks forever and the server lingers after the client is gone).
    drop(gs);
    drop(connection);
    io_threads.join()?;
    Ok(())
}

/// How long `select!` parks when there's no pending lint deadline. crossbeam's
/// `select!` arms are fixed at compile time, so we can't drop the timeout arm
/// when idle; instead we wake on a coarse interval. The exact value is
/// irrelevant — any client message or worker result wakes us immediately. In
/// production (threaded writer) this is always the timeout: the writer thread
/// self-times the debounced settle, so the main loop has no lint deadline to
/// watch; the deadline arm only matters in inline mode (pre-spawn, tests).
const IDLE_TICK: Duration = Duration::from_secs(3600);

/// A single main-loop step blocks the event loop for its whole duration: every
/// pending client request (format-on-save included) waits until it returns. Warn
/// when one exceeds this so a perceptible stall names its own culprit in the log
/// instead of hiding as a gap between unrelated lines. Normal steps are single-
/// digit milliseconds; the settle read pass runs off-thread and never lands here.
const SLOW_STEP: Duration = Duration::from_millis(50);

/// Log a `warn` when a main-loop step blocked longer than [`SLOW_STEP`].
fn log_if_slow(label: &str, start: Instant) {
    let elapsed = start.elapsed();
    if elapsed >= SLOW_STEP {
        log::warn!("main-loop step blocked {elapsed:?}: {label}");
    }
}

fn main_loop(gs: &mut GlobalState, conn: &Connection) -> std::io::Result<()> {
    // Clone the worker-result receiver so the `select!` doesn't borrow `gs`.
    let task_rx = gs.task_receiver.clone();

    loop {
        // Block until the next message, a finished task, or the nearest lint
        // deadline (so debounced lints fire even when the client is idle).
        let timeout = gs.next_lint_timeout().unwrap_or(IDLE_TICK);

        select! {
            recv(conn.receiver) -> msg => {
                let Ok(msg) = msg else { break };
                match msg {
                    Message::Request(req) => {
                        if conn.handle_shutdown(&req).map_err(to_io)? {
                            break;
                        }
                        let label = format!("request {} (id {})", req.method, req.id);
                        let t = Instant::now();
                        gs.on_request(req);
                        log_if_slow(&label, t);
                    }
                    Message::Notification(not) => {
                        if not.method == lsp_types::notification::Exit::METHOD {
                            break;
                        }
                        let label = format!("notification {}", not.method);
                        let t = Instant::now();
                        gs.on_notification(not);
                        log_if_slow(&label, t);
                    }
                    Message::Response(resp) => {
                        let t = Instant::now();
                        gs.on_client_response(resp);
                        log_if_slow("client response", t);
                    }
                }
            }
            recv(task_rx) -> task => {
                if let Ok(task) = task {
                    let t = Instant::now();
                    gs.on_task(task);
                    log_if_slow("worker task result", t);
                }
            }
            default(timeout) => {}
        }

        let t = Instant::now();
        gs.dispatch_due_lints();
        log_if_slow("dispatch_due_lints", t);
    }

    Ok(())
}
