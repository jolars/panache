//! YAML frontmatter parsing with position tracking.

use std::path::Path;

use rowan::TextSize;
use rowan::ast::AstNode as _;

use super::{BibliographyInfo, BibliographyParse, DocumentMetadata, InlineReference};
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

#[derive(Debug, Clone)]
struct ScalarValue {
    value: String,
    range: std::ops::Range<usize>,
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
        let content_byte_offset = err.offset().min(yaml_content.len());
        let (line, column) = byte_offset_to_line_col_1based(&yaml_content, content_byte_offset);
        YamlError::ParseError {
            message: err.message().to_string(),
            line: line as u64,
            column: column as u64,
            byte_offset: Some(doc_base_offset + content_byte_offset),
        }
    })?;

    let root = yaml_parser::ast::Root::cast(yaml_parser::parse(&yaml_content).map_err(|err| {
        let content_byte_offset = err.offset().min(yaml_content.len());
        let (line, column) = byte_offset_to_line_col_1based(&yaml_content, content_byte_offset);
        YamlError::ParseError {
            message: err.message().to_string(),
            line: line as u64,
            column: column as u64,
            byte_offset: Some(doc_base_offset + content_byte_offset),
        }
    })?)
    .ok_or_else(|| YamlError::StructureError("Invalid YAML root".to_string()))?;
    let map = root
        .documents()
        .next()
        .and_then(|doc| doc.block())
        .and_then(|block| block.block_map());

    let title = map
        .as_ref()
        .and_then(|map| map_entry_value(map, "title"))
        .and_then(block_map_value_to_scalar);
    let bibliography_values = map
        .as_ref()
        .and_then(|map| map_entry_value(map, "bibliography"))
        .map(block_map_value_to_scalar_list)
        .unwrap_or_default();
    let bibliography = if bibliography_values.is_empty() {
        None
    } else {
        let doc_dir = doc_path.parent().unwrap_or_else(|| Path::new("."));
        let paths = bibliography_values
            .iter()
            .map(|entry| doc_dir.join(&entry.value))
            .collect();
        let source_ranges = bibliography_values
            .iter()
            .map(|entry| absolute_text_range(doc_base_offset, entry.range.start..entry.range.end))
            .collect();
        Some(BibliographyInfo {
            paths,
            source_ranges,
        })
    };

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
    let inline_references = map
        .as_ref()
        .and_then(|map| map_entry_value(map, "references"))
        .map(|value| extract_inline_references_from_yaml(value, doc_base_offset, doc_path))
        .unwrap_or_default();

    Ok(DocumentMetadata {
        source_path: doc_path.to_path_buf(),
        bibliography,
        metadata_files: Vec::new(),
        bibliography_parse,
        inline_references,
        citations: super::CitationInfo { keys: Vec::new() },
        title: title.map(|entry| entry.value),
        raw_yaml: yaml_content.to_string(),
    })
}

fn map_entry_value(
    map: &yaml_parser::ast::BlockMap,
    key: &str,
) -> Option<yaml_parser::ast::BlockMapValue> {
    map.entries()
        .find(|entry| block_map_entry_key(entry).as_deref() == Some(key))
        .and_then(|entry| entry.value())
}

fn block_map_entry_key(entry: &yaml_parser::ast::BlockMapEntry) -> Option<String> {
    let key = entry.key()?;
    if let Some(flow) = key.flow() {
        return flow_scalar_text(&flow);
    }
    let block = key.block()?;
    let flow = block_to_flow_scalar(&block)?;
    flow_scalar_text(&flow)
}

fn block_map_value_to_scalar(value: yaml_parser::ast::BlockMapValue) -> Option<ScalarValue> {
    if let Some(flow) = value.flow() {
        return flow_scalar(&flow);
    }
    let block = value.block()?;
    let flow = block_to_flow_scalar(&block)?;
    flow_scalar(&flow)
}

