use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rowan::{TextRange, TextSize};
use serde::{Deserialize, Serialize};
use serde_saphyr::Spanned;

use crate::bib::BibEntry;

#[derive(Debug, Clone)]
pub struct InlineReference {
    pub id: String,
    pub range: TextRange,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReferenceEntry {
    pub id: Option<Spanned<String>>,
}

pub(crate) fn extract_inline_references(
    entries: Vec<ReferenceEntry>,
    yaml_offset: TextSize,
    doc_path: &Path,
) -> Vec<InlineReference> {
    let mut inline = Vec::new();

    for entry in entries {
        let Some(id) = entry.id else {
            continue;
        };
        let span = id.referenced.span();
        let yaml_byte_offset = span.byte_offset().unwrap_or(span.offset());
        let yaml_byte_len = span.byte_len().unwrap_or(span.len());
        let start = yaml_offset + TextSize::from(yaml_byte_offset as u32);
        let end = start + TextSize::from(yaml_byte_len as u32);
        inline.push(InlineReference {
            id: id.value,
            range: TextRange::new(start, end),
            path: doc_path.to_path_buf(),
        });
    }

    inline
}

#[derive(Debug, Clone)]
pub struct InlineReferenceDuplicate {
    pub key: String,
    pub first: InlineReference,
    pub duplicate: InlineReference,
}

#[derive(Debug, Clone)]
pub struct InlineBibConflict {
    pub key: String,
    pub inline: InlineReference,
    pub bib: BibEntry,
}

pub fn inline_reference_map(inline: &[InlineReference]) -> HashMap<String, Vec<InlineReference>> {
    let mut map: HashMap<String, Vec<InlineReference>> = HashMap::new();
    for entry in inline {
        map.entry(entry.id.to_lowercase())
            .or_default()
            .push(entry.clone());
    }
    map
}

pub fn inline_reference_duplicates(inline: &[InlineReference]) -> Vec<InlineReferenceDuplicate> {
    let mut duplicates = Vec::new();
    let map = inline_reference_map(inline);
    for (key, entries) in map {
        if entries.len() <= 1 {
            continue;
        }
        let first = entries[0].clone();
        for duplicate in entries.iter().skip(1).cloned() {
            duplicates.push(InlineReferenceDuplicate {
                key: key.clone(),
                first: first.clone(),
                duplicate,
            });
        }
    }
    duplicates
}

pub fn inline_bib_conflicts(
    inline: &[InlineReference],
    index: &crate::bib::BibIndex,
) -> Vec<InlineBibConflict> {
    let mut conflicts = Vec::new();
    for entry in inline {
        if let Some(bib) = index.get(&entry.id) {
            conflicts.push(InlineBibConflict {
                key: entry.id.clone(),
                inline: entry.clone(),
                bib: bib.clone(),
            });
        }
    }
    conflicts
}

pub fn inline_reference_contains(inline: &[InlineReference], key: &str) -> bool {
    let needle = key.to_lowercase();
    inline.iter().any(|entry| entry.id.to_lowercase() == needle)
}
