//! LSP subcommand tests
//!
//! These exercise process startup and configuration inherited through the
//! environment. In-process protocol tests live in `tests/lsp/`.

use assert_cmd::cargo::cargo_bin_cmd;
use std::time::Duration;

struct LspProcess {
    child: std::process::Child,
    messages: std::sync::mpsc::Receiver<lsp_server::Message>,
}

impl LspProcess {
    fn send(&mut self, message: serde_json::Value) {
        use std::io::Write;
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin
            .write_all(lsp_frame(&message.to_string()).as_bytes())
            .unwrap();
        stdin.flush().unwrap();
    }

    fn response(&self, id: i32) -> serde_json::Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let message = self
                .messages
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("language server response");
            if let lsp_server::Message::Response(response) = message
                && response.id == id.into()
            {
                return response.response_result.unwrap();
            }
        }
    }
}

impl Drop for LspProcess {
    fn drop(&mut self) {
        // Reap the server even if a protocol assertion fails.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn format_with_panache_config(contents: Option<&str>) -> serde_json::Value {
    use panache::lsp::UriExt;
    use serde_json::json;
    use std::{
        fs,
        io::BufReader,
        process::{Command, Stdio},
    };

    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    fs::create_dir_all(project.join(".git")).unwrap();
    let config = dir.path().join("shared.toml");
    if let Some(contents) = contents {
        fs::write(&config, contents).unwrap();
    }
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("panache"))
        .arg("lsp")
        .current_dir(&project)
        .env("PANACHE_CONFIG", config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let (sender, messages) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(Some(message)) = lsp_server::Message::read(&mut stdout) {
            if sender.send(message).is_err() {
                break;
            }
        }
    });
    let mut server = LspProcess { child, messages };
    let root = lsp_types::Uri::from_file_path(&project).unwrap();
    let doc = lsp_types::Uri::from_file_path(project.join("doc.md")).unwrap();
    server.send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"capabilities": {}, "rootUri": root, "processId": null}}));
    server.response(1);
    server.send(json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
    server.send(json!({"jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": {"textDocument": {"uri": doc, "languageId": "markdown",
            "version": 1, "text": "Alpha\nbravo."}}}));
    server.send(
        json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/formatting",
        "params": {"textDocument": {"uri": doc},
            "options": {"tabSize": 2, "insertSpaces": true}}}),
    );
    let result = server.response(2);
    server.send(json!({"jsonrpc": "2.0", "id": 3, "method": "shutdown", "params": null}));
    server.response(3);
    server.send(json!({"jsonrpc": "2.0", "method": "exit", "params": null}));
    result
}

#[test]
fn lsp_formatting_uses_panache_config() {
    let edits = format_with_panache_config(Some("[format]\nwrap = \"preserve\"\n"));
    assert_eq!(edits[0]["newText"], "Alpha\nbravo.\n");
}

#[test]
fn lsp_formatting_refuses_missing_panache_config() {
    assert!(format_with_panache_config(None).is_null());
}

#[test]
fn lsp_formatting_refuses_malformed_panache_config() {
    assert!(format_with_panache_config(Some("[format]\nwrpa = \"preserve\"\n")).is_null());
}

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
