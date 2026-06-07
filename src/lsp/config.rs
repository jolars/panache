use std::path::{Path, PathBuf};

use lsp_types::Uri;

use crate::config::ConfigSource;
use crate::lsp::uri_ext::UriExt;

/// Load config from workspace root, falling back to default
///
/// If `document_uri` is provided, the file extension will be used to auto-detect
/// the flavor (.qmd → Quarto, .Rmd/.Rmarkdown → RMarkdown)
pub(crate) fn load_config(
    workspace_root: &Option<PathBuf>,
    document_uri: Option<&Uri>,
) -> crate::Config {
    load_config_with_source(workspace_root, document_uri).0
}

/// Like [`load_config`] but also returns the [`ConfigSource`] so callers can
/// resolve the project anchor used by `exclude`/`include` patterns.
pub(crate) fn load_config_with_source(
    workspace_root: &Option<PathBuf>,
    document_uri: Option<&Uri>,
) -> (crate::Config, ConfigSource) {
    // Convert URI to file path for flavor detection
    let input_file: Option<PathBuf> = if let Some(uri) = document_uri {
        uri.to_file_path().map(|p| p.into_owned())
    } else {
        None
    };

    if let Some(root) = workspace_root.as_ref() {
        // Start the config walk at the file's directory (so a `panache.toml`
        // closer to the file shadows one at the workspace root). Project-root
        // discovery via `.git` happens inside `config::load`, so CLI and LSP
        // pick the same project boundary symmetrically.
        let start_dir = input_file
            .as_deref()
            .and_then(|p| p.parent())
            .filter(|p| p.starts_with(root))
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        match crate::config::load(None, &start_dir, input_file.as_deref(), None) {
            Ok((config, source)) => {
                if let Some(p) = source.path() {
                    log::info!("Loaded config from {}", p.display());
                }
                return (config, source);
            }
            Err(e) => {
                log::warn!("Failed to load config: {e}");
            }
        }
    }

    // Even if there's no workspace root, try to detect flavor from file extension
    if let Some(file_path) = &input_file {
        let mut config = crate::Config::default();
        if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
            let detected_flavor = match ext.to_lowercase().as_str() {
                "qmd" => Some(crate::config::Flavor::Quarto),
                "rmd" | "rmarkdown" => Some(crate::config::Flavor::RMarkdown),
                _ => None,
            };
            if let Some(flavor) = detected_flavor {
                config.flavor = flavor;
                config.extensions = crate::config::Extensions::for_flavor(flavor);
            }
        }
        return (config, ConfigSource::None);
    }

    (crate::Config::default(), ConfigSource::None)
}
