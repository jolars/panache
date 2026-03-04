use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use crate::linter;
use crate::lsp::DocumentState;
use crate::metadata::{DocumentMetadata, YamlError};
use crate::syntax::SyntaxNode;

use super::super::conversions::{convert_diagnostic, offset_to_position};
use super::super::helpers::get_config;

fn lint_included_documents(
    root_uri: &Uri,
    text: &str,
    tree: &SyntaxNode,
    config: &crate::Config,
    graph: &crate::includes::ProjectGraph,
) -> Vec<(Uri, Vec<Diagnostic>)> {
    let Some(doc_path) = root_uri.to_file_path() else {
        return Vec::new();
    };
    let base_dir = doc_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let project_root = crate::includes::find_quarto_root(&doc_path);
    let resolution =
        crate::includes::collect_includes(tree, text, base_dir, project_root.as_deref(), config);
    let mut results = Vec::new();
    let mut root_diagnostics: Vec<Diagnostic> = resolution
        .diagnostics
        .iter()
        .map(|d| convert_diagnostic(d, text))
        .collect();

    if let Some(extra) = graph.diagnostics().get(doc_path.as_ref()) {
        root_diagnostics.extend(extra.iter().map(|d| convert_diagnostic(d, text)));
    }

    for include in resolution.includes {
        match std::fs::read_to_string(&include.path) {
            Ok(include_text) => {
                let include_uri =
                    Uri::from_file_path(&include.path).unwrap_or_else(|| root_uri.clone());
                let include_tree = crate::parse(&include_text, Some(config.clone()));
                let include_metadata =
                    crate::metadata::extract_project_metadata(&include_tree, &include.path).ok();
                let include_diagnostics = linter::lint_with_metadata(
                    &include_tree,
                    &include_text,
                    config,
                    include_metadata.as_ref(),
                );
                let mut mapped: Vec<Diagnostic> = include_diagnostics
                    .iter()
                    .map(|d| convert_diagnostic(d, &include_text))
                    .collect();
                if let Some(extra) = graph.diagnostics().get(&include.path) {
                    mapped.extend(extra.iter().map(|d| convert_diagnostic(d, &include_text)));
                }
                results.push((include_uri, mapped));
            }
            Err(err) => {
                let diag = crate::includes::include_read_error_diagnostic(
                    text,
                    include.range,
                    &include.path,
                    &err.to_string(),
                );
                root_diagnostics.push(convert_diagnostic(&diag, text));
            }
        }
    }

    results.push((root_uri.clone(), root_diagnostics));
    results
}

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

fn check_bibliography_parse(metadata: &DocumentMetadata, _text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let Some(parse) = metadata.bibliography_parse.as_ref() else {
        return diagnostics;
    };

    for error in &parse.index.load_errors {
        diagnostics.push(Diagnostic {
            range: Range {
                start: Position::default(),
                end: Position::default(),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(
                "bibliography-load-error".to_string(),
            )),
            source: Some("panache".to_string()),
            message: format!(
                "Failed to load bibliography {}: {}",
                error.path.display(),
                error.message
            ),
            ..Default::default()
        });
    }

    for duplicate in &parse.index.duplicates {
        diagnostics.push(Diagnostic {
            range: Range {
                start: Position::default(),
                end: Position::default(),
            },
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String(
                "duplicate-bibliography-key".to_string(),
            )),
            source: Some("panache".to_string()),
            message: format!(
                "Duplicate bibliography key '{}' in {} and {}",
                duplicate.key,
                duplicate.first.file.display(),
                duplicate.duplicate.file.display()
            ),
            ..Default::default()
        });
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
    let metadata = doc_state.metadata.clone();
    let graph = doc_state.graph.clone();
    let mut all_diagnostics = Vec::new();

    // Check for YAML metadata errors
    if let Some(ref metadata) = doc_state.metadata {
        // Check for missing bibliography files
        all_diagnostics.extend(check_bibliography_files(metadata, &text));
        all_diagnostics.extend(check_bibliography_parse(metadata, &text));
    } else {
        // Metadata parsing failed - try to get the error
        // Re-parse to get the error (this is a bit wasteful, but errors are rare)
        if let Some(file_path) = uri.to_file_path() {
            let tree = crate::parse(&text, None);
            if let Err(yaml_error) = crate::metadata::extract_project_metadata(&tree, &file_path)
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
    let config_clone = config.clone();
    let metadata = metadata.clone();
    let has_external_linters = !config.linters.is_empty();

    let diagnostics = if has_external_linters {
        // Use async runtime for external linters
        tokio::task::spawn_blocking(move || {
            let tree = crate::parse(&text_clone, Some(config_clone.clone()));
            // Create a runtime for the async lint function
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(linter::lint_with_external_and_metadata(
                &tree,
                &text_clone,
                &config_clone,
                metadata.as_ref(),
            ))
        })
        .await
    } else {
        // Regular sync lint for built-in rules only
        tokio::task::spawn_blocking(move || {
            let tree = crate::parse(&text_clone, Some(config_clone.clone()));
            linter::lint_with_metadata(&tree, &text_clone, &config_clone, metadata.as_ref())
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

            let include_diagnostics = lint_included_documents(
                &uri,
                &text,
                &crate::parse(&text, Some(config.clone())),
                &config,
                &graph,
            );

            let mut published_root = false;
            for (target_uri, diags) in include_diagnostics {
                if target_uri == uri {
                    let mut merged = all_diagnostics.clone();
                    merged.extend(diags);
                    client.publish_diagnostics(uri.clone(), merged, None).await;
                    published_root = true;
                } else {
                    client.publish_diagnostics(target_uri, diags, None).await;
                }
            }

            if !published_root {
                client.publish_diagnostics(uri, all_diagnostics, None).await;
            }
        }
        Err(e) => {
            client
                .log_message(MessageType::ERROR, format!("Linting task failed: {}", e))
                .await;
        }
    }
}
