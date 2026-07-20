//! Tests for `$/cancelRequest` handling through the real dispatcher.

use super::helpers::*;
use lsp_server::{ErrorCode, Message, RequestId};
use std::time::Duration;

#[test]
fn test_cancel_request_returns_request_cancelled() {
    let mut server = TestLspServer::new();
    server.initialize("file:///workspace");
    let uri = "file:///workspace/doc.qmd";

    // Open a document and flush the open-time publish so the cancel test
    // assertion is anchored to the format response alone.
    server.open_document(uri, "# Heading\n\nBody.\n", "quarto");
    server.pump(Duration::from_secs(2));
    server.drain_client_messages();

    let id = server.send_format_request_raw(42, uri);
    server.send_cancel(id.clone());
    server.pump(Duration::from_secs(2));

    let messages = server.drain_client_messages();
    let response = messages
        .iter()
        .find_map(|msg| match msg {
            Message::Response(resp) if resp.id == id => Some(resp),
            _ => None,
        })
        .expect("expected a response for the format request id");
    let err = response
        .response_result
        .as_ref()
        .expect_err("cancelled request should produce an error response");
    assert_eq!(
        err.code,
        ErrorCode::RequestCanceled as i32,
        "expected RequestCanceled, got code {}: {}",
        err.code,
        err.message
    );
}

#[test]
fn test_cancel_unknown_request_id_is_a_noop() {
    let mut server = TestLspServer::new();
    server.initialize("file:///workspace");

    // Sending a cancel for an id that was never in flight should not panic
    // or produce a spurious response.
    server.send_cancel(RequestId::from(9999));
    server.pump(Duration::from_millis(100));

    let messages = server.drain_client_messages();
    let responses: Vec<_> = messages
        .iter()
        .filter(|m| matches!(m, Message::Response(_)))
        .collect();
    assert!(
        responses.is_empty(),
        "cancel of unknown id should not synthesize a response, got: {responses:?}"
    );
}
