//! Tests for BibTeX index functionality.
//!
//! These tests cover the bibtex/index.rs module to improve coverage.

use panache::bibtex::load_bibliography;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn bib_index_iter_keys() {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(
        file,
        "@article{{key1,\n  title={{Title 1}},\n  author={{Author 1}}\n}}"
    )
    .unwrap();
    writeln!(
        file,
        "@book{{key2,\n  title={{Title 2}},\n  author={{Author 2}}\n}}"
    )
    .unwrap();
    file.flush().unwrap();

    let index = load_bibliography(&[file.path().to_path_buf()]);

    // Test iter_keys()
    let keys: Vec<_> = index.iter_keys().cloned().collect();
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&"key1".to_string()));
    assert!(keys.contains(&"key2".to_string()));
}

#[test]
fn bib_index_entries() {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(
        file,
        "@article{{mykey,\n  title={{My Title}},\n  author={{My Author}}\n}}"
    )
    .unwrap();
    file.flush().unwrap();

    let index = load_bibliography(&[file.path().to_path_buf()]);

    // Test entries() iterator
    let entries: Vec<_> = index.entries().collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "mykey");
}

#[test]
fn bib_index_find_entry() {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(
        file,
        "@article{{testkey,\n  title={{Test Title}},\n  author={{Test Author}},\n  year={{2023}}\n}}"
    )
    .unwrap();
    file.flush().unwrap();

    let index = load_bibliography(&[file.path().to_path_buf()]);

    // Test find_entry() - should find entry by key
    let entry = index.find_entry("testkey");
    assert!(entry.is_some(), "Should find entry by key");

    let entry = entry.unwrap();
    assert_eq!(entry.key, "testkey");
    assert_eq!(entry.entry_type, "article");

    // Check that we can access fields
    assert!(!entry.fields.is_empty());
    let has_title = entry.fields.iter().any(|f| f.name == "title");
    assert!(has_title, "Should have a title field");
}

#[test]
fn bib_index_find_entry_case_insensitive() {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(
        file,
        "@article{{MyKey,\n  title={{Title}},\n  author={{Author}}\n}}"
    )
    .unwrap();
    file.flush().unwrap();

    let index = load_bibliography(&[file.path().to_path_buf()]);

    // Test case-insensitive lookup
    assert!(index.find_entry("mykey").is_some());
    assert!(index.find_entry("MyKey").is_some());
    assert!(index.find_entry("MYKEY").is_some());
}

#[test]
fn bib_index_find_entry_not_found() {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(file, "@article{{key1,\n  title={{Title}}\n}}").unwrap();
    file.flush().unwrap();

    let index = load_bibliography(&[file.path().to_path_buf()]);

    // Test with non-existent key
    let entry = index.find_entry("nonexistent");
    assert!(entry.is_none(), "Should return None for non-existent key");
}

#[test]
fn bib_index_get_case_insensitive() {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(file, "@article{{TestKey,\n  title={{Title}}\n}}").unwrap();
    file.flush().unwrap();

    let index = load_bibliography(&[file.path().to_path_buf()]);

    // Test BibIndex::get() method with different cases
    assert!(index.get("testkey").is_some());
    assert!(index.get("TestKey").is_some());
    assert!(index.get("TESTKEY").is_some());
    assert!(index.get("nonexistent").is_none());
}

#[test]
fn bib_index_load_multiple_files() {
    let mut file1 = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(file1, "@article{{key1,\n  title={{Title 1}}\n}}").unwrap();
    file1.flush().unwrap();

    let mut file2 = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(file2, "@article{{key2,\n  title={{Title 2}}\n}}").unwrap();
    file2.flush().unwrap();

    let index = load_bibliography(&[file1.path().to_path_buf(), file2.path().to_path_buf()]);

    // Should have entries from both files
    assert_eq!(index.entries.len(), 2);
    assert!(index.get("key1").is_some());
    assert!(index.get("key2").is_some());
}

#[test]
fn bib_index_handles_duplicates() {
    let mut file1 = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(file1, "@article{{dupkey,\n  title={{First}}\n}}").unwrap();
    file1.flush().unwrap();

    let mut file2 = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(file2, "@article{{dupkey,\n  title={{Second}}\n}}").unwrap();
    file2.flush().unwrap();

    let index = load_bibliography(&[file1.path().to_path_buf(), file2.path().to_path_buf()]);

    // Should track duplicates
    assert_eq!(index.duplicates.len(), 1);
    assert_eq!(index.duplicates[0].key, "dupkey");

    // First occurrence should be in index
    assert!(index.get("dupkey").is_some());
}

#[test]
fn bib_index_load_error_handling() {
    use std::path::PathBuf;

    // Try to load a non-existent file
    let nonexistent = PathBuf::from("/nonexistent/path/to/file.bib");
    let index = load_bibliography(std::slice::from_ref(&nonexistent));

    // Should record load error
    assert_eq!(index.load_errors.len(), 1);
    assert_eq!(index.load_errors[0].path, nonexistent);
    assert!(!index.load_errors[0].message.is_empty());
}
