//! External linter integration for code blocks.

use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(feature = "lsp")]
use std::process::{Command, Stdio};
#[cfg(feature = "lsp")]
use std::time::Duration;

use crate::linter::code_block_collector::BlockMapping;
use crate::linter::diagnostics::Diagnostic;
use crate::linter::offsets::line_col_to_byte_offset_1based;

mod clippy;
mod eslint;
mod jarl;
mod ruff;
mod shellcheck;
mod staticcheck;

pub(crate) trait ExternalLinterParser {
    const NAME: &'static str;
    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError>;
}

/// Errors that can occur when invoking external linters.
#[derive(Debug)]
pub enum LinterError {
    SpawnFailed(String),
    NonZeroExit { code: i32, stderr: String },
    Timeout,
    IoError(std::io::Error),
    ParseError(String),
}

impl std::fmt::Display for LinterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed(cmd) => write!(f, "failed to spawn linter: {}", cmd),
            Self::NonZeroExit { code, stderr } => {
                write!(f, "linter exited with code {}: {}", code, stderr)
            }
            Self::Timeout => write!(f, "linter timed out"),
            Self::IoError(e) => write!(f, "linter I/O error: {}", e),
            Self::ParseError(msg) => write!(f, "failed to parse linter output: {}", msg),
        }
    }
}

impl std::error::Error for LinterError {}

impl From<std::io::Error> for LinterError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

/// Shared parse context for linter-specific parsers.
pub(crate) struct ParseContext<'a> {
    pub output: &'a str,
    pub linted_input: &'a str,
    pub original_input: &'a str,
    pub mappings: Option<&'a [BlockMapping]>,
}

/// Information about a supported linter.
pub struct LinterInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub url: &'static str,
    pub command: &'static str,
    pub args: Vec<&'static str>,
    pub supported_languages: Vec<&'static str>,
}

fn shellcheck_shell_for_language(language: &str) -> &'static str {
    match language.to_ascii_lowercase().as_str() {
        "bash" => "bash",
        "ksh" => "ksh",
        // ShellCheck doesn't support zsh as a dialect flag; use sh as the closest baseline.
        "zsh" | "sh" | "shell" => "sh",
        _ => "sh",
    }
}

pub(crate) fn append_language_specific_args(
    cmd: &mut std::process::Command,
    linter_name: &str,
    language: &str,
) {
    if linter_name.eq_ignore_ascii_case("shellcheck") {
        let shell = shellcheck_shell_for_language(language);
        cmd.arg("--shell").arg(shell);
    }
}

pub(crate) fn file_suffix_for_language(language: &str) -> Option<&'static str> {
    match language.to_ascii_lowercase().as_str() {
        "js" | "javascript" => Some(".js"),
        "jsx" => Some(".jsx"),
        "mjs" => Some(".mjs"),
        "cjs" => Some(".cjs"),
        "ts" | "typescript" => Some(".ts"),
        "tsx" => Some(".tsx"),
        "python" => Some(".py"),
        "go" | "golang" => Some(".go"),
        "rust" | "rs" => Some(".rs"),
        "r" => Some(".R"),
        "sh" | "bash" | "zsh" | "ksh" | "shell" => Some(".sh"),
        _ => None,
    }
}

pub(crate) fn create_linter_temp_input(
    language: &str,
    code: &str,
) -> Result<(tempfile::TempDir, PathBuf), LinterError> {
    let mut dir_builder = tempfile::Builder::new();
    dir_builder.prefix("panache-external-");
    let temp_dir = dir_builder.tempdir()?;

    let suffix = file_suffix_for_language(language).unwrap_or("");
    let temp_path = temp_dir.path().join(format!("input{}", suffix));
    std::fs::write(&temp_path, code.as_bytes())?;

    Ok((temp_dir, temp_path))
}

/// Registry of supported external linters.
pub struct ExternalLinterRegistry {
    linters: HashMap<String, LinterInfo>,
}

