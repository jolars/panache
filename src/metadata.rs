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
mod citations;
mod project;
mod yaml;

pub use bibliography::BibliographyInfo;
pub use bibliography::BibliographyParse;
pub use citations::{CitationInfo, extract_citations};
pub use yaml::YamlError;

/// Structured metadata extracted from document frontmatter.
#[derive(Debug, Clone)]
pub struct DocumentMetadata {
    /// Bibliography information (files and their source positions).
    pub bibliography: Option<BibliographyInfo>,
    /// Parsed bibliography data (if available).
    pub bibliography_parse: Option<BibliographyParse>,
    /// Citation keys in the document.
    pub citations: CitationInfo,
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
    let mut metadata = yaml::parse_frontmatter(&yaml_text, yaml_offset, doc_path)?;
    metadata.citations = extract_citations(tree);
    Ok(metadata)
}

/// Extract metadata using Quarto project configuration when available.
pub fn extract_project_metadata(
    tree: &crate::syntax::SyntaxNode,
    doc_path: &Path,
) -> Result<DocumentMetadata, YamlError> {
    project::extract_project_metadata(tree, doc_path)
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
    use std::fs;
    use tempfile::TempDir;

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
        assert!(metadata.citations.keys.is_empty());
    }

    #[test]
    fn test_extract_project_metadata_merges_sources() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();
        fs::write(project_root.join("_quarto.yml"), "bibliography: proj.bib\n").unwrap();
        fs::create_dir_all(project_root.join("docs")).unwrap();
        fs::write(
            project_root.join("docs/_metadata.yml"),
            "bibliography: dir.bib\n",
        )
        .unwrap();
        fs::write(project_root.join("docs/refs.bib"), "@book{doc,}\n").unwrap();
        fs::write(project_root.join("proj.bib"), "@book{proj,}\n").unwrap();
        fs::write(project_root.join("dir.bib"), "@book{dir,}\n").unwrap();

        let input = "---\nbibliography: refs.bib\n---\n\nText";
        let tree = parse(input, None);
        let doc_path = project_root.join("docs/doc.qmd");
        let metadata = extract_project_metadata(&tree, &doc_path).unwrap();

        let bib = metadata.bibliography.expect("bibliography");
        let paths: Vec<_> = bib
            .paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"proj.bib".to_string()));
        assert!(paths.contains(&"dir.bib".to_string()));
        assert!(paths.contains(&"refs.bib".to_string()));
    }

    #[test]
    fn test_extract_project_metadata_includes_metadata_files() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();
        fs::write(
            project_root.join("_quarto.yml"),
            "metadata-files:\n  - _website.yml\n",
        )
        .unwrap();
        fs::write(
            project_root.join("_website.yml"),
            "bibliography: site.bib\n",
        )
        .unwrap();
        fs::write(project_root.join("site.bib"), "@book{site,}\n").unwrap();

        let input = "---\n---\n\nText";
        let tree = parse(input, None);
        let doc_path = project_root.join("doc.qmd");
        let metadata = extract_project_metadata(&tree, &doc_path).unwrap();

        let bib = metadata.bibliography.expect("bibliography");
        let paths: Vec<_> = bib
            .paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"site.bib".to_string()));
    }
}