fn block_map_value_to_scalar_list(value: yaml_parser::ast::BlockMapValue) -> Vec<ScalarValue> {
    if let Some(single) = block_map_value_to_scalar(value.clone()) {
        return vec![single];
    }
    if let Some(flow) = value.flow()
        && let Some(seq) = flow.flow_seq()
    {
        return seq
            .entries()
            .into_iter()
            .flat_map(|entries| entries.entries())
            .filter_map(|entry| entry.flow().and_then(|flow| flow_scalar(&flow)))
            .collect();
    }
    if let Some(block) = value.block()
        && let Some(seq) = block.block_seq()
    {
        return seq
            .entries()
            .filter_map(|entry| {
                if let Some(flow) = entry.flow() {
                    return flow_scalar(&flow);
                }
                let block = entry.block()?;
                let flow = block_to_flow_scalar(&block)?;
                flow_scalar(&flow)
            })
            .collect();
    }
    Vec::new()
}

fn extract_inline_references_from_yaml(
    references: yaml_parser::ast::BlockMapValue,
    doc_base_offset: usize,
    doc_path: &Path,
) -> Vec<InlineReference> {
    let Some(block) = references.block() else {
        return Vec::new();
    };
    let Some(seq) = block.block_seq() else {
        return Vec::new();
    };
    seq.entries()
        .filter_map(|entry| {
            let block = entry.block()?;
            let map = block.block_map()?;
            let id_value = map_entry_value(&map, "id")?;
            let id = block_map_value_to_scalar(id_value)?;
            Some(InlineReference {
                id: id.value,
                range: absolute_text_range(doc_base_offset, id.range),
                path: doc_path.to_path_buf(),
            })
        })
        .collect()
}

fn block_to_flow_scalar(block: &yaml_parser::ast::Block) -> Option<yaml_parser::ast::Flow> {
    block
        .syntax()
        .children()
        .find_map(yaml_parser::ast::Flow::cast)
}

fn flow_scalar(flow: &yaml_parser::ast::Flow) -> Option<ScalarValue> {
    let token = if let Some(token) = flow.plain_scalar() {
        token
    } else if let Some(token) = flow.single_quoted_scalar() {
        token
    } else {
        flow.double_qouted_scalar()?
    };
    let mut value = token.text().to_string();
    if token.kind() == yaml_parser::SyntaxKind::SINGLE_QUOTED_SCALAR {
        value = value.trim_matches('\'').to_string();
    } else if token.kind() == yaml_parser::SyntaxKind::DOUBLE_QUOTED_SCALAR {
        value = value.trim_matches('"').to_string();
    }
    let start: usize = token.text_range().start().into();
    let end: usize = token.text_range().end().into();
    Some(ScalarValue {
        value,
        range: start..end,
    })
}

fn flow_scalar_text(flow: &yaml_parser::ast::Flow) -> Option<String> {
    flow_scalar(flow).map(|scalar| scalar.value)
}

fn absolute_text_range(base_offset: usize, range: std::ops::Range<usize>) -> rowan::TextRange {
    rowan::TextRange::new(
        rowan::TextSize::from((base_offset + range.start) as u32),
        rowan::TextSize::from((base_offset + range.end) as u32),
    )
}

#[cfg(test)]
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
        let yaml = "---\nbibliography: refs.bib\n---";
        let metadata = parse_frontmatter(yaml, TextSize::from(0), Path::new("test.qmd")).unwrap();
        let bib = metadata.bibliography.expect("bibliography");
        assert_eq!(bib.paths.len(), 1);
        assert!(bib.paths[0].ends_with("refs.bib"));
    }

    #[test]
    fn test_string_or_array_multiple() {
        let yaml = "---\nbibliography:\n  - refs.bib\n  - other.bib\n---";
        let metadata = parse_frontmatter(yaml, TextSize::from(0), Path::new("test.qmd")).unwrap();
        let bib = metadata.bibliography.expect("bibliography");
        assert_eq!(bib.paths.len(), 2);
        assert!(bib.paths[0].ends_with("refs.bib"));
        assert!(bib.paths[1].ends_with("other.bib"));
    }

    #[test]
    fn test_byte_offset_to_line_col_1based() {
        let input = "a\néx\n";
        assert_eq!(byte_offset_to_line_col_1based(input, 0), (1, 1));
        assert_eq!(byte_offset_to_line_col_1based(input, 2), (2, 1));
        assert_eq!(byte_offset_to_line_col_1based(input, 4), (2, 2));
    }
}