impl ExternalLinterRegistry {
    pub fn new() -> Self {
        let mut linters = HashMap::new();
        linters.insert(
            "jarl".to_string(),
            LinterInfo {
                name: "jarl",
                description: "Jarl is a fast linter for R: it does static code analysis to search for programming errors, bugs, and suspicious patterns of code.",
                url: "https://github.com/etiennebacher/jarl",
                command: "jarl",
                args: vec!["check", "--output-format=json"],
                supported_languages: vec!["r"],
            },
        );
        linters.insert(
            "ruff".to_string(),
            LinterInfo {
                name: "ruff",
                description: "An extremely fast Python linter and code formatter, written in Rust. ",
                url: "https://docs.astral.sh/ruff/",
                command: "ruff",
                args: vec!["check", "--output-format", "json"],
                supported_languages: vec!["python"],
            },
        );
        linters.insert(
            "eslint".to_string(),
            LinterInfo {
                name: "eslint",
                description: "JavaScript and TypeScript linter.",
                url: "https://eslint.org/",
                command: "eslint",
                args: vec![
                    "--no-config-lookup",
                    "--rule",
                    "no-unused-vars:error",
                    "--format",
                    "json",
                ],
                supported_languages: vec![
                    "js",
                    "javascript",
                    "jsx",
                    "mjs",
                    "cjs",
                    "ts",
                    "typescript",
                    "tsx",
                ],
            },
        );
        linters.insert(
            "shellcheck".to_string(),
            LinterInfo {
                name: "shellcheck",
                description: "Static analysis for shell scripts.",
                url: "https://www.shellcheck.net/",
                command: "shellcheck",
                args: vec!["-f", "json"],
                supported_languages: vec!["sh", "bash", "zsh", "ksh", "shell"],
            },
        );
        linters.insert(
            "staticcheck".to_string(),
            LinterInfo {
                name: "staticcheck",
                description: "Advanced static analysis for Go code.",
                url: "https://staticcheck.dev/",
                command: "staticcheck",
                args: vec!["-f", "json"],
                supported_languages: vec!["go", "golang"],
            },
        );
        linters.insert(
            "clippy".to_string(),
            LinterInfo {
                name: "clippy",
                description: "Rust lints to catch mistakes and improve style.",
                url: "https://doc.rust-lang.org/clippy/",
                command: "clippy-driver",
                args: vec!["--error-format=json", "-W", "clippy::all"],
                supported_languages: vec!["rust", "rs"],
            },
        );
        Self { linters }
    }

    pub fn get(&self, name: &str) -> Option<&LinterInfo> {
        self.linters.get(name)
    }

    pub fn supports_language(&self, linter_name: &str, language: &str) -> Option<bool> {
        self.get(linter_name).map(|info| {
            info.supported_languages
                .iter()
                .any(|supported| supported.eq_ignore_ascii_case(language))
        })
    }
}

impl Default for ExternalLinterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "lsp")]
pub async fn run_linter(
    linter_name: &str,
    language: &str,
    code: &str,
    original_input: &str,
    registry: &ExternalLinterRegistry,
    mappings: Option<&[BlockMapping]>,
) -> Result<Vec<Diagnostic>, LinterError> {
    let linter_info = registry
        .get(linter_name)
        .ok_or_else(|| LinterError::SpawnFailed(format!("unknown linter: {}", linter_name)))?;
    if !registry
        .supports_language(linter_name, language)
        .unwrap_or(false)
    {
        return Err(LinterError::SpawnFailed(format!(
            "unsupported linter-language mapping: {} for {}",
            linter_name, language
        )));
    }

    let (_temp_dir, temp_path) = create_linter_temp_input(language, code)?;

    let mut cmd = Command::new(linter_info.command);
    cmd.args(linter_info.args.iter());
    append_language_specific_args(&mut cmd, linter_name, language);
    if (linter_name.eq_ignore_ascii_case("eslint") || linter_name.eq_ignore_ascii_case("clippy"))
        && let Some(parent) = temp_path.parent()
    {
        cmd.current_dir(parent);
    }
    cmd.arg(&temp_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::task::spawn_blocking(move || cmd.output()).await
    })
    .await
    .map_err(|_| LinterError::Timeout)?
    .map_err(|e| LinterError::IoError(e.into()))??;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() && stdout.trim().is_empty() && stderr.trim().is_empty() {
        return Err(LinterError::NonZeroExit {
            code: output.status.code().unwrap_or(-1),
            stderr: stderr.to_string(),
        });
    }

    let linter_output = if stdout.trim().is_empty() {
        stderr.as_ref()
    } else {
        stdout.as_ref()
    };

    parse_linter_output(linter_name, linter_output, code, original_input, mappings)
}

pub fn parse_linter_output(
    linter_name: &str,
    output: &str,
    linted_input: &str,
    original_input: &str,
    mappings: Option<&[BlockMapping]>,
) -> Result<Vec<Diagnostic>, LinterError> {
    let ctx = ParseContext {
        output,
        linted_input,
        original_input,
        mappings,
    };
    if linter_name == jarl::JarlParser::NAME {
        return jarl::JarlParser::parse(&ctx);
    }
    if linter_name == ruff::RuffParser::NAME {
        return ruff::RuffParser::parse(&ctx);
    }
    if linter_name == eslint::EslintParser::NAME {
        return eslint::EslintParser::parse(&ctx);
    }
    if linter_name == staticcheck::StaticcheckParser::NAME {
        return staticcheck::StaticcheckParser::parse(&ctx);
    }
    if linter_name == clippy::ClippyParser::NAME {
        return clippy::ClippyParser::parse(&ctx);
    }
    if linter_name == shellcheck::ShellcheckParser::NAME {
        return shellcheck::ShellcheckParser::parse(&ctx);
    }

    Err(LinterError::ParseError(format!(
        "no parser for linter: {}",
        linter_name
    )))
}

