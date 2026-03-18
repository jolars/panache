//! Bibliography extraction from YAML frontmatter.

use std::collections::HashMap;
use std::path::PathBuf;

use rowan::TextRange;

use super::DocumentMetadata;

/// Information about bibliography files extracted from frontmatter.
#[derive(Debug, Clone)]
pub struct BibliographyInfo {
    /// Resolved absolute paths to bibliography files.
    pub paths: Vec<PathBuf>,
    /// Source text ranges for each path in the document (for diagnostics).
    pub source_ranges: Vec<TextRange>,
}

#[derive(Debug, Clone)]
pub struct BibliographyParse {
    pub index: crate::bib::BibIndex,
    pub parse_errors: Vec<String>,
}

pub fn bibliography_range_map(metadata: &DocumentMetadata) -> HashMap<PathBuf, TextRange> {
    let mut map = HashMap::new();
    if let Some(info) = metadata.bibliography.as_ref() {
        for (path, range) in info.paths.iter().zip(info.source_ranges.iter()) {
            map.insert(path.clone(), *range);
        }
    }
    map
}

pub fn format_bibliography_load_error(message: &str) -> String {
    if message.contains("No such file or directory") {
        "File not found".to_string()
    } else {
        message.to_string()
    }
}
