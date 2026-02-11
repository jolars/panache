use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

use crate::linter;
use crate::linter::Severity as PanacheSeverity;

/// Helper to convert LSP UTF-16 position to byte offset in UTF-8 string
fn position_to_offset(text: &str, position: Position) -> Option<usize> {
    let mut offset = 0;
    let mut current_line = 0;

    for line in text.lines() {
        if current_line == position.line {
            // LSP uses UTF-16 code units, Rust uses UTF-8 bytes
            let mut utf16_offset = 0;
            for (byte_idx, ch) in line.char_indices() {
                if utf16_offset >= position.character as usize {
                    return Some(offset + byte_idx);
                }
                utf16_offset += ch.len_utf16();
            }
            // Position is at or past end of line
            return Some(offset + line.len());
        }
        // +1 for newline character
        offset += line.len() + 1;
        current_line += 1;
    }

    // Position is beyond document end
    if current_line == position.line {
        // Empty last line or position at very end
        return Some(offset);
    }

    None
}

/// Convert byte offset to LSP Position (line/character in UTF-16)
fn offset_to_position(text: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut character = 0;
    let mut current_offset = 0;

    for text_line in text.lines() {
        if current_offset + text_line.len() >= offset {
            // Offset is in this line
            let line_offset = offset - current_offset;
            let line_slice = &text_line[..line_offset];
            character = line_slice.chars().map(|c| c.len_utf16()).sum::<usize>() as u32;
            break;
        }
        current_offset += text_line.len() + 1; // +1 for newline
        line += 1;
    }

    Position {
        line: line as u32,
        character,
    }
}

/// Convert panache Diagnostic to LSP Diagnostic
fn convert_diagnostic(diag: &linter::Diagnostic, text: &str) -> Diagnostic {
    let start = offset_to_position(text, diag.location.range.start().into());
    let end = offset_to_position(text, diag.location.range.end().into());

    let severity = match diag.severity {
        PanacheSeverity::Error => DiagnosticSeverity::ERROR,
        PanacheSeverity::Warning => DiagnosticSeverity::WARNING,
        PanacheSeverity::Info => DiagnosticSeverity::INFORMATION,
    };

    Diagnostic {
        range: Range { start, end },
        severity: Some(severity),
        code: Some(NumberOrString::String(diag.code.clone())),
        source: Some("panache".to_string()),
        message: diag.message.clone(),
        ..Default::default()
    }
}

/// Apply a single content change to text
fn apply_content_change(text: &str, change: &TextDocumentContentChangeEvent) -> String {
    match &change.range {
        Some(range) => {
            // Incremental edit with range
            let start_offset = position_to_offset(text, range.start).unwrap_or(0);
            let end_offset = position_to_offset(text, range.end).unwrap_or(text.len());

            let mut result =
                String::with_capacity(text.len() - (end_offset - start_offset) + change.text.len());
            result.push_str(&text[..start_offset]);
            result.push_str(&change.text);
            result.push_str(&text[end_offset..]);
            result
        }
        None => {
            // Full document update (fallback)
            change.text.clone()
        }
    }
}

pub struct PanacheLsp {
    client: Client,
    // Use String keys since Uri doesn't implement Send
    document_map: Arc<Mutex<HashMap<String, String>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
}

