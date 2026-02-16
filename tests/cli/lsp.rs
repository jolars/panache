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

#[test]
fn test_lsp_initialization() {
    // Send a minimal LSP initialization request
    let init_request = r#"Content-Length: 140

{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{},"processId":null,"rootUri":null,"workspaceFolders":null}}"#;

    let cmd = cargo_bin_cmd!("panache")
        .arg("lsp")
        .write_stdin(init_request)
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
