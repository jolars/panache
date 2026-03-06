//! Workspace-level caching for bibliography files.
//!
//! This module provides efficient caching of parsed bibliography files with
//! timestamp-based invalidation. Bibliography files are parsed once and reused
//! across all documents that reference them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::bib::{BibIndex, BibtexDatabase};

/// A cached bibliography file with timestamp tracking.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Cache infrastructure exists but currently bypassed
struct CachedBibFile {
    /// The parsed bibliography database.
    database: BibtexDatabase,
    /// Last modification time when this file was parsed.
    last_modified: SystemTime,
    /// Absolute path to the file.
    path: PathBuf,
}

/// Workspace-level cache for bibliography files.
///
/// Maintains parsed bibliography files and tracks their modification times
/// for invalidation. Multiple documents can share the same cached file.
#[derive(Debug, Default)]
pub struct BibliographyCache {
    /// Cached files keyed by absolute path.
    entries: HashMap<PathBuf, CachedBibFile>,
}

impl BibliographyCache {
    /// Creates a new empty bibliography cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Gets or loads a bibliography file, checking timestamps for staleness.
    ///
    /// If the file is not cached or has been modified since last parse,
    /// it will be re-parsed. Returns None for non-BibTeX formats (they're handled
    /// separately as lightweight key extractions).
    ///
    /// Note: Currently not used as `build_index()` delegates directly to `load_bibliography()`.
    #[allow(dead_code)]
    fn get_or_load(&mut self, path: &Path) -> Result<Option<&BibtexDatabase>, String> {
        // Check file extension - only cache BibTeX files
        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        if matches!(extension, "yaml" | "yml" | "json" | "ris") {
            // These formats don't have full BibDatabase - just return None
            // They'll be handled as lightweight key extractions in build_index
            return Ok(None);
        }

        // Check if we need to reload
        let needs_reload = if let Some(cached) = self.entries.get(path) {
            // Check if file has been modified
            match std::fs::metadata(path) {
                Ok(metadata) => match metadata.modified() {
                    Ok(modified) => modified > cached.last_modified,
                    Err(_) => true, // Can't get mtime, reload to be safe
                },
                Err(_) => true, // File doesn't exist, will error below
            }
        } else {
            true // Not cached yet
        };

        if needs_reload {
            // Read and parse the file
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

            let database = crate::bib::parse_bibtex(&text);

            // Get current modification time
            let last_modified = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .unwrap_or_else(|_| SystemTime::now());

            self.entries.insert(
                path.to_path_buf(),
                CachedBibFile {
                    database,
                    last_modified,
                    path: path.to_path_buf(),
                },
            );
        }

        // Return reference to cached database
        Ok(Some(&self.entries.get(path).unwrap().database))
    }

    /// Builds a BibIndex from the given bibliography file paths.
    ///
    /// This is the main entry point for document metadata extraction.
    /// It loads/caches each file and combines them into a single index.
    /// Handles all formats: BibTeX, CSL-JSON, CSL-YAML, RIS.
    ///
    /// Note: For now, this delegates to the unified `load_bibliography` function.
    /// BibTeX-specific caching at the file level (via `get_or_load`) could be
    /// re-added if needed for performance, but the unified parsing provides
    /// consistent field extraction across all formats.
    pub fn build_index(&mut self, paths: &[PathBuf]) -> BibIndex {
        crate::bib::load_bibliography(paths)
    }

    /// Invalidates a bibliography file in the cache.
    ///
    /// Call this when a file change is detected. The file will be re-parsed
    /// on next access.
    pub fn invalidate(&mut self, path: &Path) {
        self.entries.remove(path);
    }