impl PanacheLsp {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            document_map: Arc::new(Mutex::new(HashMap::new())),
            workspace_root: Arc::new(Mutex::new(None)),
        }
    }

    async fn load_config(&self) -> crate::Config {
        let workspace_root = self.workspace_root.lock().await;
        if let Some(root) = workspace_root.as_ref() {
            match crate::config::load(None, root, None) {
                Ok((config, path)) => {
                    if let Some(p) = path {
                        self.client
                            .log_message(
                                MessageType::INFO,
                                format!("Loaded config from {}", p.display()),
                            )
                            .await;
                    }
                    return config;
                }
                Err(e) => {
                    self.client
                        .log_message(
                            MessageType::WARNING,
                            format!("Failed to load config: {}", e),
                        )
                        .await;
                }
            }
        }
        crate::Config::default()
    }

    /// Parse document and run linter, then publish diagnostics
    async fn lint_and_publish(&self, uri: Uri, text: String) {
        let config = self.load_config().await;

        // Parse and lint in blocking task
        let text_clone = text.clone();
        let diagnostics = tokio::task::spawn_blocking(move || {
            let tree = crate::parse(&text_clone, Some(config.clone()));
            linter::lint(&tree, &text_clone, &config)
        })
        .await;

        match diagnostics {
            Ok(panache_diagnostics) => {
                let lsp_diagnostics: Vec<Diagnostic> = panache_diagnostics
                    .iter()
                    .map(|d| convert_diagnostic(d, &text))
                    .collect();

                self.client
                    .publish_diagnostics(uri, lsp_diagnostics, None)
                    .await;
            }
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Linting task failed: {}", e))
                    .await;
            }
        }
    }
}

impl LanguageServer for PanacheLsp {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Store workspace root for config discovery
        // Try workspace_folders first, fall back to deprecated root_uri
        if let Some(folders) = params.workspace_folders
            && let Some(folder) = folders.first()
            && let Some(path) = folder.uri.to_file_path()
        {
            *self.workspace_root.lock().await = Some(path.into_owned());
        } else {
            #[allow(deprecated)]
            if let Some(root_uri) = params.root_uri
                && let Some(path) = root_uri.to_file_path()
            {
                *self.workspace_root.lock().await = Some(path.into_owned());
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        ..Default::default()
                    },
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "panache-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "panache LSP server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        let text = params.text_document.text;

        self.document_map
            .lock()
            .await
            .insert(uri.clone(), text.clone());

        self.client
            .log_message(MessageType::INFO, format!("Opened document: {}", uri))
            .await;

        // Run linting and publish diagnostics
        self.lint_and_publish(params.text_document.uri, text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        // Apply incremental changes sequentially
        let text = {
            let mut document_map = self.document_map.lock().await;
            if let Some(text) = document_map.get_mut(&uri) {
                for change in params.content_changes {
                    *text = apply_content_change(text, &change);
                }
                text.clone()
            } else {
                return;
            }
        };

        // Run linting and publish diagnostics
        self.lint_and_publish(params.text_document.uri, text).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        self.document_map.lock().await.remove(&uri);

        // Clear diagnostics
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let uri_string = uri.to_string();

        self.client
            .log_message(
                MessageType::INFO,
                format!("Formatting request for {}", uri_string),
            )
            .await;

        // Get document content (clone to avoid holding lock across await)
        let text = {
            let document_map = self.document_map.lock().await;
            match document_map.get(&uri_string) {
                Some(t) => t.clone(),
                None => {
                    self.client
                        .log_message(
                            MessageType::ERROR,
                            format!("Document not found: {}", uri_string),
                        )
                        .await;
                    return Ok(None);
                }
            }
        };

        // Load config
        let config = self.load_config().await;

        // Run formatting in a blocking task (because rowan::SyntaxNode isn't Send)
        // but use format_async inside to support external formatters
        let text_clone = text.clone();
        let formatted = tokio::task::spawn_blocking(move || {
            // Create a new tokio runtime for async external formatters
            tokio::runtime::Runtime::new()
                .expect("Failed to create runtime")
                .block_on(crate::format_async(&text_clone, Some(config)))
        })
        .await
        .map_err(|_| tower_lsp_server::jsonrpc::Error::internal_error())?;

        // If the content didn't change, return None
        if formatted == text {
            return Ok(None);
        }

        // Calculate the range to replace (entire document)
        let lines: Vec<&str> = text.lines().collect();
        let end_line = lines.len().saturating_sub(1) as u32;
        let end_char = lines.last().map(|l| l.len()).unwrap_or(0) as u32;

        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: end_line,
                character: end_char,
            },
        };

