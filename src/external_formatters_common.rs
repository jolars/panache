//! Common types and utilities for external formatter integration.

use std::collections::{HashMap, HashSet};

use crate::config::FormatterConfig;

/// Errors that can occur when invoking external formatters.
#[derive(Debug)]
pub enum FormatterError {
    /// Formatter command not found or failed to spawn
    SpawnFailed(String),
    /// Formatter process exited with non-zero status
    NonZeroExit { code: i32, stderr: String },
    /// Formatter timed out
    Timeout,
    /// I/O error during communication with formatter
    IoError(std::io::Error),
}

impl std::fmt::Display for FormatterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed(cmd) => write!(f, "failed to spawn formatter: {}", cmd),
            Self::NonZeroExit { code, stderr } => {
                write!(f, "formatter exited with code {}: {}", code, stderr)
            }
            Self::Timeout => write!(f, "formatter timed out"),
            Self::IoError(e) => write!(f, "formatter I/O error: {}", e),
        }
    }
}

impl std::error::Error for FormatterError {}

impl From<std::io::Error> for FormatterError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

/// Find external formatter commands that are configured but unavailable.
#[cfg(not(target_arch = "wasm32"))]
pub fn find_missing_formatter_commands(
    formatters: &HashMap<String, Vec<FormatterConfig>>,
) -> HashSet<String> {
    formatters
        .values()
        .flat_map(|configs| configs.iter())
        .filter_map(|cfg| {
            let cmd = cfg.cmd.trim();
            if cmd.is_empty() || command_exists(cmd) {
                None
            } else {
                Some(cmd.to_string())
            }
        })
        .collect()
}

/// WASM has no external formatter execution.
#[cfg(target_arch = "wasm32")]
pub fn find_missing_formatter_commands(
    _formatters: &HashMap<String, Vec<FormatterConfig>>,
) -> HashSet<String> {
    HashSet::new()
}

/// Log one consolidated warning for missing external formatter commands.
pub fn log_missing_formatter_commands(missing: &HashSet<String>) {
    if missing.is_empty() {
        return;
    }

    let mut sorted_missing: Vec<_> = missing.iter().map(String::as_str).collect();
    sorted_missing.sort_unstable();

    log::warn!(
        "External formatter command(s) not found: {}. Configured external formatting for these tools will be skipped.",
        sorted_missing.join(", ")
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn command_exists(cmd: &str) -> bool {
    use std::path::Path;

    if has_path_separator(cmd) {
        return Path::new(cmd).exists();
    }
    which::which(cmd).is_ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn has_path_separator(cmd: &str) -> bool {
    cmd.contains(std::path::MAIN_SEPARATOR)
        || cfg!(windows) && (cmd.contains('/') || cmd.contains('\\'))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::find_missing_formatter_commands;
    use crate::config::FormatterConfig;
    use std::collections::HashMap;

    #[test]
    fn reports_missing_commands_once() {
        let mut formatters = HashMap::new();
        formatters.insert(
            "python".to_string(),
            vec![
                FormatterConfig {
                    cmd: "definitely-not-a-real-formatter-123".to_string(),
                    args: vec![],
                    enabled: true,
                    stdin: true,
                },
                FormatterConfig {
                    cmd: "definitely-not-a-real-formatter-123".to_string(),
                    args: vec![],
                    enabled: true,
                    stdin: true,
                },
            ],
        );

        let missing = find_missing_formatter_commands(&formatters);
        assert_eq!(missing.len(), 1);
        assert!(missing.contains("definitely-not-a-real-formatter-123"));
    }

    #[test]
    fn skips_empty_commands() {
        let mut formatters = HashMap::new();
        formatters.insert(
            "python".to_string(),
            vec![FormatterConfig {
                cmd: "   ".to_string(),
                args: vec![],
                enabled: true,
                stdin: true,
            }],
        );

        let missing = find_missing_formatter_commands(&formatters);
        assert!(missing.is_empty());
    }
}
