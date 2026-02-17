//! Metadata extraction from YAML frontmatter.
//!
//! This module provides functionality to parse YAML frontmatter from documents
//! and extract structured metadata, particularly bibliography information for
//! LSP features like citation completion and validation.
//!
//! # Architecture
//!
//! - YAML frontmatter remains **opaque** in the CST (stored as TEXT nodes)
//! - Semantic parsing happens on-demand using serde-saphyr
//! - `Spanned<T>` provides byte-precise positions for diagnostics
//! - Position mapping: `document_offset = yaml_node.start() + yaml_span.offset()`
//!
//! # Usage
//!
//! ```rust,ignore
//! use panache::metadata::extract_metadata;
//!
//! let tree = parse(document);
//! let metadata = extract_metadata(&tree, Path::new("document.qmd"))?;
//!
//! if let Some(bib_info) = metadata.bibliography {
//!     for path in &bib_info.paths {
//!         println!("Bibliography: {:?}", path);
//!     }
//! }
//! ```

use std::path::Path;

mod bibliography;
mod yaml;

pub use bibliography::BibliographyInfo;
pub use yaml::YamlError;

/// Structured metadata extracted from document frontmatter.
#[derive(Debug, Clone)]
pub struct DocumentMetadata {
    /// Bibliography information (files and their source positions).
    pub bibliography: Option<BibliographyInfo>,
    /// Document title.
    pub title: Option<String>,
    /// Raw YAML text for future queries.
    pub raw_yaml: String,
}

/// Extract metadata from a syntax tree.
///
/// Finds YAML frontmatter in the document, parses it with serde-saphyr,
/// and extracts structured metadata including bibliography paths.
///
/// # Arguments
///
/// * `tree` - The syntax tree to extract metadata from
/// * `doc_path` - Path to the document (for resolving relative bibliography paths)
///
/// # Errors
///
/// Returns `YamlError` if:
/// - YAML parsing fails
/// - YAML structure is invalid
pub fn extract_metadata(
    tree: &crate::syntax::SyntaxNode,
    doc_path: &Path,
) -> Result<DocumentMetadata, YamlError> {
    // Find YamlMetadata node in CST
    let yaml_node = find_yaml_metadata_node(tree)
        .ok_or_else(|| YamlError::NotFound("No YAML frontmatter found in document".to_string()))?;

    // Extract text and calculate offset
    let yaml_text = yaml_node.text().to_string();
    let yaml_offset = yaml_node.text_range().start();

    // Parse YAML with position tracking
    yaml::parse_frontmatter(&yaml_text, yaml_offset, doc_path)
}

/// Find the first YamlMetadata node in the syntax tree.
fn find_yaml_metadata_node(tree: &crate::syntax::SyntaxNode) -> Option<crate::syntax::SyntaxNode> {
    use crate::syntax::SyntaxKind;

    tree.descendants()
        .find(|child| child.kind() == SyntaxKind::YAML_METADATA)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn test_find_yaml_metadata() {
        let input = "---\ntitle: Test\n---\n\nContent";
        let tree = parse(input, None);
        let yaml_node = find_yaml_metadata_node(&tree);
        assert!(yaml_node.is_some());
    }

    #[test]
    fn test_no_yaml_metadata() {
        let input = "# Heading\n\nContent";
        let tree = parse(input, None);
        let yaml_node = find_yaml_metadata_node(&tree);
        assert!(yaml_node.is_none());
    }

    #[test]
    fn test_extract_metadata_basic() {
        let input = "---\ntitle: My Document\nbibliography: refs.bib\n---\n\nContent";
        let tree = parse(input, None);
        let metadata = extract_metadata(&tree, Path::new("test.qmd")).unwrap();

        assert_eq!(metadata.title.as_deref(), Some("My Document"));
        assert!(metadata.bibliography.is_some());
    }
}