        Ok(Some(vec![TextEdit {
            range,
            new_text: formatted,
        }]))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri_string = params.text_document.uri.to_string();

        // Get document content
        let text = {
            let document_map = self.document_map.lock().await;
            match document_map.get(&uri_string) {
                Some(t) => t.clone(),
                None => return Ok(None),
            }
        };

        // Load config and run linter
        let config = self.load_config().await;
        let text_clone = text.clone();
        let diagnostics = tokio::task::spawn_blocking(move || {
            let tree = crate::parse(&text_clone, Some(config.clone()));
            linter::lint(&tree, &text_clone, &config)
        })
        .await
        .map_err(|_| tower_lsp_server::jsonrpc::Error::internal_error())?;

        // Convert fixes to code actions
        let mut actions = Vec::new();
        for diag in diagnostics {
            if let Some(ref fix) = diag.fix {
                let mut changes = HashMap::new();
                let text_edits: Vec<TextEdit> = fix
                    .edits
                    .iter()
                    .map(|edit| {
                        let start = offset_to_position(&text, edit.range.start().into());
                        let end = offset_to_position(&text, edit.range.end().into());
                        TextEdit {
                            range: Range { start, end },
                            new_text: edit.replacement.clone(),
                        }
                    })
                    .collect();

                changes.insert(params.text_document.uri.clone(), text_edits);

                let action = CodeAction {
                    title: fix.message.clone(),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![convert_diagnostic(&diag, &text)]),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                };

                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }

        Ok(Some(actions))
    }
}

