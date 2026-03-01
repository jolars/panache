//! BibTeX parsing for citation integrations.

mod cst;
mod index;
mod parser;

pub use cst::{BibTexLanguage, BibTexNode, BibTexSyntaxKind, parse_bibtex_cst};
pub use index::{
    BibDuplicate, BibEntryLocation, BibFile, BibIndex, BibLoadError, load_bibliography,
};
pub use parser::{BibDatabase, BibEntry, BibError, BibField, Span, parse_bibtex};
