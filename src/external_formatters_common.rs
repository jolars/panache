//! Common types and utilities for external formatter integration.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use crate::config::FormatterConfig;
use crate::external_tools_common::{find_missing_commands, missing_commands_warning_message};

static MISSING_FORMATTER_MESSAGES_LOGGED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
pub enum FormatterIoMode {
    Stdin,
    File,
}

impl FormatterIoMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stdin => "stdin",
            Self::File => "file",
        }
    }
}

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
    find_missing_commands(
        formatters
            .values()
            .flat_map(|configs| configs.iter().map(|cfg| cfg.cmd.as_str())),
    )
}

/// WASM has no external formatter execution.
#[cfg(target_arch = "wasm32")]
pub fn find_missing_formatter_commands(
    _formatters: &HashMap<String, Vec<FormatterConfig>>,
) -> HashSet<String> {
    HashSet::new()
}

/// Log one consolidated info message for missing external formatter commands.
///
/// Missing commands are a common optional configuration scenario and should not
/// emit warnings by default. We log once at info level so release builds can
/// still surface actionable diagnostics without warning noise.
pub fn log_missing_formatter_commands(missing: &HashSet<String>) {
    let Some(message) = missing_formatter_warning_message(missing) else {
        return;
    };

    let logged_messages =
        MISSING_FORMATTER_MESSAGES_LOGGED.get_or_init(|| Mutex::new(HashSet::new()));
    let mut logged = logged_messages
        .lock()
        .expect("missing formatter message mutex poisoned");
    if !logged.insert(message.clone()) {
        return;
    }

    log::info!("{}", message);
}

/// Resolve stdin argument placeholders against a language-aware virtual filename.
///
/// Some tools (for example Prettier) need a file path hint while still reading
/// from stdin. We support `{}` as a placeholder in stdin args for this purpose.
pub fn resolve_stdin_args(args: &[String], language: &str) -> Vec<String> {
    let virtual_filename = virtual_filename_for_language(language);
    args.iter()
        .map(|arg| arg.replace("{}", &virtual_filename))
        .collect()
}

pub fn log_formatter_invocation(
    command: &str,
    language: &str,
    mode: FormatterIoMode,
    args: &[String],
) {
    log::debug!(
        "External formatter start: cmd='{}', language='{}', mode='{}', args={}",
        command,
        language,
        mode.as_str(),
        args.len()
    );
    log::trace!("External formatter args ({}): {:?}", command, args);
}

pub fn log_formatter_spawn_failed(
    command: &str,
    language: &str,
    mode: FormatterIoMode,
    error: &std::io::Error,
) {
    if error.kind() == std::io::ErrorKind::NotFound {
        log::debug!(
            "External formatter unavailable: cmd='{}', language='{}', mode='{}', error={}",
            command,
            language,
            mode.as_str(),
            error
        );
    } else {
        log::warn!(
            "External formatter spawn failed: cmd='{}', language='{}', mode='{}', error={}",
            command,
            language,
            mode.as_str(),
            error
        );
    }
}

pub fn log_formatter_nonzero_exit(
    command: &str,
    language: &str,
    mode: FormatterIoMode,
    exit_code: i32,
    stderr: &str,
) {
    let summary = stderr.lines().next().unwrap_or("").trim();
    log::warn!(
        "External formatter failed: cmd='{}', language='{}', mode='{}', exit_code={}, stderr='{}'",
        command,
        language,
        mode.as_str(),
        exit_code,
        summary
    );
    log::trace!("External formatter stderr ({}): {}", command, stderr);
}

pub fn log_formatter_timeout(command: &str, language: &str, mode: FormatterIoMode) {
    log::warn!(
        "External formatter timed out: cmd='{}', language='{}', mode='{}'",
        command,
        language,
        mode.as_str()
    );
}

pub fn log_formatter_success(
    command: &str,
    language: &str,
    mode: FormatterIoMode,
    output_len: usize,
    elapsed: std::time::Duration,
) {
    log::debug!(
        "External formatter succeeded: cmd='{}', language='{}', mode='{}', output_bytes={}, elapsed_ms={}",
        command,
        language,
        mode.as_str(),
        output_len,
        elapsed.as_millis()
    );
}

pub(crate) fn temp_file_extension_for_language(language: &str) -> &'static str {
    match normalized_language(language).as_str() {
        "javascript" | "js" => "js",
        "typescript" | "ts" => "ts",
        "jsx" => "jsx",
        "tsx" => "tsx",
        "json" => "json",
        "jsonc" => "jsonc",
        "yaml" | "yml" => "yaml",
        "markdown" | "md" | "qmd" | "rmd" => "md",
        "css" => "css",
        "scss" => "scss",
        "less" => "less",
        "html" => "html",
        "vue" => "vue",
        "svelte" => "svelte",
        "graphql" | "gql" => "graphql",
        "r" => "r",
        _ => "txt",
    }
}

fn virtual_filename_for_language(language: &str) -> String {
    format!("stdin.{}", temp_file_extension_for_language(language))
}

fn normalized_language(language: &str) -> String {
    language.trim().to_ascii_lowercase().replace('_', "-")
}

fn missing_formatter_warning_message(missing: &HashSet<String>) -> Option<String> {
    missing_commands_warning_message(missing, "formatter", "formatting")
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{
        find_missing_formatter_commands, missing_formatter_warning_message, resolve_stdin_args,
        temp_file_extension_for_language,
    };
    use crate::config::FormatterConfig;
    use std::collections::{HashMap, HashSet};

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

    #[test]
    fn warning_message_sorts_and_deduplicates_commands() {
        let missing = HashSet::from([
            "black".to_string(),
            "rustfmt".to_string(),
            "black".to_string(),
        ]);

        let message = missing_formatter_warning_message(&missing).expect("message expected");
        assert_eq!(
            message,
            "External formatter command(s) not found: black, rustfmt. Configured external formatting for these tools will be skipped."
        );
    }

    #[test]
    fn resolve_stdin_args_replaces_placeholder_with_language_filename() {
        let args = vec!["--stdin-filepath".to_string(), "{}".to_string()];
        let resolved = resolve_stdin_args(&args, "typescript");
        assert_eq!(
            resolved,
            vec!["--stdin-filepath".to_string(), "stdin.ts".to_string()]
        );
    }

    #[test]
    fn resolve_stdin_args_falls_back_for_unknown_language() {
        let args = vec!["--stdin-filepath".to_string(), "{}".to_string()];
        let resolved = resolve_stdin_args(&args, "unknownlang");
        assert_eq!(
            resolved,
            vec!["--stdin-filepath".to_string(), "stdin.txt".to_string()]
        );
    }

    #[test]
    fn temp_file_extension_is_language_aware() {
        assert_eq!(temp_file_extension_for_language("r"), "r");
        assert_eq!(temp_file_extension_for_language("TypeScript"), "ts");
        assert_eq!(temp_file_extension_for_language("unknownlang"), "txt");
    }
}
