// `lsp_types::Uri` (fluent_uri-backed) trips `clippy::mutable_key_type` when used
// as a `HashMap`/`HashSet` key, but we never mutate keys and the protocol type
// `WorkspaceEdit.changes` mandates `HashMap<Uri, _>`. Allow it module-wide.
#![allow(clippy::mutable_key_type)]

use std::path::PathBuf;
use std::time::Duration;

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

    let capabilities = serde_json::to_value(dispatch::server_capabilities()).map_err(to_io)?;
    // Performs the full initialize/initialized handshake and returns the params.
    let init_value = connection.initialize(capabilities).map_err(to_io)?;
    let init_params: InitializeParams = serde_json::from_value(init_value).map_err(to_io)?;

    let mut gs = GlobalState::new(ClientSender::new(connection.sender.clone()));
    gs.on_initialize(init_params);
    gs.on_initialized();

    main_loop(&mut gs, &connection)?;

    drop(connection);
    io_threads.join()?;
    Ok(())
}

/// How long `select!` parks when there's no pending lint deadline. crossbeam's
/// `select!` arms are fixed at compile time, so we can't drop the timeout arm
/// when idle; instead we wake on a coarse interval. The exact value is
/// irrelevant — any client message or worker result wakes us immediately, and
/// `dispatch_due_lints` re-arms a real deadline the moment one is scheduled.
const IDLE_TICK: Duration = Duration::from_secs(3600);

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
                        gs.on_request(req);
                    }
                    Message::Notification(not) => {
                        if not.method == lsp_types::notification::Exit::METHOD {
                            break;
                        }
                        gs.on_notification(not);
                    }
                    Message::Response(resp) => gs.on_client_response(resp),
                }
            }
            recv(task_rx) -> task => {
                if let Ok(task) = task {
                    gs.on_task(task);
                }
            }
            default(timeout) => {}
        }

        gs.dispatch_due_lints();
    }

    Ok(())
}
