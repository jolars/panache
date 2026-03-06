//! Bibliography parsing for citation integrations.
//!
//! This module provides AST-based parsing for bibliography files:
//! - BibTeX (.bib) - Full parser with typed structures
//! - CSL JSON (.json) - Key extraction only
//! - CSL YAML (.yaml, .yml) - Key extraction only
//! - RIS (.ris) - Full parser with validation
//!
//! Note: We use AST parsing (not CST) because bibliography files are external
//! data files, not documents being edited/formatted in the LSP.

mod bibtex;
mod csl_json;
mod csl_yaml;
mod index;
mod ris;

pub use bibtex::{BibDatabase, BibEntry, BibError, BibField, Span, parse_bibtex};
pub use csl_json::parse_csl_json_entries;
pub use csl_yaml::parse_csl_yaml_entries;
pub use index::{
    BibDuplicate, BibEntryLocation, BibFile, BibIndex, BibLoadError, load_bibliography,
};
pub use ris::{parse_ris_entries, validate_ris};
