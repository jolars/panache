use std::path::PathBuf;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::{MessageType, Uri};

/// Load config from workspace root, falling back to default
///
/// If `document_uri` is provided, the file extension will be used to auto-detect
/// the flavor (.qmd → Quarto, .Rmd → RMarkdown)
pub(crate) async fn load_config(
    client: &Client,
    workspace_root: &Option<PathBuf>,
    document_uri: Option<&Uri>,
) -> crate::Config {
    // Convert URI to file path for flavor detection
    let input_file: Option<PathBuf> = if let Some(uri) = document_uri {
        uri.to_file_path().map(|p| p.into_owned())
    } else {
        None
    };

    if let Some(root) = workspace_root.as_ref() {
        match crate::config::load(None, root, input_file.as_deref()) {
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

    // Even if there's no workspace root, try to detect flavor from file extension
    if let Some(file_path) = &input_file {
        let mut config = crate::Config::default();
        if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
            let detected_flavor = match ext.to_lowercase().as_str() {
                "qmd" => Some(crate::config::Flavor::Quarto),
                "rmd" => Some(crate::config::Flavor::RMarkdown),
                _ => None,
            };
            if let Some(flavor) = detected_flavor {
                config.flavor = flavor;
                config.extensions = crate::config::Extensions::for_flavor(flavor);
            }
        }
        return config;
    }

    crate::Config::default()
}
