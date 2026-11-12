use panache::lsp::LspTester;
use serde_json::json;
#[test]
fn notebook_cells_have_independent_protocol_state() {
    let mut server = LspTester::new();
    let book = "file:///tmp/test.ipynb";
    let a = "vscode-notebook-cell:///tmp/test.ipynb#first";
    let b = "vscode-notebook-cell:///tmp/test.ipynb#second";
    server.notify("notebookDocument/didOpen", json!({"notebookDocument":{"uri":book,"notebookType":"jupyter-notebook","version":1,"cells":[{"kind":1,"document":a},{"kind":1,"document":b}]},"cellTextDocuments":[{"uri":a,"languageId":"markdown","version":1,"text":"#   First"},{"uri":b,"languageId":"markdown","version":1,"text":"#   Second"}]}));
    assert_eq!(server.format_document(a).unwrap()[0].new_text, "# First\n");
    assert_eq!(server.format_document(b).unwrap()[0].new_text, "# Second\n");
    server.notify("notebookDocument/didChange", json!({"notebookDocument":{"uri":book,"version":2},"change":{"cells":{"textContent":[{"document":{"uri":a,"version":2},"changes":[{"text":"#   Changed"}]}]}}}));
    assert_eq!(
        server.format_document(a).unwrap()[0].new_text,
        "# Changed\n"
    );
    assert_eq!(server.format_document(b).unwrap()[0].new_text, "# Second\n");
    server.notify("notebookDocument/didChange", json!({"notebookDocument":{"uri":book,"version":3},"change":{"cells":{"data":[{"kind":2,"document":a}]}}}));
    assert!(server.format_document(a).is_none());
    server.notify("notebookDocument/didChange", json!({"notebookDocument":{"uri":book,"version":4},"change":{"cells":{"data":[{"kind":1,"document":a}]}}}));
    assert_eq!(
        server.format_document(a).unwrap()[0].new_text,
        "# Changed\n"
    );
    server.notify(
        "notebookDocument/didClose",
        json!({"notebookDocument":{"uri":book},"cellTextDocuments":[{"uri":a},{"uri":b}]}),
    );
    assert!(server.format_document(a).is_none());
    assert!(server.format_document(b).is_none());
}

#[test]
fn notebook_uses_parent_config_and_exclusions() {
    use lsp_types::Uri;
    use panache::lsp::UriExt;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    std::fs::create_dir(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("panache.toml"), "line-width = 80\n").unwrap();
    std::fs::write(dir.path().join("nested/panache.toml"), "line-width = 20\n").unwrap();
    let parent = Uri::from_file_path(dir.path().join("nested/test.ipynb")).unwrap();
    let cell = "cell:configuration";
    let mut server = LspTester::new();
    server.initialize(Uri::from_file_path(dir.path()).unwrap().as_str());
    server.notify("notebookDocument/didOpen", json!({"notebookDocument":{"uri":parent,"notebookType":"jupyter-notebook","version":1,"cells":[{"kind":1,"document":cell}]},"cellTextDocuments":[{"uri":cell,"languageId":"markdown","version":1,"text":"alpha beta gamma delta epsilon zeta eta theta"}]}));
    let output = server.format_document(cell).unwrap()[0].new_text.clone();
    assert!(output.lines().all(|l| l.len() <= 20));
    std::fs::write(
        dir.path().join("nested/panache.toml"),
        "exclude = ['**/*.ipynb']\n",
    )
    .unwrap();
    assert!(server.format_document(cell).is_none());
}

