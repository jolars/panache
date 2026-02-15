//! YAML frontmatter parsing with position tracking.

use std::path::Path;

use rowan::TextSize;
use serde::Deserialize;
use serde_saphyr::Spanned;

use super::DocumentMetadata;

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
            }
        } else {
            Self::ParseError {
                message: err.to_string(),
                line: 0,
                column: 0,
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

    // Parse with serde-saphyr
    let frontmatter: Frontmatter = serde_saphyr::from_str(&yaml_content)?;

    // Extract bibliography info if present
    let bibliography = frontmatter
        .bibliography
        .map(|spanned| {
            super::bibliography::extract_bibliography_info(spanned, yaml_offset, doc_path)
        })
        .transpose()?;

    Ok(DocumentMetadata {
        bibliography,
        title: frontmatter.title,
        raw_yaml: yaml_content.to_string(),
    })
}

/// Strip YAML delimiters (---) from frontmatter text.
fn strip_yaml_delimiters(text: &str) -> String {
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
}
