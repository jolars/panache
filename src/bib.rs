//! BibTeX parsing for citation integrations.

mod bibtex;
mod csl_json;
mod csl_yaml;
mod cst;
mod index;
mod ris;

pub use bibtex::{BibDatabase, BibEntry, BibError, BibField, Span, parse_bibtex};
pub(crate) use csl_json::parse_csl_json_entries;
pub(crate) use csl_yaml::parse_csl_yaml_entries;
pub use cst::{BibTexLanguage, BibTexNode, BibTexSyntaxKind, parse_bibtex_cst};
pub use index::{
    BibDuplicate, BibEntryLocation, BibFile, BibIndex, BibLoadError, load_bibliography,
};
pub(crate) use ris::{parse_ris_entries, validate_ris};
