use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::ls_types::Uri;

use crate::Config;

use super::config::load_config;

/// Helper to get document content from the document map
pub(crate) async fn get_document_content(
    document_map: &Arc<Mutex<HashMap<String, String>>>,
    uri: &Uri,
) -> Option<String> {
    let doc_map = document_map.lock().await;
    doc_map.get(&uri.to_string()).cloned()
}

/// Helper to load config with URI-based flavor detection
pub(crate) async fn get_config(
    client: &tower_lsp_server::Client,
    workspace_root: &Arc<Mutex<Option<PathBuf>>>,
    uri: &Uri,
) -> Config {
    let workspace_root = workspace_root.lock().await.clone();
    load_config(client, &workspace_root, Some(uri)).await
}

/// Combined helper: get document and config in one call
pub(crate) async fn get_document_and_config(
    client: &tower_lsp_server::Client,
    document_map: &Arc<Mutex<HashMap<String, String>>>,
    workspace_root: &Arc<Mutex<Option<PathBuf>>>,
    uri: &Uri,
) -> Option<(String, Config)> {
    let content = get_document_content(document_map, uri).await?;
    let config = get_config(client, workspace_root, uri).await;
    Some((content, config))
}
