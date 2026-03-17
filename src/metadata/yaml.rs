//! YAML frontmatter parsing with position tracking.

use std::path::Path;

use rowan::TextSize;
use serde::Deserialize;
use serde_saphyr::Spanned;

use super::references::extract_inline_references;
use super::{BibliographyParse, DocumentMetadata, ReferenceEntry};
use crate::bib;

/// Errors that can occur during YAML parsing.
#[derive(Debug, Clone)]
pub enum YamlError {
    /// YAML frontmatter not found in document.
    NotFound(String),
    /// YAML syntax error.
    ParseError {
        message: String,
        line: u64,
        column: u64,
        byte_offset: Option<usize>,
    },
    /// Invalid YAML structure.
    StructureError(String),
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(msg) => write!(f, "YAML not found: {}", msg),
            Self::ParseError {
                message,
                line,
                column,
                ..
            } => {
                write!(f, "YAML parse error at {}:{}: {}", line, column, message)
            }
            Self::StructureError(msg) => write!(f, "Invalid YAML structure: {}", msg),
        }
    }
}

impl std::error::Error for YamlError {}

impl From<serde_saphyr::Error> for YamlError {
    fn from(err: serde_saphyr::Error) -> Self {
        // Extract location from serde-saphyr error if available
        if let Some(location) = err.location() {
            Self::ParseError {
                message: err.to_string(),
                line: location.line(),
                column: location.column(),
                byte_offset: None,
            }
        } else {
            Self::ParseError {
                message: err.to_string(),
                line: 0,
                column: 0,
                byte_offset: None,
            }
        }
    }
}

/// Internal representation of YAML frontmatter fields.
#[derive(Debug, Deserialize)]
struct Frontmatter {
    /// Document title.
    title: Option<String>,
    /// Bibliography files (string or array).
    bibliography: Option<Spanned<StringOrArray>>,
    /// Inline YAML references.
    references: Option<Vec<ReferenceEntry>>,
    // Additional fields can be added here as needed
}

/// Helper type to deserialize both string and array forms.
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum StringOrArray {
    Single(String),
    Multiple(Vec<String>),
}