pub(crate) fn line_col_to_offset(input: &str, line: usize, column: usize) -> Option<usize> {
    line_col_to_byte_offset_1based(input, line, column)
}

pub(crate) fn map_concatenated_offset_to_original(
    offset: usize,
    mappings: &[BlockMapping],
) -> Option<usize> {
    for mapping in mappings {
        if mapping.concatenated_range.contains(&offset) {
            let relative_offset = offset - mapping.concatenated_range.start;
            let original_offset = mapping.original_range.start + relative_offset;
            if original_offset <= mapping.original_range.end {
                return Some(original_offset);
            }
        }
    }
    None
}

pub(crate) fn map_concatenated_offset_to_original_with_end_boundary(
    offset: usize,
    mappings: &[BlockMapping],
) -> Option<usize> {
    map_concatenated_offset_to_original(offset, mappings).or_else(|| {
        mappings.iter().find_map(|mapping| {
            if mapping.concatenated_range.end == offset {
                Some(mapping.original_range.end)
            } else {
                None
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_contains_linters() {
        let registry = ExternalLinterRegistry::new();
        assert!(registry.get("jarl").is_some());
        assert!(registry.get("ruff").is_some());
        assert!(registry.get("eslint").is_some());
        assert!(registry.get("staticcheck").is_some());
        assert!(registry.get("clippy").is_some());
        assert!(registry.get("shellcheck").is_some());
    }

    #[test]
    fn test_registry_linter_language_support() {
        let registry = ExternalLinterRegistry::new();
        assert_eq!(registry.supports_language("jarl", "r"), Some(true));
        assert_eq!(registry.supports_language("jarl", "bash"), Some(false));
        assert_eq!(registry.supports_language("ruff", "python"), Some(true));
        assert_eq!(registry.supports_language("eslint", "js"), Some(true));
        assert_eq!(
            registry.supports_language("eslint", "typescript"),
            Some(true)
        );
        assert_eq!(registry.supports_language("eslint", "python"), Some(false));
        assert_eq!(registry.supports_language("staticcheck", "go"), Some(true));
        assert_eq!(
            registry.supports_language("staticcheck", "golang"),
            Some(true)
        );
        assert_eq!(
            registry.supports_language("staticcheck", "python"),
            Some(false)
        );
        assert_eq!(registry.supports_language("clippy", "rust"), Some(true));
        assert_eq!(registry.supports_language("clippy", "rs"), Some(true));
        assert_eq!(registry.supports_language("clippy", "go"), Some(false));
        assert_eq!(registry.supports_language("shellcheck", "bash"), Some(true));
        assert_eq!(registry.supports_language("shellcheck", "sh"), Some(true));
        assert_eq!(
            registry.supports_language("shellcheck", "python"),
            Some(false)
        );
        assert_eq!(registry.supports_language("unknown", "r"), None);
    }

    #[test]
    fn test_create_linter_temp_input_cleanup_removes_sibling_artifacts() {
        let temp_dir_path;
        {
            let (temp_dir, temp_path) =
                create_linter_temp_input("rust", "fn main() { let _x = 1; }\n").unwrap();
            temp_dir_path = temp_dir.path().to_path_buf();

            assert!(temp_path.exists());

            let sibling_artifact = temp_dir.path().join("input");
            std::fs::write(&sibling_artifact, b"compiled artifact").unwrap();
            assert!(sibling_artifact.exists());
        }

        assert!(!temp_dir_path.exists());
    }

    #[test]
    fn test_shellcheck_language_maps_to_explicit_shell() {
        assert_eq!(shellcheck_shell_for_language("sh"), "sh");
        assert_eq!(shellcheck_shell_for_language("shell"), "sh");
        assert_eq!(shellcheck_shell_for_language("bash"), "bash");
        assert_eq!(shellcheck_shell_for_language("ksh"), "ksh");
        assert_eq!(shellcheck_shell_for_language("zsh"), "sh");
    }

    #[test]
    fn test_append_language_specific_args_adds_shellcheck_shell_flag() {
        let mut cmd = std::process::Command::new("shellcheck");
        append_language_specific_args(&mut cmd, "shellcheck", "bash");
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["--shell".to_string(), "bash".to_string()]);
    }
}
