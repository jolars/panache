//! Tests for pulling client settings via `workspace/configuration`: the server
//! requests the `panache` section (after `initialized` and on every
//! `didChangeConfiguration`) and applies the reply to its runtime settings.

use super::helpers::*;
use lsp_server::{Message, RequestId};
use lsp_types::ConfigurationParams;
use lsp_types::request::{Request as _, WorkspaceConfiguration};
use serde_json::json;

/// Collect the `workspace/configuration` requests the server has emitted since
/// the last drain, as `(id, params)` pairs.
fn drain_config_pulls(server: &TestLspServer) -> Vec<(RequestId, ConfigurationParams)> {
    server
        .drain_client_messages()
        .into_iter()
        .filter_map(|msg| match msg {
            Message::Request(req) if req.method == WorkspaceConfiguration::METHOD => {
                let params = serde_json::from_value(req.params).expect("configuration params");
                Some((req.id, params))
            }
            _ => None,
        })
        .collect()
}

/// After `initialized`, a client that advertises `workspace.configuration` is
/// asked for its `panache` settings section.
#[test]
fn initialized_pulls_panache_configuration() {
    let mut server = TestLspServer::new();
    server.initialize_pull_configuration("file:///workspace");
    server.initialized();

    let pulls = drain_config_pulls(&server);
    assert_eq!(
        pulls.len(),
        1,
        "one workspace/configuration request at startup"
    );
    let (_id, params) = &pulls[0];
    assert_eq!(params.items.len(), 1);
    assert_eq!(params.items[0].section.as_deref(), Some("panache"));
    assert!(
        params.items[0].scope_uri.is_none(),
        "runtime settings are global, not scoped to a document"
    );
}

/// The pull reply applies the runtime setting: a section object carrying
/// `experimental.incrementalParsing = true` flips the flag live.
#[test]
fn configuration_reply_applies_runtime_setting() {
    // This asserts the client-settings plumbing, which the environment
    // override deliberately bypasses.
    if incremental_parsing_forced_by_env() {
        return;
    }
    let mut server = TestLspServer::new();
    server.initialize_pull_configuration("file:///workspace");
    server.initialized();
    assert!(
        !server.experimental_incremental_parsing_enabled(),
        "incremental parsing defaults off"
    );

    let (id, _params) = drain_config_pulls(&server)
        .into_iter()
        .next()
        .expect("a configuration pull");
    // A `section: "panache"` request returns the bare section object per item.
    server.send_client_response(
        id,
        json!([{ "experimental": { "incrementalParsing": true } }]),
    );

    assert!(
        server.experimental_incremental_parsing_enabled(),
        "workspace/configuration reply should enable incremental parsing"
    );
}

/// A `didChangeConfiguration` notification triggers a fresh pull for
/// pull-capable clients (the LSP-canonical live-reload path).
#[test]
fn did_change_configuration_re_pulls() {
    let mut server = TestLspServer::new();
    server.initialize_pull_configuration("file:///workspace");
    server.initialized();
    let _ = drain_config_pulls(&server); // discard the startup pull

    server.did_change_configuration(json!(null));

    let pulls = drain_config_pulls(&server);
    assert_eq!(
        pulls.len(),
        1,
        "didChangeConfiguration should re-pull configuration"
    );
}

/// Without the capability, the server never issues a `workspace/configuration`
/// request — neither at startup nor on `didChangeConfiguration`.
#[test]
fn no_capability_means_no_pull() {
    let mut server = TestLspServer::new();
    server.initialize("file:///workspace");
    server.initialized();
    server.did_change_configuration(json!(null));

    assert!(
        drain_config_pulls(&server).is_empty(),
        "a client without workspace.configuration must not be pulled"
    );
}

/// An empty or `null` configuration reply is a no-op: no panic, no change to
/// runtime settings.
#[test]
fn empty_configuration_reply_is_noop() {
    // This asserts the client-settings plumbing, which the environment
    // override deliberately bypasses.
    if incremental_parsing_forced_by_env() {
        return;
    }
    let mut server = TestLspServer::new();
    server.initialize_pull_configuration("file:///workspace");
    server.initialized();
    let (id, _) = drain_config_pulls(&server)
        .into_iter()
        .next()
        .expect("a pull");

    server.send_client_response(id, json!([]));
    assert!(!server.experimental_incremental_parsing_enabled());

    // A `null` section (client has no `panache` settings) is likewise ignored.
    server.did_change_configuration(json!(null));
    let (id2, _) = drain_config_pulls(&server)
        .into_iter()
        .next()
        .expect("a re-pull");
    server.send_client_response(id2, json!([null]));
    assert!(!server.experimental_incremental_parsing_enabled());
}
