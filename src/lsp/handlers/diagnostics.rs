use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use crate::linter;
use crate::lsp::DocumentState;
use crate::metadata::{DocumentMetadata, YamlError};

use super::super::conversions::{convert_diagnostic, offset_to_position};
use super::super::helpers::get_config;

/// Create LSP diagnostic from YAML parse error
fn yaml_error_to_diagnostic(error: &YamlError, _text: &str) -> Diagnostic {
    match error {
        YamlError::ParseError {
            message,
            line,
            column,
        } => {
            // Convert 1-based line/column to 0-based Position
            let position = Position {
                line: line.saturating_sub(1) as u32,
                character: column.saturating_sub(1) as u32,
            };
            Diagnostic {
                range: Range {
                    start: position,
                    end: position,
                },
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String("yaml-parse-error".to_string())),
                source: Some("panache".to_string()),
                message: format!("YAML parse error: {}", message),
                ..Default::default()
            }
        }
        YamlError::StructureError(msg) => Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("yaml-structure-error".to_string())),
            source: Some("panache".to_string()),
            message: format!("YAML structure error: {}", msg),
            ..Default::default()
        },
        YamlError::NotFound(_) => {
            // No diagnostic for missing frontmatter
            Diagnostic {
                range: Range {
                    start: Position::default(),
                    end: Position::default(),
                },
                severity: Some(DiagnosticSeverity::HINT),
                code: None,
                source: Some("panache".to_string()),
                message: String::new(),
                ..Default::default()
            }
        }
    }
}

/// Check bibliography paths and create diagnostics for missing files
fn check_bibliography_files(metadata: &DocumentMetadata, text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if let Some(ref bib_info) = metadata.bibliography {
        for (path, range) in bib_info.paths.iter().zip(bib_info.source_ranges.iter()) {
            if !path.exists() {
                let start_pos = offset_to_position(text, range.start().into());
                let end_pos = offset_to_position(text, range.end().into());

                diagnostics.push(Diagnostic {
                    range: Range {
                        start: start_pos,
                        end: end_pos,
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(NumberOrString::String("missing-bibliography".to_string())),
                    source: Some("panache".to_string()),
                    message: format!("Bibliography file not found: {}", path.display()),
                    ..Default::default()
                });
            }
        }
    }

    diagnostics
}

/// Parse document and run linter, then publish diagnostics
pub(crate) async fn lint_and_publish(
    client: &Client,
    document_map: &Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: &Arc<Mutex<Option<PathBuf>>>,
    uri: Uri,
) {
    // Get document state
    let doc_state = {
        let map = document_map.lock().await;
        map.get(&uri.to_string()).cloned()
    };

    let Some(doc_state) = doc_state else {
        client
            .log_message(
                MessageType::WARNING,
                format!("Document not found: {}", *uri),
            )
            .await;
        return;
    };

    let text = doc_state.text;
    let mut all_diagnostics = Vec::new();

    // Check for YAML metadata errors
    if let Some(ref metadata) = doc_state.metadata {
        // Check for missing bibliography files
        all_diagnostics.extend(check_bibliography_files(metadata, &text));
    } else {
        // Metadata parsing failed - try to get the error
        // Re-parse to get the error (this is a bit wasteful, but errors are rare)
        if let Some(file_path) = uri.to_file_path() {
            let tree = crate::parse(&text, None);
            if let Err(yaml_error) = crate::metadata::extract_metadata(&tree, &file_path)
                && !matches!(yaml_error, YamlError::NotFound(_))
            {
                all_diagnostics.push(yaml_error_to_diagnostic(&yaml_error, &text));
            }
        }
    }

    // Use helper to load config
    let config = get_config(client, workspace_root, &uri).await;

    // Parse and lint (including external linters) in blocking task
    let text_clone = text.clone();
    let has_external_linters = !config.linters.is_empty();

    let diagnostics = if has_external_linters {
        // Use async runtime for external linters
        tokio::task::spawn_blocking(move || {
            let tree = crate::parse(&text_clone, Some(config.clone()));
            // Create a runtime for the async lint function
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(linter::lint_with_external(&tree, &text_clone, &config))
        })
        .await
    } else {
        // Regular sync lint for built-in rules only
        tokio::task::spawn_blocking(move || {
            let tree = crate::parse(&text_clone, Some(config.clone()));
            linter::lint(&tree, &text_clone, &config)
        })
        .await
    };

    match diagnostics {
        Ok(panache_diagnostics) => {
            let lsp_diagnostics: Vec<Diagnostic> = panache_diagnostics
                .iter()
                .map(|d| convert_diagnostic(d, &text))
                .collect();

            all_diagnostics.extend(lsp_diagnostics);
            client.publish_diagnostics(uri, all_diagnostics, None).await;
        }
        Err(e) => {
            client
                .log_message(MessageType::ERROR, format!("Linting task failed: {}", e))
                .await;
        }
    }
}
