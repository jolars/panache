use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use crate::linter;
use crate::lsp::DocumentState;
use crate::metadata::{
    DocumentMetadata, InlineBibConflict, InlineReferenceDuplicate, YamlError,
    bibliography_range_map, format_bibliography_load_error, inline_bib_conflicts,
    inline_reference_duplicates,
};
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

fn check_bibliography_parse(metadata: &DocumentMetadata, text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let Some(parse) = metadata.bibliography_parse.as_ref() else {
        return diagnostics;
    };
    let range_by_path = bibliography_range_map(metadata);
    let source_ranges = metadata
        .bibliography
        .as_ref()
        .map(|info| info.source_ranges.as_slice())
        .unwrap_or_default();
    let fallback_range = source_ranges.first().cloned().unwrap_or_default();
    let fallback_start = offset_to_position(text, fallback_range.start().into());
    let fallback_end = offset_to_position(text, fallback_range.end().into());

    for error in &parse.index.load_errors {
        let range = range_by_path
            .get(&error.path)
            .copied()
            .unwrap_or(fallback_range);
        let start = offset_to_position(text, range.start().into());
        let end = offset_to_position(text, range.end().into());
        let message = format_bibliography_load_error(&error.message);
        diagnostics.push(Diagnostic {
            range: Range { start, end },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(
                "bibliography-load-error".to_string(),
            )),
            source: Some("panache".to_string()),
            message: format!(
                "Failed to load bibliography {}: {}",
                error.path.display(),
                message
            ),
            ..Default::default()
        });
    }

    for duplicate in &parse.index.duplicates {
        let range = range_by_path
            .get(&duplicate.first.file)
            .or_else(|| range_by_path.get(&duplicate.duplicate.file))
            .copied()
            .unwrap_or(fallback_range);
        let start = offset_to_position(text, range.start().into());
        let end = offset_to_position(text, range.end().into());
        diagnostics.push(Diagnostic {
            range: Range { start, end },
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

    for message in &parse.parse_errors {
        diagnostics.push(Diagnostic {
            range: Range {
                start: fallback_start,
                end: fallback_end,
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(
                "bibliography-parse-error".to_string(),
            )),
            source: Some("panache".to_string()),
            message: format!("Invalid bibliography entry: {}", message),
            ..Default::default()
        });
    }

    diagnostics
}

fn inline_reference_diagnostics(metadata: &DocumentMetadata, text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if metadata.inline_references.is_empty() {
        return diagnostics;
    }
    let duplicates = inline_reference_duplicates(&metadata.inline_references);
    for duplicate in duplicates {
        diagnostics.push(inline_reference_duplicate_diagnostic(&duplicate, text));
    }

    if let Some(parse) = metadata.bibliography_parse.as_ref() {
        let conflicts = inline_bib_conflicts(&metadata.inline_references, &parse.index);
        for conflict in conflicts {
            diagnostics.push(inline_reference_conflict_diagnostic(&conflict, text));
        }
    }

    diagnostics
}

fn inline_reference_duplicate_diagnostic(
    duplicate: &InlineReferenceDuplicate,
    text: &str,
) -> Diagnostic {
    let start = offset_to_position(text, duplicate.duplicate.range.start().into());
    let end = offset_to_position(text, duplicate.duplicate.range.end().into());
    Diagnostic {
        range: Range { start, end },
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String(
            "duplicate-inline-reference-id".to_string(),
        )),
        source: Some("panache".to_string()),
        message: format!("Duplicate inline reference id '{}'", duplicate.key),
        ..Default::default()
    }
}

fn inline_reference_conflict_diagnostic(conflict: &InlineBibConflict, text: &str) -> Diagnostic {
    let start = offset_to_position(text, conflict.inline.range.start().into());
    let end = offset_to_position(text, conflict.inline.range.end().into());
    Diagnostic {
        range: Range { start, end },
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String(
            "duplicate-inline-reference-id".to_string(),
        )),
        source: Some("panache".to_string()),
        message: format!(
            "Duplicate inline reference id '{}' in {} and {}",
            conflict.key,
            conflict.inline.path.display(),
            conflict.bib.file.display()
        ),
        ..Default::default()
    }
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
        all_diagnostics.extend(check_bibliography_parse(metadata, &text));
        all_diagnostics.extend(inline_reference_diagnostics(metadata, &text));
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
                .filter(|d| {
                    !matches!(
                        d.code.as_str(),
                        "bibliography-load-error"
                            | "bibliography-parse-error"
                            | "duplicate-bibliography-key"
                    )
                })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bib::{BibIndex, BibLoadError};
    use crate::metadata::{BibliographyInfo, BibliographyParse, CitationInfo, DocumentMetadata};
    use rowan::{TextRange, TextSize};

    #[test]
    fn bibliography_load_error_uses_source_range() {
        let text = "---\nbibliography: test.bib\n---\n\nText\n";
        let start = text.find("test.bib").unwrap();
        let end = start + "test.bib".len();
        let range = TextRange::new(TextSize::from(start as u32), TextSize::from(end as u32));
        let path = PathBuf::from("/tmp/test.bib");

        let metadata = DocumentMetadata {
            bibliography: Some(BibliographyInfo {
                paths: vec![path.clone()],
                source_ranges: vec![range],
            }),
            metadata_files: Vec::new(),
            bibliography_parse: Some(BibliographyParse {
                index: BibIndex {
                    entries: HashMap::new(),
                    duplicates: Vec::new(),
                    errors: Vec::new(),
                    files: Vec::new(),
                    load_errors: vec![BibLoadError {
                        path: path.clone(),
                        message: "No such file or directory (os error 2)".to_string(),
                    }],
                },
                parse_errors: Vec::new(),
            }),
            inline_references: Vec::new(),
            citations: CitationInfo { keys: Vec::new() },
            title: None,
            raw_yaml: String::new(),
        };

        let diagnostics = check_bibliography_parse(&metadata, text);
        assert_eq!(diagnostics.len(), 1);
        let diag = &diagnostics[0];
        let expected_start = offset_to_position(text, start);
        let expected_end = offset_to_position(text, end);
        assert_eq!(diag.range.start, expected_start);
        assert_eq!(diag.range.end, expected_end);
        assert_eq!(
            diag.message,
            "Failed to load bibliography /tmp/test.bib: File not found"
        );
    }
}
