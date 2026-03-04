//! Indexing and loading bibliography files.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::bibtex::{BibDatabase, BibEntry, BibError, Span, parse_bibtex};

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

#[derive(Debug, Clone)]
pub struct BibFile {
    pub path: PathBuf,
    pub database: BibDatabase,
}

#[derive(Debug, Clone)]
pub struct BibIndex {
    pub entries: HashMap<String, BibEntryLocation>,
    pub duplicates: Vec<BibDuplicate>,
    pub errors: Vec<BibError>,
    pub files: Vec<BibFile>,
    pub load_errors: Vec<BibLoadError>,
}

#[derive(Debug, Clone)]
pub struct BibLoadError {
    pub path: PathBuf,
    pub message: String,
}

pub fn load_bibliography(paths: &[PathBuf]) -> BibIndex {
    let mut entries: HashMap<String, BibEntryLocation> = HashMap::new();
    let mut duplicates = Vec::new();
    let mut errors = Vec::new();
    let mut files = Vec::new();
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

        let database = parse_bibtex(&text);
        errors.extend(database.errors.clone());

        for entry in &database.entries {
            let key_lower = entry.key.to_lowercase();
            let location = BibEntryLocation {
                key: entry.key.clone(),
                file: path.clone(),
                span: entry.key_span,
            };

            if let Some(existing) = entries.get(&key_lower) {
                duplicates.push(BibDuplicate {
                    key: entry.key.clone(),
                    first: existing.clone(),
                    duplicate: location.clone(),
                });
            } else {
                entries.insert(key_lower, location);
            }
        }

        files.push(BibFile {
            path: path.clone(),
            database,
        });
    }

    BibIndex {
        entries,
        duplicates,
        errors,
        files,
        load_errors,
    }
}

impl BibIndex {
    pub fn get(&self, key: &str) -> Option<&BibEntryLocation> {
        self.entries.get(&key.to_lowercase())
    }

    pub fn iter_keys(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    pub fn entries(&self) -> impl Iterator<Item = &BibEntryLocation> {
        self.entries.values()
    }

    pub fn find_entry(&self, key: &str) -> Option<&BibEntry> {
        for file in &self.files {
            if let Some(index) = file.database.entry_index.get(&key.to_lowercase()) {
                return file.database.entries.get(*index);
            }
        }
        None
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
        assert_eq!(index.files.len(), 1);
    }
}
