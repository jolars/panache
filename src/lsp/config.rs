use std::path::{Path, PathBuf};

use lsp_types::Uri;

use crate::config::{ConfigError, ConfigSource};
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
///
/// A broken config file is swallowed here (logged, then defaulted) so the
/// document still parses and lints. Callers that need to *surface* the parse
/// error — refuse to format, publish a diagnostic, toast — use
/// [`try_load_config`] instead.
pub(crate) fn load_config_with_source(
    workspace_root: &Option<PathBuf>,
    document_uri: Option<&Uri>,
) -> (crate::Config, ConfigSource) {
    match try_load_config(workspace_root, document_uri) {
        Ok(loaded) => loaded,
        Err(e) => {
            log::warn!("Failed to load config: {e}");
            (default_config_for_uri(document_uri), ConfigSource::None)
        }
    }
}

/// Load config, surfacing a structured [`ConfigError`] when a discovered or
/// explicit config file is present but fails to parse.
///
/// Returns `Ok` for the success path *and* the no-config-found default path
/// (the latter still carries a flavor detected from the file extension). Only a
/// genuine parse/validation failure of a config file yields `Err`. A non-parse
/// I/O error (e.g. unreadable file) is logged and treated as "no config".
pub(crate) fn try_load_config(
    workspace_root: &Option<PathBuf>,
    document_uri: Option<&Uri>,
) -> Result<(crate::Config, ConfigSource), ConfigError> {
    // Convert URI to file path for flavor detection
    let input_file: Option<PathBuf> =
        document_uri.and_then(|uri| uri.to_file_path().map(|p| p.into_owned()));

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
                return Ok((config, source));
            }
            Err(e) => {
                // A config file was found but failed to parse: recover the
                // structured error so the caller can anchor a diagnostic. A
                // non-parse I/O error has no `ConfigError` source; fall through
                // to the flavor-detected default.
                if let Some(cfg_err) = e
                    .get_ref()
                    .and_then(|src| src.downcast_ref::<ConfigError>())
                {
                    return Err(cfg_err.clone());
                }
                log::warn!("Failed to load config: {e}");
            }
        }
    }

    Ok((default_config_for_uri(document_uri), ConfigSource::None))
}

/// The default config to use when no config file applies, with the flavor
/// inferred from the document's file extension (`.qmd` → Quarto,
/// `.Rmd`/`.Rmarkdown` → RMarkdown, `.svx`/`.svelte.md` → Mdsvex). Detection is
/// delegated to [`crate::config::detect_flavor_from_path`] so the recognized
/// extension set stays in lockstep with the config-file path; a reduced
/// hand-rolled match here previously dropped mdsvex on the floor.
pub(crate) fn default_config_for_uri(document_uri: Option<&Uri>) -> crate::Config {
    let mut config = crate::Config::default();
    let Some(file_path) = document_uri.and_then(|uri| uri.to_file_path()) else {
        return config;
    };
    if let Some(flavor) = crate::config::detect_flavor_from_path(&file_path, &config) {
        config.flavor = flavor;
        config.extensions = crate::config::Extensions::for_flavor(flavor);
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Flavor;

    /// Build an absolute path for `file_name` that is valid on the host OS, so
    /// `Uri::from_file_path` succeeds on Windows (which rejects Unix-style
    /// `/tmp/...` paths) as well as Unix.
    fn config_for(file_name: &str) -> crate::Config {
        let mut path = std::env::temp_dir();
        path.push(file_name);
        let uri = Uri::from_file_path(&path).expect("uri");
        default_config_for_uri(Some(&uri))
    }

    #[test]
    fn default_config_detects_quarto() {
        assert_eq!(config_for("doc.qmd").flavor, Flavor::Quarto);
    }

    #[test]
    fn default_config_detects_rmarkdown() {
        assert_eq!(config_for("doc.Rmd").flavor, Flavor::RMarkdown);
    }

    #[test]
    fn default_config_detects_mdsvex_svx() {
        let config = config_for("doc.svx");
        assert_eq!(config.flavor, Flavor::Mdsvex);
        // The mdsvex flavor must carry its `svelte-template` extension so the
        // no-config LSP path actually parses Svelte spans.
        assert!(config.extensions.svelte_template);
    }

    #[test]
    fn default_config_detects_mdsvex_compound_svelte_md() {
        let config = config_for("page.svelte.md");
        assert_eq!(config.flavor, Flavor::Mdsvex);
        assert!(config.extensions.svelte_template);
    }

    #[test]
    fn default_config_leaves_plain_markdown_as_default() {
        assert_eq!(config_for("doc.md").flavor, Flavor::default());
    }
}
