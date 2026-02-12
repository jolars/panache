use std::path::PathBuf;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::MessageType;

/// Load config from workspace root, falling back to default
pub(crate) async fn load_config(
    client: &Client,
    workspace_root: &Option<PathBuf>,
) -> crate::Config {
    if let Some(root) = workspace_root.as_ref() {
        match crate::config::load(None, root, None) {
            Ok((config, path)) => {
                if let Some(p) = path {
                    client
                        .log_message(
                            MessageType::INFO,
                            format!("Loaded config from {}", p.display()),
                        )
                        .await;
                }
                return config;
            }
            Err(e) => {
                client
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
