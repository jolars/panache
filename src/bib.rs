//! Bibliography parsing for citation integrations.
//!
//! This module provides unified parsing for bibliography files:
//! - BibTeX (.bib) - Full field extraction with typed structures
//! - CSL JSON (.json) - Full field extraction via serde
//! - CSL YAML (.yaml, .yml) - Full field extraction via serde
//! - RIS (.ris) - Full field extraction with validation
//!
//! All formats are unified into `BibEntry` for consistent LSP features.

use std::collections::HashMap;

mod bibtex;
mod csl_json;
mod csl_yaml;
mod index;
mod ris;

pub use bibtex::{BibtexEntry, parse_bibtex, parse_bibtex_full};
pub use csl_json::{parse_csl_json_entries, parse_csl_json_full};
pub use csl_yaml::{parse_csl_yaml_entries, parse_csl_yaml_full};
pub use index::{
    BibDuplicate, BibEntry, BibEntryLocation, BibFormat, BibIndex, BibLoadError, load_bibliography,
};
pub use ris::{parse_ris_entries, parse_ris_full, validate_ris};

/// Parsed entry data: (id, entry_type, fields, span).
///
/// This is the intermediate representation used by all parsers before conversion to `BibEntry`.
pub(crate) type ParsedEntry = (String, Option<String>, HashMap<String, String>, Span);

#[derive(Debug, Clone)]
pub struct BibError {
    pub message: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Default, Clone)]
pub struct BibtexDatabase {
    pub entries: Vec<BibtexEntry>,
    pub entry_index: HashMap<String, usize>,
    pub errors: Vec<BibError>,
}
