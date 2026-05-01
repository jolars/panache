//! Loader for the pandoc-conformance corpus.
//!
//! Each case lives in
//! `tests/fixtures/pandoc-conformance/corpus/<NNNN>-<section>-<slug>/`
//! and contains:
//!   - `input.md`         markdown snippet fed to panache
//!   - `expected.native`  pinned output of `pandoc -f markdown -t native`
//!
//! Numbering convention: 4-digit zero-padded prefix, the next dash-segment is
//! the section group (e.g. `inline`, `block`), the rest is a free-form slug.
//! This zero-ceremony layout means we can derive `id` and `section` from the
//! directory name alone — no extra metadata files.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PandocCase {
    pub id: u32,
    pub slug: String,
    pub section: String,
    pub markdown: String,
    pub expected_native: String,
}

pub fn read_corpus(corpus_dir: &Path) -> Vec<PandocCase> {
    let mut entries: Vec<PathBuf> = fs::read_dir(corpus_dir)
        .unwrap_or_else(|e| panic!("failed to read corpus dir {}: {e}", corpus_dir.display()))
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    entries.sort();

    let mut out = Vec::with_capacity(entries.len());
    for path in entries {
        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_else(|| panic!("non-utf8 corpus dir name at {}", path.display()))
            .to_string();

        let (id_str, rest) = dir_name.split_once('-').unwrap_or_else(|| {
            panic!("corpus dir {dir_name:?} must follow `<NNNN>-<section>-<slug>` format")
        });
        let id: u32 = id_str
            .parse()
            .unwrap_or_else(|_| panic!("corpus dir {dir_name:?} prefix is not a u32: {id_str:?}"));

        let (section, slug_tail) = rest.split_once('-').unwrap_or_else(|| {
            panic!(
                "corpus dir {dir_name:?} must follow `<NNNN>-<section>-<slug>` format \
                 (missing section)"
            )
        });

        let input_path = path.join("input.md");
        let expected_path = path.join("expected.native");
        let markdown = fs::read_to_string(&input_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", input_path.display()));
        let expected_native = fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", expected_path.display()));

        out.push(PandocCase {
            id,
            slug: format!("{section}-{slug_tail}"),
            section: section.to_string(),
            markdown,
            expected_native,
        });
    }
    out
}
