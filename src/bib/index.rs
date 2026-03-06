//! Indexing and loading bibliography files.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::bib::{BibError, ParsedEntry, Span, validate_ris};

/// Format of a bibliography file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BibFormat {
    BibTeX,
    CslJson,
    CslYaml,
    Ris,
}

/// Unified bibliography entry supporting all formats.
///
/// This structure provides a common representation for citations across
/// BibTeX, CSL-JSON, CSL-YAML, and RIS formats, enabling consistent LSP
/// features (hover, completion, rename) regardless of source format.
#[derive(Debug, Clone)]
pub struct BibEntry {
    /// Citation key (e.g., "smith2020").
    pub key: String,
    /// Entry type (e.g., "article", "book"). Optional for some formats.
    pub entry_type: Option<String>,
    /// All fields as key-value pairs (author, title, year, doi, etc.).
    pub fields: HashMap<String, String>,
    /// Path to the source bibliography file.
    pub source_file: PathBuf,
    /// Span of the citation key in the source file.
    pub span: Span,
    /// Original format of this entry.
    pub format: BibFormat,
}

#[derive(Debug, Clone)]
pub struct BibEntryLocation {
    pub key: String,
    pub file: PathBuf,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct BibDuplicate {
    pub key: String,
    pub first: BibEntryLocation,
    pub duplicate: BibEntryLocation,
}

/// Index of all bibliography entries across multiple files.
///
/// This structure provides efficient access to citations with O(1) lookup
/// by citation key. All entries are unified into a common representation
/// regardless of source format (BibTeX, CSL-JSON, CSL-YAML, RIS).
#[derive(Debug, Clone)]
pub struct BibIndex {
    /// All entries, keyed by lowercase citation key for case-insensitive lookup.
    pub entries: HashMap<String, BibEntry>,
    /// Duplicate citations detected across files.
    pub duplicates: Vec<BibDuplicate>,
    /// Parse errors from bibliography files.
    pub errors: Vec<BibError>,
    /// File load errors (I/O failures, etc.).
    pub load_errors: Vec<BibLoadError>,
}

#[derive(Debug, Clone)]
pub struct BibLoadError {
    pub path: PathBuf,
    pub message: String,
}

pub fn load_bibliography(paths: &[PathBuf]) -> BibIndex {
    let mut entries: HashMap<String, BibEntry> = HashMap::new();
    let mut duplicates = Vec::new();
    let mut errors = Vec::new();
    let mut load_errors = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    for path in paths {
        if !seen_paths.insert(path.clone()) {
            continue;
        }
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) => {
                load_errors.push(BibLoadError {
                    path: path.clone(),
                    message: err.to_string(),
                });
                continue;
            }
        };

        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

        // Determine format and parse accordingly
        let (format, parsed_result, parse_errors): (
            BibFormat,
            Result<Vec<ParsedEntry>, String>,
            Vec<BibError>,
        ) = match extension {
            "json" => {
                use crate::bib::parse_csl_json_full;
                (BibFormat::CslJson, parse_csl_json_full(&text), Vec::new())
            }
            "yaml" | "yml" => {
                use crate::bib::parse_csl_yaml_full;
                (BibFormat::CslYaml, parse_csl_yaml_full(&text), Vec::new())
            }
            "ris" => {
                use crate::bib::parse_ris_full;
                // Validate first
                if let Err(message) = validate_ris(&text) {
                    errors.push(BibError {
                        message: message.clone(),
                        span: None,
                    });
                    continue;
                }
                (BibFormat::Ris, parse_ris_full(&text), Vec::new())
            }
            _ => {
                // BibTeX
                use crate::bib::parse_bibtex_full;
                let (entries, parse_errors) = parse_bibtex_full(&text);
                (BibFormat::BibTeX, Ok(entries), parse_errors)
            }
        };

        // Add any parse errors
        errors.extend(parse_errors);

        // Handle all formats uniformly
        match parsed_result {
            Ok(parsed_entries) => {
                for (key, entry_type, entry_fields, span) in parsed_entries {
                    let key_lower = key.to_lowercase();

                    let unified_entry = BibEntry {
                        key: key.clone(),
                        entry_type,
                        fields: entry_fields,
                        source_file: path.clone(),
                        span,
                        format,
                    };

                    if let Some(existing) = entries.get(&key_lower) {
                        duplicates.push(BibDuplicate {
                            key: key.clone(),
                            first: BibEntryLocation {
                                key: existing.key.clone(),
                                file: existing.source_file.clone(),
                                span: existing.span,
                            },
                            duplicate: BibEntryLocation {
                                key: key.clone(),
                                file: path.clone(),
                                span,
                            },
                        });
                    } else {
                        entries.insert(key_lower, unified_entry);
                    }
                }
            }
            Err(message) => {
                errors.push(BibError {
                    message,
                    span: None,
                });
            }
        }
    }

    BibIndex {
        entries,
        duplicates,
        errors,
        load_errors,
    }
}

impl BibIndex {
    /// Get a unified entry by citation key (case-insensitive).
    pub fn get(&self, key: &str) -> Option<&BibEntry> {
        self.entries.get(&key.to_lowercase())
    }

    /// Iterate over all citation keys.
    pub fn iter_keys(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    /// Iterate over all unified entries.
    pub fn entries(&self) -> impl Iterator<Item = &BibEntry> {
        self.entries.values()
    }

    /// Get entry location (legacy API).
    pub fn get_location(&self, key: &str) -> Option<BibEntryLocation> {
        self.entries
            .get(&key.to_lowercase())
            .map(|entry| BibEntryLocation {
                key: entry.key.clone(),
                file: entry.source_file.clone(),
                span: entry.span,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_bibliography_dedupes_paths() {
        let temp_dir = TempDir::new().unwrap();
        let bib_path = temp_dir.path().join("refs.bib");
        std::fs::write(&bib_path, "@book{Test,}\n").unwrap();

        let index = load_bibliography(&[bib_path.clone(), bib_path]);
        assert!(index.duplicates.is_empty());
        assert_eq!(index.entries.len(), 1);
    }
}