impl StringOrArray {
    /// Get all paths from the value.
    pub fn paths(&self) -> Vec<&str> {
        match self {
            Self::Single(s) => vec![s.as_str()],
            Self::Multiple(v) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

/// Parse YAML frontmatter and extract metadata.
///
/// # Arguments
///
/// * `yaml_text` - The YAML content (without --- delimiters)
/// * `yaml_offset` - Byte offset of YAML content in the document
/// * `doc_path` - Path to the document (for resolving relative paths)
pub(super) fn parse_frontmatter(
    yaml_text: &str,
    yaml_offset: TextSize,
    doc_path: &Path,
) -> Result<DocumentMetadata, YamlError> {
    // Extract just the YAML content (strip delimiters)
    let yaml_content = strip_yaml_delimiters(yaml_text);
    let content_start = yaml_content_start_offset(yaml_text);
    let doc_base_offset = u32::from(yaml_offset) as usize + content_start;

    crate::yaml_engine::validate_yaml(&yaml_content).map_err(|err| {
        let content_byte_offset = err.offset.min(yaml_content.len());
        let (line, column) = byte_offset_to_line_col_1based(&yaml_content, content_byte_offset);
        YamlError::ParseError {
            message: err.message,
            line: line as u64,
            column: column as u64,
            byte_offset: Some(doc_base_offset + content_byte_offset),
        }
    })?;

    // Parse with serde-saphyr
    let frontmatter: Frontmatter = serde_saphyr::from_str(&yaml_content)
        .map_err(|err| map_yaml_parse_error(err, &yaml_content, yaml_offset, content_start))?;

    // Extract bibliography info if present
    let bibliography = frontmatter
        .bibliography
        .map(|spanned| {
            super::bibliography::extract_bibliography_info(spanned, yaml_offset, doc_path)
        })
        .transpose()?;

    let bibliography_parse = bibliography.as_ref().map(|info| {
        let index = bib::load_bibliography(&info.paths);
        BibliographyParse {
            parse_errors: index
                .errors
                .iter()
                .map(|error| error.message.clone())
                .collect(),
            index,
        }
    });
    let inline_references = frontmatter
        .references
        .map(|refs| extract_inline_references(refs, yaml_offset, doc_path))
        .unwrap_or_default();

    Ok(DocumentMetadata {
        bibliography,
        metadata_files: Vec::new(),
        bibliography_parse,
        inline_references,
        citations: super::CitationInfo { keys: Vec::new() },
        title: frontmatter.title,
        raw_yaml: yaml_content.to_string(),
    })
}

fn map_yaml_parse_error(
    err: serde_saphyr::Error,
    yaml_content: &str,
    yaml_offset: TextSize,
    content_start: usize,
) -> YamlError {
    if let Some(location) = err.location() {
        let line = location.line();
        let column = location.column();
        let content_byte_offset =
            line_col_to_byte_offset_1based(yaml_content, line as usize, column as usize)
                .unwrap_or(yaml_content.len());
        let absolute = u32::from(yaml_offset) as usize + content_start + content_byte_offset;
        YamlError::ParseError {
            message: err.to_string(),
            line,
            column,
            byte_offset: Some(absolute),
        }
    } else {
        YamlError::from(err)
    }
}

fn line_col_to_byte_offset_1based(input: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }

    let mut offset = 0usize;
    let bytes = input.as_bytes();

    for (idx, text_line) in input.lines().enumerate() {
        let line_no = idx + 1;
        if line_no == line {
            let line_byte_offset = text_line
                .char_indices()
                .nth(column.saturating_sub(1))
                .map(|(byte, _)| byte)
                .unwrap_or(text_line.len());
            return Some(offset + line_byte_offset);
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
    }

    if line == input.lines().count() + 1 && column == 1 {
        Some(offset)
    } else {
        None
    }
}

fn byte_offset_to_line_col_1based(input: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut line_start = 0usize;
    let mut i = 0usize;
    let bytes = input.as_bytes();
    let target = offset.min(input.len());

    while i < target {
        if bytes[i] == b'\n' {
            line += 1;
            line_start = i + 1;
        }
        i += 1;
    }
    let col = input[line_start..target].chars().count() + 1;
    (line, col)
}

fn yaml_content_start_offset(text: &str) -> usize {
    let Some(first) = text.lines().next() else {
        return 0;
    };
    if first.trim() != "---" {
        return 0;
    }
    if text[first.len()..].starts_with("\r\n") {
        first.len() + 2
    } else if text[first.len()..].starts_with('\n') {
        first.len() + 1
    } else {
        first.len()
    }
}

/// Strip YAML delimiters (---) from frontmatter text.
pub(super) fn strip_yaml_delimiters(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();

    if lines.is_empty() {
        return text.to_string();
    }

    // Check if starts with ---
    let start_idx = if lines[0].trim() == "---" { 1 } else { 0 };

    // Check if ends with --- or ...
    let mut end_idx = lines.len();
    if end_idx > start_idx {
        let last_line = lines[end_idx - 1].trim();
        if last_line == "---" || last_line == "..." {
            end_idx -= 1;
        }
    }

    // Reconstruct the content
    lines[start_idx..end_idx].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_yaml_delimiters() {
        let input = "---\ntitle: Test\n---";
        let stripped = strip_yaml_delimiters(input);
        assert_eq!(stripped, "title: Test");
    }

    #[test]
    fn test_strip_yaml_delimiters_dots() {
        let input = "---\ntitle: Test\n...";
        let stripped = strip_yaml_delimiters(input);
        assert_eq!(stripped, "title: Test");
    }

    #[test]
    fn test_parse_simple_frontmatter() {
        let yaml = "title: My Document\nauthor: John Doe";
        let result = parse_frontmatter(yaml, TextSize::from(0), Path::new("test.qmd"));
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.title.as_deref(), Some("My Document"));
        assert!(metadata.citations.keys.is_empty());
    }

    #[test]
    fn test_parse_with_delimiters() {
        let yaml = "---\ntitle: My Document\n---";
        let result = parse_frontmatter(yaml, TextSize::from(0), Path::new("test.qmd"));
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.title.as_deref(), Some("My Document"));
    }

    #[test]
    fn test_yaml_content_start_offset() {
        let yaml = "---\ntitle: Test\n---";
        assert_eq!(yaml_content_start_offset(yaml), 4);
    }

    #[test]
    fn test_yaml_content_start_offset_crlf() {
        let yaml = "---\r\ntitle: Test\r\n---";
        assert_eq!(yaml_content_start_offset(yaml), 5);
    }

    #[test]
    fn test_parse_error_includes_document_byte_offset() {
        let yaml = "---\ntitle: [\n---";
        let base = TextSize::from(10);
        let err = parse_frontmatter(yaml, base, Path::new("test.qmd")).expect_err("parse error");
        match err {
            YamlError::ParseError {
                line,
                column,
                byte_offset,
                ..
            } => {
                let local =
                    line_col_to_byte_offset_1based("title: [", line as usize, column as usize)
                        .unwrap_or("title: [".len());
                let expected = 10 + yaml_content_start_offset(yaml) + local;
                assert_eq!(byte_offset, Some(expected));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_string_or_array_single() {
        // Test via Frontmatter struct, not directly
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct Test {
            bibliography: StringOrArray,
        }

        let yaml = r#"
bibliography: refs.bib
"#;
        let value: Test = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(value.bibliography.paths(), vec!["refs.bib"]);
    }

    #[test]
    fn test_string_or_array_multiple() {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct Test {
            bibliography: StringOrArray,
        }

        let yaml = r#"
bibliography: 
  - refs.bib
  - other.bib
"#;
        let value: Test = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(value.bibliography.paths(), vec!["refs.bib", "other.bib"]);
    }

    #[test]
    fn test_byte_offset_to_line_col_1based() {
        let input = "a\néx\n";
        assert_eq!(byte_offset_to_line_col_1based(input, 0), (1, 1));
        assert_eq!(byte_offset_to_line_col_1based(input, 2), (2, 1));
        assert_eq!(byte_offset_to_line_col_1based(input, 4), (2, 2));
    }
}
