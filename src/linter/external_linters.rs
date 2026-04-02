//! External linter integration for code blocks.

use std::collections::HashMap;

#[cfg(feature = "lsp")]
use std::io::Write;
#[cfg(feature = "lsp")]
use std::process::{Command, Stdio};
#[cfg(feature = "lsp")]
use std::time::Duration;

use crate::linter::code_block_collector::BlockMapping;
use crate::linter::diagnostics::Diagnostic;
use crate::linter::offsets::line_col_to_byte_offset_1based;

mod jarl;
mod ruff;
mod shellcheck;

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
    pub command: &'static str,
    pub args: Vec<&'static str>,
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
                command: "jarl",
                args: vec!["check", "--output-format=json"],
            },
        );
        linters.insert(
            "ruff".to_string(),
            LinterInfo {
                name: "ruff",
                command: "ruff",
                args: vec!["check", "--output-format", "json"],
            },
        );
        linters.insert(
            "shellcheck".to_string(),
            LinterInfo {
                name: "shellcheck",
                command: "shellcheck",
                args: vec!["-f", "json"],
            },
        );
        Self { linters }
    }

    pub fn get(&self, name: &str) -> Option<&LinterInfo> {
        self.linters.get(name)
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
    code: &str,
    original_input: &str,
    registry: &ExternalLinterRegistry,
    mappings: Option<&[BlockMapping]>,
) -> Result<Vec<Diagnostic>, LinterError> {
    let linter_info = registry
        .get(linter_name)
        .ok_or_else(|| LinterError::SpawnFailed(format!("unknown linter: {}", linter_name)))?;

    let mut temp_file = tempfile::NamedTempFile::new()?;
    temp_file.write_all(code.as_bytes())?;
    temp_file.flush()?;
    let temp_path = temp_file.path();

    let mut cmd = Command::new(linter_info.command);
    cmd.args(linter_info.args.iter())
        .arg(temp_path)
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

    if !output.status.success() && stdout.is_empty() {
        return Err(LinterError::NonZeroExit {
            code: output.status.code().unwrap_or(-1),
            stderr: stderr.to_string(),
        });
    }

    parse_linter_output(linter_name, &stdout, code, original_input, mappings)
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
        assert!(registry.get("shellcheck").is_some());
    }
}
