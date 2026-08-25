//! Shared `.tex` discovery for the math corpus test suites.
//!
//! Single source of truth for walking `tests/fixtures/math_corpus`, consumed
//! via `#[path]` by the formatter corpus tests and by the parser crate's
//! Badness parity oracle, so the suites can never drift onto different case
//! sets.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use panache_parser::semantic::math::SignatureScope;

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

/// Read the optional document preamble paired with a corpus body.
///
/// `case.tex` uses `case.preamble`; the non-TeX extension keeps metadata out of
/// recursive corpus discovery.
#[allow(dead_code)]
pub fn read_preamble(case: &Path) -> io::Result<Option<String>> {
    let path = case.with_extension("preamble");
    match fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Build the same document signature overlay used by the production formatter.
#[allow(dead_code)]
pub fn signature_scope(preamble: Option<&str>) -> SignatureScope {
    preamble.map_or_else(SignatureScope::default, |source| {
        let root = panache_parser::parse(source, None);
        SignatureScope::from_root(&root)
    })
}
