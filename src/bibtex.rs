//! BibTeX parsing for citation integrations.

mod csl_json;
mod csl_yaml;
mod cst;
mod index;
mod parser;
mod ris;

pub(crate) use csl_json::parse_csl_json_entries;
pub(crate) use csl_yaml::parse_csl_yaml_entries;
pub use cst::{BibTexLanguage, BibTexNode, BibTexSyntaxKind, parse_bibtex_cst};
pub use index::{
    BibDuplicate, BibEntryLocation, BibFile, BibIndex, BibLoadError, load_bibliography,
};
pub use parser::{BibDatabase, BibEntry, BibError, BibField, Span, parse_bibtex};
pub(crate) use ris::{parse_ris_entries, validate_ris};
