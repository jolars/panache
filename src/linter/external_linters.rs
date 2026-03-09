//! External linter integration for code blocks.
//!
//! This module provides support for running external linters (like jarl for R)
//! on code blocks and converting their output to panache diagnostics.
//!
//! ## Auto-fix Support
//!
//! External linters can provide auto-fixes, but there's a complexity: the linter
//! runs on a concatenated temporary file (all code blocks with blank line padding),
//! while fixes need to be applied to the original document.
//!
//! The mapping works as follows:
//! 1. Code blocks are concatenated with `concatenate_with_blanks_and_mapping()`
//! 2. This preserves line numbers but creates different byte offsets
//! 3. Mapping information tracks both concatenated and original byte ranges
//! 4. When parsing linter fixes, `map_concatenated_offset_to_original()` converts
//!    byte offsets from the temp file back to the original document
//!
//! This allows linter fixes to be seamlessly applied to the correct locations
//! in the source markdown file.

use std::collections::HashMap;

#[cfg(feature = "lsp")]
use std::io::Write;
#[cfg(feature = "lsp")]
use std::process::{Command, Stdio};
#[cfg(feature = "lsp")]
use std::time::Duration;

use rowan::TextRange;
use serde::Deserialize;

use crate::linter::diagnostics::{Diagnostic, Location};

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
#[cfg(feature = "lsp")]
pub async fn run_linter(
    linter_name: &str,
    code: &str,
    original_input: &str,
    registry: &ExternalLinterRegistry,
    mappings: Option<&[crate::linter::code_block_collector::BlockMapping]>,
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
    parse_linter_output(linter_name, &stdout, original_input, mappings)
}

