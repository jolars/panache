//! External linter integration for code blocks.
//!
//! This module provides support for running external linters (like jarl for R)
//! on code blocks and converting their output to panache diagnostics.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use rowan::TextRange;
use serde::Deserialize;

use crate::linter::diagnostics::{Diagnostic, Edit, Fix, Location};

/// Errors that can occur when invoking external linters.
#[derive(Debug)]
pub enum LinterError {
    /// Linter command not found or failed to spawn
    SpawnFailed(String),
    /// Linter process exited with non-zero status (note: many linters exit 1 when issues found)
    NonZeroExit { code: i32, stderr: String },
    /// Linter timed out
    Timeout,
    /// I/O error during communication with linter
    IoError(std::io::Error),
    /// Failed to parse linter output
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

/// Registry of supported external linters.
pub struct ExternalLinterRegistry {
    linters: HashMap<String, LinterInfo>,
}

/// Information about a supported linter.
pub struct LinterInfo {
    /// Display name
    pub name: &'static str,
    /// Command to execute
    pub command: &'static str,
    /// Arguments (file path will be appended)
    pub args: Vec<&'static str>,
}

impl ExternalLinterRegistry {
    /// Create a new registry with default supported linters.
    pub fn new() -> Self {
        let mut linters = HashMap::new();

        // jarl: R linter
        linters.insert(
            "jarl".to_string(),
            LinterInfo {
                name: "jarl",
                command: "jarl",
                args: vec!["check", "--output-format=json"],
            },
        );

        Self { linters }
    }

    /// Get linter info by name.
    pub fn get(&self, name: &str) -> Option<&LinterInfo> {
        self.linters.get(name)
    }
}

impl Default for ExternalLinterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Run an external linter on code and parse its output.
pub async fn run_linter(
    linter_name: &str,
    code: &str,
    registry: &ExternalLinterRegistry,
) -> Result<Vec<Diagnostic>, LinterError> {
    let linter_info = registry
        .get(linter_name)
        .ok_or_else(|| LinterError::SpawnFailed(format!("unknown linter: {}", linter_name)))?;

    // Create temp file with code
    let mut temp_file = tempfile::NamedTempFile::new()?;
    temp_file.write_all(code.as_bytes())?;
    temp_file.flush()?;

    let temp_path = temp_file.path();

    // Build command
    let mut cmd = Command::new(linter_info.command);
    cmd.args(linter_info.args.iter())
        .arg(temp_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Execute with timeout
    let output = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::task::spawn_blocking(move || cmd.output()).await
    })
    .await
    .map_err(|_| LinterError::Timeout)?
    .map_err(|e| LinterError::IoError(e.into()))??;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Note: Many linters exit with code 1 when they find issues, so we don't treat that as an error
    // Only fail if the command truly failed to run
    if !output.status.success() && stdout.is_empty() {
        return Err(LinterError::NonZeroExit {
            code: output.status.code().unwrap_or(-1),
            stderr: stderr.to_string(),
        });
    }

    // Parse output based on linter type
    match linter_name {
        "jarl" => parse_jarl_output(&stdout, code),
        _ => Err(LinterError::ParseError(format!(
            "no parser for linter: {}",
            linter_name
        ))),
    }
}

/// jarl JSON output structures
#[derive(Debug, Deserialize)]
struct JarlOutput {
    diagnostics: Vec<JarlDiagnostic>,
    #[allow(dead_code)]
    errors: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JarlDiagnostic {
    message: JarlMessage,
    #[allow(dead_code)]
    filename: String,
    range: [usize; 2],
    location: JarlLocation,
    fix: JarlFix,
}

#[derive(Debug, Deserialize)]
struct JarlMessage {
    name: String,
    body: String,
    #[allow(dead_code)]
    suggestion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JarlLocation {
    row: usize,
    column: usize,
}

#[derive(Debug, Deserialize)]
struct JarlFix {
    content: String,
    start: usize,
    end: usize,
    to_skip: bool,
}

/// Parse jarl JSON output into panache diagnostics.
fn parse_jarl_output(json: &str, _input: &str) -> Result<Vec<Diagnostic>, LinterError> {
    let output: JarlOutput = serde_json::from_str(json)
        .map_err(|e| LinterError::ParseError(format!("invalid jarl JSON: {}", e)))?;

    let mut diagnostics = Vec::new();

    for jarl_diag in output.diagnostics {
        // Convert location (jarl uses 1-indexed rows, 0-indexed columns)
        let line = jarl_diag.location.row; // Already 1-indexed
        let column = jarl_diag.location.column + 1; // Convert to 1-indexed

        // Convert byte range to TextRange
        let range = TextRange::new(
            (jarl_diag.range[0] as u32).into(),
            (jarl_diag.range[1] as u32).into(),
        );

        let location = Location {
            line,
            column,
            range,
        };

        // Convert fix if available
        let fix = if !jarl_diag.fix.to_skip {
            let fix_range = TextRange::new(
                (jarl_diag.fix.start as u32).into(),
                (jarl_diag.fix.end as u32).into(),
            );

            Some(Fix {
                message: format!("Apply jarl fix for {}", jarl_diag.message.name),
                edits: vec![Edit {
                    range: fix_range,
                    replacement: jarl_diag.fix.content,
                }],
            })
        } else {
            None
        };

        // jarl reports warnings, not errors
        let diagnostic =
            Diagnostic::warning(location, jarl_diag.message.name, jarl_diag.message.body);

        let diagnostic = if let Some(fix) = fix {
            diagnostic.with_fix(fix)
        } else {
            diagnostic
        };

        diagnostics.push(diagnostic);
    }

    Ok(diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_contains_jarl() {
        let registry = ExternalLinterRegistry::new();
        assert!(registry.get("jarl").is_some());
        assert_eq!(registry.get("jarl").unwrap().name, "jarl");
    }

    #[test]
    fn test_registry_unknown_linter() {
        let registry = ExternalLinterRegistry::new();
        assert!(registry.get("unknown").is_none());
    }

    #[test]
    fn test_parse_jarl_output() {
        let json = r#"{
            "diagnostics": [
                {
                    "message": {
                        "name": "assignment",
                        "body": "Use `<-` for assignment.",
                        "suggestion": null
                    },
                    "filename": "/tmp/test.R",
                    "range": [0, 3],
                    "location": {
                        "row": 1,
                        "column": 0
                    },
                    "fix": {
                        "content": "x <- 1",
                        "start": 0,
                        "end": 5,
                        "to_skip": false
                    }
                }
            ],
            "errors": []
        }"#;

        let input = "x = 1\n";
        let diagnostics = parse_jarl_output(json, input).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "assignment");
        assert_eq!(diagnostics[0].message, "Use `<-` for assignment.");
        assert_eq!(diagnostics[0].location.line, 1);
        assert_eq!(diagnostics[0].location.column, 1);
        assert!(diagnostics[0].fix.is_some());

        let fix = diagnostics[0].fix.as_ref().unwrap();
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, "x <- 1");
    }

    #[test]
    fn test_parse_jarl_output_no_fix() {
        let json = r#"{
            "diagnostics": [
                {
                    "message": {
                        "name": "test_rule",
                        "body": "Test message",
                        "suggestion": null
                    },
                    "filename": "/tmp/test.R",
                    "range": [0, 5],
                    "location": {
                        "row": 1,
                        "column": 0
                    },
                    "fix": {
                        "content": "",
                        "start": 0,
                        "end": 0,
                        "to_skip": true
                    }
                }
            ],
            "errors": []
        }"#;

        let input = "test\n";
        let diagnostics = parse_jarl_output(json, input).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].fix.is_none());
    }
}
