//! Tests for `$/cancelRequest` handling through the real dispatcher.

use super::helpers::*;
use lsp_server::{Message, RequestId};
use std::time::Duration;

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
