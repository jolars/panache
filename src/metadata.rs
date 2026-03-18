//! Metadata extraction from YAML frontmatter.
//!
//! This module provides functionality to parse YAML frontmatter from documents
//! and extract structured metadata, particularly bibliography information for
//! LSP features like citation completion and validation.
//!
//! # Architecture
//!
//! - YAML frontmatter remains **opaque** in the CST (stored as TEXT nodes)
//! - Semantic parsing happens on-demand using `yaml_parser` AST traversal
//! - Metadata offsets are derived from YAML token ranges and mapped to document offsets
//! - CSL-YAML bibliography parsing is handled through `yaml_parser`
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
pub(crate) mod project;
mod references;
mod yaml;

pub use bibliography::{
    BibliographyInfo, BibliographyParse, bibliography_range_map, format_bibliography_load_error,
};
pub use citations::{CitationInfo, extract_citations};
pub use references::{
    InlineBibConflict, InlineReference, InlineReferenceDuplicate, ReferenceEntry,
    inline_bib_conflicts, inline_reference_contains, inline_reference_duplicates,
    inline_reference_map,
};
pub use yaml::YamlError;

/// Structured metadata extracted from document frontmatter.
#[derive(Debug, Clone)]
pub struct DocumentMetadata {
    /// Bibliography information (files and their source positions).
    pub bibliography: Option<BibliographyInfo>,
    /// Metadata file dependencies (resolved paths).
    pub metadata_files: Vec<std::path::PathBuf>,
    /// Parsed bibliography data (if available).
    pub bibliography_parse: Option<BibliographyParse>,
    /// Inline references from YAML metadata.
    pub inline_references: Vec<InlineReference>,
    /// Citation keys in the document.
    pub citations: CitationInfo,
    /// Document title.
    pub title: Option<String>,
    /// Raw YAML text for future queries.
    pub raw_yaml: String,
}

/// Extract metadata from a syntax tree.
///
/// Finds YAML frontmatter in the document, parses it with `yaml_parser`,
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

/// Extract project metadata without parsing bibliography files.
///
/// This is useful when bibliography parsing is handled separately (e.g. through salsa caching).
pub fn extract_project_metadata_without_bibliography_parse(
    tree: &crate::syntax::SyntaxNode,
    doc_path: &Path,
) -> Result<DocumentMetadata, YamlError> {
    project::extract_project_metadata_without_bibliography_parse(tree, doc_path)
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
        assert!(metadata.inline_references.is_empty());
        assert!(metadata.citations.keys.is_empty());
    }

    #[test]
    fn test_extract_metadata_inline_references() {
        let input = "---\nreferences:\n  - id: InlineRef\n    title: Sample\n---\n\nContent";
        let tree = parse(input, None);
        let metadata = extract_metadata(&tree, Path::new("test.qmd")).unwrap();

        assert_eq!(metadata.inline_references.len(), 1);
        assert_eq!(metadata.inline_references[0].id, "InlineRef");
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
    fn test_extract_project_metadata_resolves_quarto_bibliography_from_root() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();
        fs::write(
            project_root.join("_quarto.yml"),
            "bibliography: references.bib\n",
        )
        .unwrap();
        fs::write(
            project_root.join("references.bib"),
            "@article{smith2020,}\n",
        )
        .unwrap();
        fs::create_dir_all(project_root.join("notebooks")).unwrap();

        let input = "# Test\n";
        let tree = parse(input, None);
        let doc_path = project_root.join("notebooks/study_I.qmd");
        let metadata = extract_project_metadata(&tree, &doc_path).unwrap();

        let bib = metadata.bibliography.expect("bibliography");
        assert_eq!(bib.paths.len(), 1);
        assert_eq!(bib.paths[0], project_root.join("references.bib"));
    }

    #[test]
    fn test_extract_project_metadata_inline_references() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();
        fs::write(
            project_root.join("_quarto.yml"),
            "references:\n  - id: projref\n",
        )
        .unwrap();

        let input = "---\nreferences:\n  - id: docref\n---\n\nText";
        let tree = parse(input, None);
        let doc_path = project_root.join("doc.qmd");
        let metadata = extract_project_metadata(&tree, &doc_path).unwrap();

        let ids: Vec<_> = metadata
            .inline_references
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        assert!(ids.contains(&"projref"));
        assert!(ids.contains(&"docref"));
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

    #[test]
    fn test_extract_project_metadata_bookdown_index_frontmatter() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();
        fs::write(project_root.join("_bookdown.yml"), "output_dir: _book\n").unwrap();
        fs::write(project_root.join("book.bib"), "@book{book,}\n").unwrap();
        fs::write(
            project_root.join("index.Rmd"),
            "---\nbibliography: book.bib\n---\n\n# Title\n",
        )
        .unwrap();

        let input = "---\n---\n\nText";
        let tree = parse(input, None);
        let doc_path = project_root.join("chapter1.Rmd");
        let metadata = extract_project_metadata(&tree, &doc_path).unwrap();

        let bib = metadata.bibliography.expect("bibliography");
        let paths: Vec<_> = bib
            .paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"book.bib".to_string()));
    }

    #[test]
    fn test_extract_project_metadata_bookdown_first_file() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();
        fs::write(
            project_root.join("_bookdown.yml"),
            "rmd_files: [\"intro.Rmd\", \"chapter.Rmd\"]\n",
        )
        .unwrap();
        fs::write(project_root.join("intro.bib"), "@book{intro,}\n").unwrap();
        fs::write(
            project_root.join("intro.Rmd"),
            "---\nbibliography: intro.bib\n---\n\n# Intro\n",
        )
        .unwrap();

        let input = "---\n---\n\nText";
        let tree = parse(input, None);
        let doc_path = project_root.join("chapter.Rmd");
        let metadata = extract_project_metadata(&tree, &doc_path).unwrap();

        let bib = metadata.bibliography.expect("bibliography");
        let paths: Vec<_> = bib
            .paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"intro.bib".to_string()));
    }

    #[test]
    fn test_extract_project_metadata_bookdown_default_index_first() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();
        fs::write(project_root.join("_bookdown.yml"), "output_dir: _book\n").unwrap();
        fs::write(project_root.join("index.bib"), "@book{index,}\n").unwrap();
        fs::write(
            project_root.join("index.Rmd"),
            "---\nbibliography: index.bib\n---\n\n# Index\n",
        )
        .unwrap();

        let input = "---\n---\n\nText";
        let tree = parse(input, None);
        let doc_path = project_root.join("chapter.Rmd");
        let metadata = extract_project_metadata(&tree, &doc_path).unwrap();

        let bib = metadata.bibliography.expect("bibliography");
        let paths: Vec<_> = bib
            .paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"index.bib".to_string()));
    }

    #[test]
    fn test_extract_project_metadata_bookdown_rmd_files_by_format() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();
        fs::write(
            project_root.join("_bookdown.yml"),
            "rmd_files:\n  html:\n    - intro.Rmd\n    - chapter.Rmd\n",
        )
        .unwrap();
        fs::write(project_root.join("intro.bib"), "@book{intro,}\n").unwrap();
        fs::write(
            project_root.join("intro.Rmd"),
            "---\nbibliography: intro.bib\n---\n\n# Intro\n",
        )
        .unwrap();

        let input = "---\n---\n\nText";
        let tree = parse(input, None);
        let doc_path = project_root.join("chapter.Rmd");
        let metadata = extract_project_metadata(&tree, &doc_path).unwrap();

        let bib = metadata.bibliography.expect("bibliography");
        let paths: Vec<_> = bib
            .paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"intro.bib".to_string()));
    }
}