#[test]
fn notebook_splices_and_utf16_edits_use_cell_coordinates() {
    let mut server = LspTester::new();
    let book = "untitled:Untitled.ipynb";
    let a = "cell:first";
    let b = "cell:second";
    server.notify("notebookDocument/didOpen", json!({"notebookDocument":{"uri":book,"notebookType":"jupyter-notebook","version":1,"cells":[{"kind":1,"document":a}]},"cellTextDocuments":[{"uri":a,"languageId":"markdown","version":1,"text":"😀 old"}]}));
    server.notify("notebookDocument/didChange", json!({"notebookDocument":{"uri":book,"version":2},"change":{"cells":{"textContent":[{"document":{"uri":a,"version":2},"changes":[{"range":{"start":{"line":0,"character":3},"end":{"line":0,"character":6}},"text":"new"}]}]}}}));
    assert_eq!(server.format_document(a).unwrap()[0].new_text, "😀 new\n");
    server.notify("notebookDocument/didChange", json!({"notebookDocument":{"uri":book,"version":3},"change":{"cells":{"structure":{"array":{"start":0,"deleteCount":1,"cells":[{"kind":1,"document":b}]},"didOpen":[{"uri":b,"languageId":"markdown","version":1,"text":"#   Inserted"}],"didClose":[{"uri":a}]}}}}));
    assert!(server.format_document(a).is_none());
    assert_eq!(
        server.format_document(b).unwrap()[0].new_text,
        "# Inserted\n"
    );
    server.notify(
        "notebookDocument/didSave",
        json!({"notebookDocument":{"uri":book}}),
    );
    assert_eq!(
        server.format_document(b).unwrap()[0].new_text,
        "# Inserted\n"
    );
}

#[test]
fn notebook_cells_publish_no_lint_diagnostics() {
    let mut server = LspTester::new();
    server.initialize_pull("file:///tmp");
    let cell = "cell:diagnostics";
    server.notify("notebookDocument/didOpen", json!({"notebookDocument":{"uri":"file:///tmp/test.ipynb","notebookType":"jupyter-notebook","version":1,"cells":[{"kind":1,"document":cell}]},"cellTextDocuments":[{"uri":cell,"languageId":"markdown","version":1,"text":"# A\n\n### B\n\n[missing][ref]\n"}]}));
    server.pump(std::time::Duration::from_millis(300));
    assert!(server.drain_publish_diagnostics(cell).is_empty());
    let report = serde_json::to_value(server.document_diagnostic(cell, None)).unwrap();
    assert_eq!(report["items"], json!([]));
}

#[test]
fn notebook_text_writes_refresh_other_documents_diagnostics() {
    use std::time::Duration;

    let mut server = LspTester::new();
    server.initialize_pull("file:///tmp");
    let document = "file:///tmp/notebook-neighbor.md";
    let book = "file:///tmp/diagnostics.ipynb";
    let cell = "cell:neighbor-diagnostics";
    server.open_document(document, "# A\n\n### B\n", "markdown");
    server.pump(Duration::from_secs(5));
    let report = serde_json::to_value(server.document_diagnostic(document, None)).unwrap();
    assert!(!report["items"].as_array().unwrap().is_empty());
    server.drain_client_messages();

    server.notify("notebookDocument/didOpen", json!({"notebookDocument":{"uri":book,"notebookType":"jupyter-notebook","version":1,"cells":[{"kind":1,"document":cell}]},"cellTextDocuments":[{"uri":cell,"languageId":"markdown","version":1,"text":"# A\n\n### B\n"}]}));
    server.pump(Duration::from_secs(5));
    assert!(server.drain_client_messages().iter().any(|message| {
        matches!(message, lsp_server::Message::Request(request) if request.method == "workspace/diagnostic/refresh")
    }));

    // The preceding pass has consumed its deadline. A notebook text write
    // must schedule a fresh pass for the other open documents even though
    // the notebook cell itself is excluded from linting. Pull refreshes also
    // signal completed passes whose diagnostics have not changed.
    server.notify("notebookDocument/didChange", json!({"notebookDocument":{"uri":book,"version":2},"change":{"cells":{"textContent":[{"document":{"uri":cell,"version":2},"changes":[{"text":"# Changed\n\n### B\n"}]}]}}}));
    server.pump(Duration::from_secs(5));
    assert!(server.drain_client_messages().iter().any(|message| {
        matches!(message, lsp_server::Message::Request(request) if request.method == "workspace/diagnostic/refresh")
    }));
    let report = serde_json::to_value(server.document_diagnostic(cell, None)).unwrap();
    assert_eq!(report["items"], json!([]));
}
