use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use super::super::conversions::{convert_diagnostic, offset_to_position};
use super::super::helpers::get_config;
use crate::linter;
use crate::lsp::DocumentState;
use crate::metadata::{
    DocumentMetadata, InlineBibConflict, InlineReferenceDuplicate, YamlError,
    bibliography_range_map, format_bibliography_load_error, inline_bib_conflicts,
    inline_reference_duplicates,
};

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
            conflict.bib.source_file.display()
        ),
        ..Default::default()
    }
}

/// Parse document and run linter, then publish diagnostics
pub(crate) async fn lint_and_publish(
    client: &Client,
    document_map: &Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
    workspace_root: &Arc<Mutex<Option<PathBuf>>>,
    uri: Uri,
) {
    log::debug!("lint_and_publish uri={}", *uri);
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

    let text = {
        let db = salsa_db.lock().await;
        doc_state.salsa_file.text(&*db).clone()
    };
    let metadata = if doc_state.yaml_ok {
        if let Some(path) = doc_state.path.clone() {
            let db = salsa_db.lock().await;
            Some(
                crate::salsa::metadata(&*db, doc_state.salsa_file, doc_state.salsa_config, path)
                    .clone(),
            )
        } else {
            None
        }
    } else {
        None
    };
    let mut all_diagnostics = Vec::new();
    // Check for YAML metadata errors
    if let Some(ref metadata) = metadata {
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

    #[derive(Debug)]
    struct ExternalLintJob {
        linter_name: String,
        content: String,
        mappings: Vec<crate::linter::code_block_collector::BlockMapping>,
    }

    // Phase A (blocking): parse + built-in lint + collect external jobs (no .await while holding rowan)
    let text_clone = text.clone();
    let config_clone = config.clone();
    let metadata_clone = metadata.clone();
    let phase_a = tokio::task::spawn_blocking(move || {
        let tree = crate::parse(&text_clone, Some(config_clone.clone()));
        let mut diagnostics =
            linter::lint_with_metadata(&tree, &text_clone, &config_clone, metadata_clone.as_ref());

        let mut jobs = Vec::new();
        if !config_clone.linters.is_empty() {
            let code_blocks = crate::utils::collect_code_blocks(&tree, &text_clone);
            for (language, linter_name) in &config_clone.linters {
                let Some(blocks) = code_blocks.get(language) else {
                    continue;
                };
                if blocks.is_empty() {
                    continue;
                }

                let concatenated =
                    crate::linter::code_block_collector::concatenate_with_blanks_and_mapping(
                        blocks,
                    );
                jobs.push(ExternalLintJob {
                    linter_name: linter_name.clone(),
                    content: concatenated.content,
                    mappings: concatenated.mappings,
                });
            }
        }

        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
        (diagnostics, jobs)
    })
    .await;

    match phase_a {
        Ok((mut panache_diagnostics, external_jobs)) => {
            #[cfg(not(target_arch = "wasm32"))]
            if !external_jobs.is_empty() {
                let registry =
                    Arc::new(crate::linter::external_linters::ExternalLinterRegistry::new());
                let max_parallel = config.external_max_parallel.max(1);
                let semaphore = Arc::new(Semaphore::new(max_parallel));
                let mut join_set = tokio::task::JoinSet::new();

                for job in external_jobs {
                    let Ok(permit) = semaphore.clone().acquire_owned().await else {
                        break;
                    };
                    let registry = registry.clone();
                    let input = text.clone();
                    join_set.spawn(async move {
                        let _permit = permit;
                        crate::linter::external_linters::run_linter(
                            &job.linter_name,
                            &job.content,
                            &input,
                            registry.as_ref(),
                            Some(&job.mappings),
                        )
                        .await
                    });
                }

                while let Some(res) = join_set.join_next().await {
                    match res {
                        Ok(Ok(diags)) => panache_diagnostics.extend(diags),
                        Ok(Err(e)) => log::warn!("External linter failed: {}", e),
                        Err(e) => log::warn!("External linter task join error: {}", e),
                    }
                }

                panache_diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
            }

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

            let mut published_root = false;
            let by_path: HashMap<PathBuf, Vec<crate::linter::diagnostics::Diagnostic>> = {
                let db = salsa_db.lock().await;
                let root_path = uri
                    .to_file_path()
                    .map(|p| p.into_owned())
                    .unwrap_or_else(|| PathBuf::from("<memory>"));
                let mut by_path: HashMap<PathBuf, Vec<crate::linter::diagnostics::Diagnostic>> =
                    HashMap::new();
                for entry in crate::salsa::project_graph::accumulated::<crate::salsa::GraphDiagnostic>(
                    &*db,
                    doc_state.salsa_file,
                    doc_state.salsa_config,
                    root_path.clone(),
                ) {
                    by_path
                        .entry(entry.0.path.clone())
                        .or_default()
                        .push(entry.0.diagnostic.clone());
                }
                by_path.entry(root_path).or_default();
                by_path
            };

            for (path, diags) in by_path {
                if path.as_os_str() == "<memory>" {
                    continue;
                }
                let target_uri = Uri::from_file_path(&path).unwrap_or_else(|| uri.clone());

                let target_text = if target_uri == uri {
                    text.clone()
                } else {
                    let Some(target_state) = document_map
                        .lock()
                        .await
                        .get(&target_uri.to_string())
                        .cloned()
                    else {
                        continue;
                    };
                    let db = salsa_db.lock().await;
                    target_state.salsa_file.text(&*db).clone()
                };

                let mapped: Vec<Diagnostic> = diags
                    .iter()
                    .map(|d| convert_diagnostic(d, &target_text))
                    .collect();

                if target_uri == uri {
                    let mut merged = all_diagnostics.clone();
                    merged.extend(mapped);
                    client.publish_diagnostics(uri.clone(), merged, None).await;
                    published_root = true;
                } else {
                    client.publish_diagnostics(target_uri, mapped, None).await;
                }
            }

            if !published_root {
                client.publish_diagnostics(uri, all_diagnostics, None).await;
            }
        }
        Err(_) => {
            client
                .log_message(
                    MessageType::ERROR,
                    "Failed to join blocking lint task".to_string(),
                )
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
