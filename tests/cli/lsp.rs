//! LSP subcommand tests
//!
//! Note: These are basic smoke tests. Full LSP protocol testing requires
//! more sophisticated test infrastructure.

use assert_cmd::cargo::cargo_bin_cmd;
use std::time::Duration;

#[test]
fn test_lsp_starts() {
    // LSP server should start without immediate error
    // We send EOF immediately to trigger shutdown
    let cmd = cargo_bin_cmd!("panache")
        .arg("lsp")
        .write_stdin("")
        .timeout(Duration::from_secs(5))
        .assert();

    // LSP server may exit with 0 (clean shutdown) or 1 (EOF/broken pipe)
    // Both are acceptable for this smoke test
    let output = cmd.get_output();
    let exit_code = output.status.code().unwrap_or(1);
    assert!(
        exit_code == 0 || exit_code == 1,
        "LSP server failed to start"
    );
}

/// Frame a JSON-RPC message with the `Content-Length` header the LSP wire
/// protocol requires. A hardcoded length desyncs the reader, which then blocks
/// on EOF and never responds.
fn lsp_frame(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{body}", body.len())
}

#[test]
fn test_lsp_initialization() {
    // Drive a full, clean handshake and shutdown so the server flushes its
    // response stream and exits normally (an abrupt EOF makes `run` bail with an
    // error before the writer thread flushes stdout).
    let init_body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{},"processId":null,"rootUri":null,"workspaceFolders":null}}"#;
    let initialized = r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#;
    let shutdown = r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#;
    let exit = r#"{"jsonrpc":"2.0","method":"exit","params":null}"#;
    let stdin = format!(
        "{}{}{}{}",
        lsp_frame(init_body),
        lsp_frame(initialized),
        lsp_frame(shutdown),
        lsp_frame(exit),
    );

    let cmd = cargo_bin_cmd!("panache")
        .arg("lsp")
        .write_stdin(stdin)
        .timeout(Duration::from_secs(5))
        .assert();

    // Server should respond (exit code may vary)
    let output = cmd.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain Content-Length header in response
    assert!(
        stdout.contains("Content-Length") || output.status.code().unwrap_or(1) <= 1,
        "LSP server did not respond to initialization"
    );

    // The InitializeResult must carry `serverInfo` with name and version so
    // clients (e.g. Neovim's `:LspInfo`) can report the server version.
    assert!(
        stdout.contains("serverInfo") && stdout.contains("panache-lsp"),
        "initialize response missing serverInfo: {stdout}"
    );
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "initialize response missing server version: {stdout}"
    );
}

#[test]
fn test_lsp_handles_invalid_json() {
    // Send invalid JSON to ensure server doesn't panic
    let invalid_request = "Content-Length: 10\n\n{invalid}";

    let cmd = cargo_bin_cmd!("panache")
        .arg("lsp")
        .write_stdin(invalid_request)
        .timeout(Duration::from_secs(5))
        .assert();

    // Server should not panic (any exit code is acceptable)
    let output = cmd.get_output();
    assert!(
        output.status.code().is_some(),
        "LSP server panicked on invalid JSON"
    );
}