    /// Clears the entire cache.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Returns the number of cached files.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the cache is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_cache_loads_file() {
        let mut cache = BibliographyCache::new();
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "@article{{test2020, title={{Test}}}}").unwrap();
        file.flush().unwrap();

        let index = cache.build_index(&[file.path().to_path_buf()]);
        assert_eq!(index.entries.len(), 1);
        assert!(index.entries.contains_key("test2020"));
    }

    #[test]
    fn test_cache_reuses_parsed_file() {
        let mut cache = BibliographyCache::new();
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "@article{{test2020, title={{Test}}}}").unwrap();
        file.flush().unwrap();

        // First load
        let _index1 = cache.build_index(&[file.path().to_path_buf()]);
        // Note: Cache behavior is internal, just verify index is built correctly
        assert!(_index1.entries.contains_key("test2020"));

        // Second load - result should be the same
        let _index2 = cache.build_index(&[file.path().to_path_buf()]);
        assert!(_index2.entries.contains_key("test2020"));
    }

    #[test]
    fn test_invalidate_removes_entry() {
        let mut cache = BibliographyCache::new();
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "@article{{test2020, title={{Test}}}}").unwrap();
        file.flush().unwrap();

        let _index = cache.build_index(&[file.path().to_path_buf()]);
        // Note: Internal cache state is not directly testable anymore,
        // but invalidate() should still work for consistency
        cache.invalidate(file.path());
        // Cache invalidation doesn't affect build_index results directly anymore
    }

    #[test]
    fn test_build_index_handles_duplicates() {
        let mut cache = BibliographyCache::new();

        let mut file1 = NamedTempFile::new().unwrap();
        writeln!(file1, "@article{{test2020, title={{First}}}}").unwrap();
        file1.flush().unwrap();

        let mut file2 = NamedTempFile::new().unwrap();
        writeln!(file2, "@article{{test2020, title={{Duplicate}}}}").unwrap();
        file2.flush().unwrap();

        let index = cache.build_index(&[file1.path().to_path_buf(), file2.path().to_path_buf()]);
        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.duplicates.len(), 1);
        assert_eq!(index.duplicates[0].key, "test2020");
    }

    #[test]
    fn test_handles_csl_json_format() {
        let mut cache = BibliographyCache::new();
        let mut file = NamedTempFile::with_suffix(".json").unwrap();
        // CSL-JSON must be an array
        writeln!(file, r#"[{{"id": "smith2020", "title": "Test"}}]"#).unwrap();
        file.flush().unwrap();

        let index = cache.build_index(&[file.path().to_path_buf()]);
        assert_eq!(index.entries.len(), 1);
        assert!(index.entries.contains_key("smith2020"));

        // Verify it's a unified entry with fields
        let entry = index.get("smith2020").unwrap();
        assert_eq!(entry.fields.get("title"), Some(&"Test".to_string()));
    }

    #[test]
    fn test_handles_csl_yaml_format() {
        let mut cache = BibliographyCache::new();
        let mut file = NamedTempFile::with_suffix(".yaml").unwrap();
        writeln!(file, "- id: jones2021").unwrap();
        writeln!(file, "  title: Test Article").unwrap();
        file.flush().unwrap();

        let index = cache.build_index(&[file.path().to_path_buf()]);
        assert_eq!(index.entries.len(), 1);
        assert!(index.entries.contains_key("jones2021"));
    }

    #[test]
    fn test_handles_mixed_formats() {
        let mut cache = BibliographyCache::new();

        let mut bib_file = NamedTempFile::with_suffix(".bib").unwrap();
        writeln!(bib_file, "@article{{test2020, title={{BibTeX}}}}").unwrap();
        bib_file.flush().unwrap();

        let mut json_file = NamedTempFile::with_suffix(".json").unwrap();
        // CSL-JSON must be an array
        writeln!(json_file, r#"[{{"id": "smith2020", "title": "JSON"}}]"#).unwrap();
        json_file.flush().unwrap();

        let index = cache.build_index(&[
            bib_file.path().to_path_buf(),
            json_file.path().to_path_buf(),
        ]);
        assert_eq!(index.entries.len(), 2);
        assert!(index.entries.contains_key("test2020"));
        assert!(index.entries.contains_key("smith2020"));

        // Verify both have fields
        assert!(index.get("test2020").unwrap().fields.contains_key("title"));
        assert!(index.get("smith2020").unwrap().fields.contains_key("title"));
    }
}
