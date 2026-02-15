//! Bibliography extraction from YAML frontmatter.

use std::path::{Path, PathBuf};

use rowan::{TextRange, TextSize};
use serde_saphyr::Spanned;

use super::yaml::{StringOrArray, YamlError};

/// Information about bibliography files extracted from frontmatter.
#[derive(Debug, Clone)]
pub struct BibliographyInfo {
    /// Resolved absolute paths to bibliography files.
    pub paths: Vec<PathBuf>,
    /// Source text ranges for each path in the document (for diagnostics).
    pub source_ranges: Vec<TextRange>,
}

/// Extract bibliography information from a spanned YAML value.
///
/// # Arguments
///
/// * `spanned` - The bibliography value with position information
/// * `yaml_offset` - Byte offset of the YAML block in the document
/// * `doc_path` - Path to the document (for resolving relative paths)
pub(super) fn extract_bibliography_info(
    spanned: Spanned<StringOrArray>,
    yaml_offset: TextSize,
    doc_path: &Path,
) -> Result<BibliographyInfo, YamlError> {
    let paths_str = spanned.value.paths();

    // Get position information from Spanned<T>
    let referenced_location = spanned.referenced;
    let span = referenced_location.span();

    // Get byte offset within YAML (falls back to character offset if unavailable)
    let yaml_byte_offset = span.byte_offset().unwrap_or(span.offset());
    let yaml_byte_len = span.byte_len().unwrap_or(span.len());

    // Calculate absolute position in document
    let doc_offset = yaml_offset + TextSize::from(yaml_byte_offset as u32);

    // For now, create a single range for the whole bibliography field
    // TODO: When dealing with arrays, we could parse each element separately
    let range = TextRange::new(
        doc_offset,
        doc_offset + TextSize::from(yaml_byte_len as u32),
    );

    // Resolve paths relative to document directory
    let doc_dir = doc_path.parent().unwrap_or_else(|| Path::new("."));

    let mut resolved_paths = Vec::new();
    let mut ranges = Vec::new();

    for path_str in paths_str {
        let path = doc_dir.join(path_str);
        resolved_paths.push(path);
        // For now, use the same range for all paths
        // In the future, we could parse array elements individually
        ranges.push(range);
    }

    Ok(BibliographyInfo {
        paths: resolved_paths,
        source_ranges: ranges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_bibliography_path() {
        use serde::Deserialize;
        use serde_saphyr::Spanned;

        #[derive(Deserialize)]
        struct Test {
            bibliography: Spanned<StringOrArray>,
        }

        let yaml = r#"
bibliography: refs.bib
"#;
        let test: Test = serde_saphyr::from_str(yaml).unwrap();

        let info = extract_bibliography_info(
            test.bibliography,
            TextSize::from(0),
            Path::new("/home/user/doc.qmd"),
        )
        .unwrap();

        assert_eq!(info.paths.len(), 1);
        assert_eq!(info.paths[0], PathBuf::from("/home/user/refs.bib"));
        assert_eq!(info.source_ranges.len(), 1);
    }

    #[test]
    fn test_multiple_bibliography_paths() {
        use serde::Deserialize;
        use serde_saphyr::Spanned;

        #[derive(Deserialize)]
        struct Test {
            bibliography: Spanned<StringOrArray>,
        }

        let yaml = r#"
bibliography: 
  - refs.bib
  - other.bib
"#;
        let test: Test = serde_saphyr::from_str(yaml).unwrap();

        let info = extract_bibliography_info(
            test.bibliography,
            TextSize::from(0),
            Path::new("/home/user/doc.qmd"),
        )
        .unwrap();

        assert_eq!(info.paths.len(), 2);
        assert_eq!(info.paths[0], PathBuf::from("/home/user/refs.bib"));
        assert_eq!(info.paths[1], PathBuf::from("/home/user/other.bib"));
    }

    #[test]
    fn test_relative_path_resolution() {
        use serde::Deserialize;
        use serde_saphyr::Spanned;

        #[derive(Deserialize)]
        struct Test {
            bibliography: Spanned<StringOrArray>,
        }

        let yaml = r#"
bibliography: ../refs/refs.bib
"#;
        let test: Test = serde_saphyr::from_str(yaml).unwrap();

        let info = extract_bibliography_info(
            test.bibliography,
            TextSize::from(0),
            Path::new("/home/user/docs/doc.qmd"),
        )
        .unwrap();

        assert_eq!(info.paths.len(), 1);
        assert_eq!(
            info.paths[0],
            PathBuf::from("/home/user/docs/../refs/refs.bib")
        );
    }
}