/// Parse linter output based on linter type (public for sync version to reuse).
pub fn parse_linter_output(
    linter_name: &str,
    output: &str,
    original_input: &str,
    mappings: Option<&[crate::linter::code_block_collector::BlockMapping]>,
) -> Result<Vec<Diagnostic>, LinterError> {
    match linter_name {
        "jarl" => parse_jarl_output(output, original_input, mappings),
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

fn line_col_to_offset(input: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }

    let mut current_line = 1;
    let mut offset = 0;
    let bytes = input.as_bytes();

    for text_line in input.lines() {
        if current_line == line {
            let mut current_column = 1;
            for (byte_idx, _ch) in text_line.char_indices() {
                if current_column == column {
                    return Some(offset + byte_idx);
                }
                current_column += 1;
            }
            return Some(offset + text_line.len());
        }

        let line_end_offset = offset + text_line.len();
        let line_ending_len = if line_end_offset + 1 < input.len()
            && bytes[line_end_offset] == b'\r'
            && bytes[line_end_offset + 1] == b'\n'
        {
            2
        } else if line_end_offset < input.len() && bytes[line_end_offset] == b'\n' {
            1
        } else {
            0
        };

        offset += text_line.len() + line_ending_len;
        current_line += 1;
    }

    if current_line == line {
        Some(offset)
    } else {
        None
    }
}

/// Map a byte offset from the concatenated file to the original document.
///
/// Given a byte offset in the concatenated temporary file (with blank line padding),
/// find which code block it belongs to and map it to the corresponding byte offset
/// in the original document.
///
/// Returns `None` if the offset doesn't fall within any code block (e.g., it's in
/// the blank line padding between blocks).
fn map_concatenated_offset_to_original(
    offset: usize,
    mappings: &[crate::linter::code_block_collector::BlockMapping],
) -> Option<usize> {
    // Find which block contains this offset
    for mapping in mappings {
        if mapping.concatenated_range.contains(&offset) {
            // Offset is within this block
            // Calculate position relative to start of block in concatenated file
            let relative_offset = offset - mapping.concatenated_range.start;

            // Map to original document
            let original_offset = mapping.original_range.start + relative_offset;

            // Ensure we don't go past the end of the original block
            if original_offset <= mapping.original_range.end {
                return Some(original_offset);
            }
        }
    }

    None
}

/// Parse jarl JSON output into panache diagnostics.
///
/// If `mappings` is provided, auto-fixes from Jarl will be enabled and byte offsets
/// will be mapped from the concatenated file back to the original document.
fn parse_jarl_output(
    json: &str,
    input: &str,
    mappings: Option<&[crate::linter::code_block_collector::BlockMapping]>,
) -> Result<Vec<Diagnostic>, LinterError> {
    use crate::linter::diagnostics::{Edit, Fix};

    let output: JarlOutput = serde_json::from_str(json)
        .map_err(|e| LinterError::ParseError(format!("invalid jarl JSON: {}", e)))?;

    let mut diagnostics = Vec::new();

    for jarl_diag in output.diagnostics {
        // Convert location (jarl uses 1-indexed rows, 0-indexed columns)
        let line = jarl_diag.location.row; // Already 1-indexed
        let column = jarl_diag.location.column + 1; // Convert to 1-indexed

        let range_len = jarl_diag.range[1].saturating_sub(jarl_diag.range[0]);
        let start_offset = line_col_to_offset(input, line, column).unwrap_or(input.len());
        let end_offset = start_offset.saturating_add(range_len).min(input.len());

        // Convert byte range to TextRange (relative to original document)
        let range = TextRange::new((start_offset as u32).into(), (end_offset as u32).into());

        let location = Location {
            line,
            column,
            range,
        };

        // Convert fix if available and mappings are provided
        let fix = if let Some(mappings) = mappings {
            if !jarl_diag.fix.to_skip {
                // Map Jarl's byte offsets (in concatenated file) to original document
                if let (Some(fix_start), Some(fix_end)) = (
                    map_concatenated_offset_to_original(jarl_diag.fix.start, mappings),
                    map_concatenated_offset_to_original(jarl_diag.fix.end, mappings),
                ) {
                    let fix_range =
                        TextRange::new((fix_start as u32).into(), (fix_end as u32).into());
                    Some(Fix {
                        message: format!("Apply suggested fix: {}", jarl_diag.fix.content),
                        edits: vec![Edit {
                            range: fix_range,
                            replacement: jarl_diag.fix.content.clone(),
                        }],
                    })
                } else {
                    // Mapping failed - log and skip this fix
                    log::warn!(
                        "Failed to map Jarl fix offsets {}..{} to original document",
                        jarl_diag.fix.start,
                        jarl_diag.fix.end
                    );
                    None
                }
            } else {
                None
            }
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
    fn test_line_col_to_offset_basic() {
        let input = "line1\n\nline3\n";
        assert_eq!(line_col_to_offset(input, 1, 1), Some(0));
        assert_eq!(line_col_to_offset(input, 2, 1), Some(6));
        assert_eq!(line_col_to_offset(input, 3, 1), Some(7));
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
        let diagnostics = parse_jarl_output(json, input, None).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "assignment");
        assert_eq!(diagnostics[0].message, "Use `<-` for assignment.");
        assert_eq!(diagnostics[0].location.line, 1);
        assert_eq!(diagnostics[0].location.column, 1);
        assert_eq!(usize::from(diagnostics[0].location.range.start()), 0);
        assert_eq!(usize::from(diagnostics[0].location.range.end()), 3);
        // Without mappings, auto-fixes are disabled
        assert!(diagnostics[0].fix.is_none());
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
        let diagnostics = parse_jarl_output(json, input, None).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].fix.is_none());
    }

    #[test]
    fn test_map_concatenated_offset_single_block() {
        use crate::linter::code_block_collector::BlockMapping;

        let mappings = vec![BlockMapping {
            concatenated_range: 1..8, // "\nx <- 1\n" at offset 1-8
            original_range: 10..17,   // Original document offsets
            start_line: 2,
        }];

        // Test mapping within the block
        assert_eq!(map_concatenated_offset_to_original(1, &mappings), Some(10)); // Start
        assert_eq!(map_concatenated_offset_to_original(4, &mappings), Some(13)); // Middle
        assert_eq!(map_concatenated_offset_to_original(7, &mappings), Some(16)); // End-1

        // Test offset outside the block (in blank padding)
        assert_eq!(map_concatenated_offset_to_original(0, &mappings), None); // Before
        assert_eq!(map_concatenated_offset_to_original(8, &mappings), None); // After
        assert_eq!(map_concatenated_offset_to_original(100, &mappings), None); // Way past
    }

    #[test]
    fn test_map_concatenated_offset_multiple_blocks() {
        use crate::linter::code_block_collector::BlockMapping;

        let mappings = vec![
            BlockMapping {
                concatenated_range: 1..8, // First block at offset 1-8
                original_range: 10..17,
                start_line: 2,
            },
            BlockMapping {
                concatenated_range: 11..18, // Second block at offset 11-18
                original_range: 50..57,
                start_line: 6,
            },
        ];

        // First block
        assert_eq!(map_concatenated_offset_to_original(1, &mappings), Some(10));
        assert_eq!(map_concatenated_offset_to_original(5, &mappings), Some(14));

        // Gap between blocks (blank lines)
        assert_eq!(map_concatenated_offset_to_original(8, &mappings), None);
        assert_eq!(map_concatenated_offset_to_original(9, &mappings), None);
        assert_eq!(map_concatenated_offset_to_original(10, &mappings), None);

        // Second block
        assert_eq!(map_concatenated_offset_to_original(11, &mappings), Some(50));
        assert_eq!(map_concatenated_offset_to_original(15, &mappings), Some(54));
        assert_eq!(map_concatenated_offset_to_original(17, &mappings), Some(56));
    }

    #[test]
    fn test_map_concatenated_offset_edge_cases() {
        use crate::linter::code_block_collector::BlockMapping;

        let mappings = vec![BlockMapping {
            concatenated_range: 0..5,
            original_range: 100..105,
            start_line: 1,
        }];

        // Block starting at offset 0
        assert_eq!(map_concatenated_offset_to_original(0, &mappings), Some(100));
        assert_eq!(map_concatenated_offset_to_original(4, &mappings), Some(104));

        // Just past the end
        assert_eq!(map_concatenated_offset_to_original(5, &mappings), None);
    }

    #[test]
    fn test_parse_jarl_output_with_fix_and_mappings() {
        use crate::linter::code_block_collector::BlockMapping;

        // Simulates: original doc has R code at offsets 50-56 ("x = 1\n")
        // Concatenated file has it at offsets 0-6 (no padding since it starts at line 1)
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

        let concatenated_input = "x = 1\n";
        let mappings = vec![BlockMapping {
            concatenated_range: 0..6,
            original_range: 50..56,
            start_line: 1,
        }];

        let diagnostics = parse_jarl_output(json, concatenated_input, Some(&mappings)).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "assignment");

        // Check that fix is now present
        assert!(diagnostics[0].fix.is_some());
        let fix = diagnostics[0].fix.as_ref().unwrap();

        assert_eq!(fix.edits.len(), 1);
        // Fix range should be mapped from concatenated (0..5) to original (50..55)
        assert_eq!(usize::from(fix.edits[0].range.start()), 50);
        assert_eq!(usize::from(fix.edits[0].range.end()), 55);
        assert_eq!(fix.edits[0].replacement, "x <- 1");
    }
}