pub async fn run() -> std::io::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(PanacheLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test helper functions directly (no mocking needed)

    #[test]
    fn test_offset_to_position_simple() {
        let text = "hello\nworld\n";

        let pos = offset_to_position(text, 0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);

        let pos = offset_to_position(text, 3);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 3);

        let pos = offset_to_position(text, 6);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);

        let pos = offset_to_position(text, 9);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 3);
    }

    #[test]
    fn test_offset_to_position_utf16() {
        // "café" = 5 UTF-8 bytes, 4 UTF-16 code units
        let text = "café\n";

        let pos = offset_to_position(text, 0);
        assert_eq!(pos.character, 0);

        let pos = offset_to_position(text, 3);
        assert_eq!(pos.character, 3);

        // After é (2 UTF-8 bytes, but 1 UTF-16 code unit)
        let pos = offset_to_position(text, 5);
        assert_eq!(pos.character, 4);
    }

    #[test]
    fn test_offset_to_position_emoji() {
        // "👋" = 4 UTF-8 bytes, 2 UTF-16 code units (surrogate pair)
        let text = "hi👋\n";

        let pos = offset_to_position(text, 2);
        assert_eq!(pos.character, 2);

        // After emoji (6 bytes total, 4 UTF-16 code units)
        let pos = offset_to_position(text, 6);
        assert_eq!(pos.character, 4);
    }

    #[test]
    fn test_convert_diagnostic_basic() {
        use crate::linter::diagnostics::{Diagnostic as PanacheDiagnostic, Location, Severity};
        use rowan::TextRange;

        let text = "# H1\n\n### H3\n";

        let diag = PanacheDiagnostic {
            severity: Severity::Warning,
            location: Location {
                line: 3,
                column: 1,
                range: TextRange::new(7.into(), 14.into()),
            },
            message: "Heading level skipped from h1 to h3".to_string(),
            code: "heading-hierarchy".to_string(),
            fix: None,
        };

        let lsp_diag = convert_diagnostic(&diag, text);

        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            lsp_diag.code,
            Some(NumberOrString::String("heading-hierarchy".to_string()))
        );
        assert_eq!(lsp_diag.source, Some("panache".to_string()));
        assert!(lsp_diag.message.contains("h1 to h3"));

        // Verify range conversion
        assert_eq!(lsp_diag.range.start.line, 2); // Line 3 in text becomes line 2 (0-indexed)
    }

    #[test]
    fn test_convert_diagnostic_severity() {
        use crate::linter::diagnostics::{Diagnostic as PanacheDiagnostic, Location, Severity};
        use rowan::TextRange;

        let text = "test\n";

        let error_diag = PanacheDiagnostic {
            severity: Severity::Error,
            location: Location {
                line: 1,
                column: 1,
                range: TextRange::new(0.into(), 4.into()),
            },
            message: "Error".to_string(),
            code: "test-error".to_string(),
            fix: None,
        };

        let lsp_diag = convert_diagnostic(&error_diag, text);
        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::ERROR));

        let info_diag = PanacheDiagnostic {
            severity: Severity::Info,
            location: Location {
                line: 1,
                column: 1,
                range: TextRange::new(0.into(), 4.into()),
            },
            message: "Info".to_string(),
            code: "test-info".to_string(),
            fix: None,
        };

        let lsp_diag = convert_diagnostic(&info_diag, text);
        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::INFORMATION));
    }

    // Original position_to_offset tests
    #[test]
    fn test_position_to_offset_simple() {
        let text = "hello\nworld\n";

        // Start of first line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 0
                }
            ),
            Some(0)
        );

        // Middle of first line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 3
                }
            ),
            Some(3)
        );

        // End of first line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 5
                }
            ),
            Some(5)
        );

        // Start of second line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 1,
                    character: 0
                }
            ),
            Some(6)
        );

        // Middle of second line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 1,
                    character: 3
                }
            ),
            Some(9)
        );
    }

    #[test]
    fn test_position_to_offset_utf8() {
        // "café" = 5 UTF-8 bytes, 4 UTF-16 code units (é = 2 bytes, 1 code unit)
        let text = "café\nworld\n";

        // Start of line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 0
                }
            ),
            Some(0)
        );

        // After 'c' (1 byte, 1 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 1
                }
            ),
            Some(1)
        );

        // After 'ca' (2 bytes, 2 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 2
                }
            ),
            Some(2)
        );

        // After 'caf' (3 bytes, 3 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 3
                }
            ),
            Some(3)
        );

        // After 'café' (5 bytes, 4 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 4
                }
            ),
            Some(5)
        );
    }

    #[test]
    fn test_position_to_offset_emoji() {
        // "👋" = 4 UTF-8 bytes, 2 UTF-16 code units (surrogate pair)
        let text = "hi👋\n";

        // After "hi" (2 bytes, 2 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 2
                }
            ),
            Some(2)
        );

        // After "hi👋" (6 bytes, 4 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 4
                }
            ),
            Some(6)
        );
    }

    #[test]
    fn test_apply_content_change_insert() {
        let text = "hello world";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 6,
                },
                end: Position {
                    line: 0,
                    character: 6,
                },
            }),
            range_length: None,
            text: "beautiful ".to_string(),
        };

        assert_eq!(apply_content_change(text, &change), "hello beautiful world");
    }

    #[test]
    fn test_apply_content_change_delete() {
        let text = "hello beautiful world";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 5,
                },
                end: Position {
                    line: 0,
                    character: 15,
                },
            }),
            range_length: None,
            text: String::new(),
        };

        assert_eq!(apply_content_change(text, &change), "hello world");
    }

    #[test]
    fn test_apply_content_change_replace() {
        let text = "hello world";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 5,
                },
            }),
            range_length: None,
            text: "goodbye".to_string(),
        };

        assert_eq!(apply_content_change(text, &change), "goodbye world");
    }

    #[test]
    fn test_apply_content_change_full_document() {
        let text = "old content";
        let change = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "new content".to_string(),
        };

        assert_eq!(apply_content_change(text, &change), "new content");
    }

    #[test]
    fn test_apply_content_change_multiline() {
        let text = "line1\nline2\nline3";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 1,
                    character: 2,
                },
                end: Position {
                    line: 2,
                    character: 2,
                },
            }),
            range_length: None,
            text: "NEW\nLINE".to_string(),
        };

        assert_eq!(apply_content_change(text, &change), "line1\nliNEW\nLINEne3");
    }
}
