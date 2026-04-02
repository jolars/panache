use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use super::super::conversions::convert_diagnostic;
use super::super::helpers::get_config;
use crate::lsp::DocumentState;

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
    let mut all_diagnostics = Vec::new();

    // Use helper to load config
    let config = get_config(client, workspace_root, &uri).await;
    let lint_plan = {
        let path = doc_state
            .path
            .clone()
            .or_else(|| uri.to_file_path().map(|p| p.into_owned()))
            .unwrap_or_else(|| PathBuf::from("<memory>"));
        let db = salsa_db.lock().await;
        crate::salsa::built_in_lint_plan(&*db, doc_state.salsa_file, doc_state.salsa_config, path)
            .clone()
    };
    let mut panache_diagnostics = lint_plan.diagnostics;
    let external_jobs = lint_plan.external_jobs;

    #[cfg(not(target_arch = "wasm32"))]
    if !external_jobs.is_empty() {
        let registry = Arc::new(crate::linter::external_linters::ExternalLinterRegistry::new());
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
                    &job.language,
                    &job.content,
                    &input,
                    registry.as_ref(),
                    Some(job.mappings.as_slice()),
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
