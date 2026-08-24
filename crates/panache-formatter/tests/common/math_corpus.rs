//! Shared `.tex` discovery for the math corpus test suites.
//!
//! Single source of truth for walking `tests/fixtures/math_corpus`, consumed
//! via `#[path]` by the formatter corpus tests and by the parser crate's
//! Badness parity oracle, so the suites can never drift onto different case
//! sets.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

/// Every `.tex` case under `root`, sorted. Panics on an unreadable directory
/// so a corpus reorganization can never silently shrink a suite's case set.
pub fn discover_cases(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, cases: &mut Vec<PathBuf>) {
        let entries = fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()));
        for entry in entries {
            let path = entry.expect("failed to read corpus entry").path();
            if path.is_dir() {
                walk(&path, cases);
            } else if path.extension() == Some(OsStr::new("tex")) {
                cases.push(path);
            }
        }
    }

    let mut cases = Vec::new();
    walk(root, &mut cases);
    cases.sort();
    cases
}
