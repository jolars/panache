use rowan::TextRange;

use crate::lsp::context::OpenDocumentContext;
use crate::lsp::global_state::StateSnapshot;
use crate::syntax::{AstNode, ImageLink, Link, ReferenceDefinition, SyntaxNode};
use crate::utils::{normalize_anchor_label, normalize_label};

use super::helpers;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SymbolTarget {
    Citation(String),
    Crossref(String),
    ChunkLabel(String),
    ExampleLabel(String),
    HeadingLink(String),
    HeadingId(String),
    Reference { label: String, is_footnote: bool },
}

pub(crate) fn resolve_symbol_target_at_offset(
    root: &SyntaxNode,
    offset: usize,
) -> Option<SymbolTarget> {
    if let Some((label, is_footnote)) = helpers::extract_definition_target_at_offset(root, offset) {
        return Some(SymbolTarget::Reference { label, is_footnote });
    }

    if let Some(key) = helpers::extract_example_label_target_at_offset(root, offset) {
        return Some(SymbolTarget::ExampleLabel(key));
    }

    if let Some(label) = helpers::extract_bookdown_definition_target_at_offset(root, offset) {
        return Some(SymbolTarget::Crossref(label));
    }

    let mut node = helpers::find_node_at_offset(root, offset)?;

    loop {
        if let Some(key) = helpers::extract_citation_key(&node) {
            return Some(SymbolTarget::Citation(key));
        }

        if let Some(key) = helpers::extract_crossref_key(&node) {
            return Some(SymbolTarget::Crossref(key));
        }

        if let Some(key) = helpers::extract_chunk_label_key(&node) {
            return Some(SymbolTarget::ChunkLabel(key));
        }

        if let Some(key) = helpers::extract_heading_id_key(&node) {
            return Some(SymbolTarget::HeadingId(key));
        }

        if let Some(key) = helpers::extract_attribute_id_key(&node) {
            return Some(SymbolTarget::Crossref(key));
        }

        if let Some(key) = helpers::extract_heading_link_target(&node) {
            return Some(SymbolTarget::HeadingLink(key));
        }

        if let Some((label, is_footnote)) = helpers::extract_reference_target(&node) {
            return Some(SymbolTarget::Reference { label, is_footnote });
        }

        node = node.parent()?;
    }
}

/// Gather every same-document value-span for `target`.
///
/// Everything except link references routes through the per-document salsa
/// [`SymbolUsageIndex`](crate::salsa::SymbolUsageIndex) accessors that
/// `rename`/`references` already use; link references are walked from the CST
/// because the index tracks only their definitions (as full-node ranges) and
/// none of their usages.
///
/// Shared by `linked_editing_range` (which then filters to identical source
/// text) and `document_highlight` (which highlights the full set as-is).
pub(crate) fn collect_symbol_ranges(
    snap: &StateSnapshot,
    ctx: &OpenDocumentContext,
    config: &crate::Config,
    root: &SyntaxNode,
    target: &SymbolTarget,
) -> Vec<TextRange> {
    let index = {
        let db = snap.db();
        crate::salsa::symbol_usage_index(db, ctx.salsa_file, ctx.salsa_config).clone()
    };

    let mut ranges: Vec<TextRange> = Vec::new();
    match target {
        SymbolTarget::Citation(key) => {
            if let Some(rs) = index.citation_usages(key) {
                ranges.extend(rs.iter().copied());
            }
        }
        SymbolTarget::Crossref(label) | SymbolTarget::ChunkLabel(label) => {
            let candidates = crate::utils::crossref_symbol_labels(
                &normalize_anchor_label(label),
                config.extensions.bookdown_references,
            );
            for candidate in &candidates {
                if let Some(rs) = index.crossref_usages(candidate) {
                    ranges.extend(rs.iter().copied());
                }
                if let Some(rs) = index.crossref_declaration_value_ranges(candidate) {
                    ranges.extend(rs.iter().copied());
                }
                if let Some(rs) = index.chunk_label_value_ranges(candidate) {
                    ranges.extend(rs.iter().copied());
                }
            }
        }
        SymbolTarget::ExampleLabel(label) => {
            if let Some(rs) = index.example_label_usages(label) {
                ranges.extend(rs.iter().copied());
            }
            if let Some(rs) = index.example_label_definitions(label) {
                ranges.extend(rs.iter().copied());
            }
        }
        SymbolTarget::HeadingId(label) | SymbolTarget::HeadingLink(label) => {
            ranges.extend(index.heading_rename_ranges(label));
        }
        SymbolTarget::Reference {
            label,
            is_footnote: true,
        } => {
            ranges.extend(index.footnote_rename_ranges(label));
        }
        SymbolTarget::Reference {
            label,
            is_footnote: false,
        } => {
            ranges.extend(collect_reference_link_ranges(root, label));
        }
    }
    ranges
}

/// Collect the label value-spans for a link reference: the definition
/// (`[label]: url`) plus full-form usages (`[text][label]`, `![alt][label]`).
/// Shortcut (`[label]`) and collapsed (`[label][]`) forms are classified as
/// implicit heading links by [`resolve_symbol_target_at_offset`] and handled
/// through the heading branch instead.
fn collect_reference_link_ranges(root: &SyntaxNode, label: &str) -> Vec<TextRange> {
    let norm = normalize_label(label);
    if norm.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for node in root.descendants() {
        if let Some(def) = ReferenceDefinition::cast(node.clone()) {
            if normalize_label(&def.label()) == norm
                && let Some(range) = def.label_value_range()
            {
                out.push(range);
            }
        } else if let Some(link) = Link::cast(node.clone()) {
            if let Some(reference) = link.reference()
                && normalize_label(&reference.label()) == norm
                && let Some(range) = reference.label_value_range()
            {
                out.push(range);
            }
        } else if let Some(image) = ImageLink::cast(node.clone())
            && let Some(reference) = image.reference()
            && normalize_label(&reference.label()) == norm
            && let Some(range) = reference.label_value_range()
        {
            out.push(range);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{SymbolTarget, resolve_symbol_target_at_offset};

    #[test]
    fn resolves_citation_target() {
        let input = "See @doe2020.";
        let root = crate::parse(input, None);
        let offset = input.find("doe2020").unwrap();
        let target = resolve_symbol_target_at_offset(&root, offset);
        assert_eq!(target, Some(SymbolTarget::Citation("doe2020".to_string())));
    }

    #[test]
    fn resolves_bookdown_crossref_with_hyphen() {
        let input = "# Heading 2\n\nSee \\@ref(heading-2).\n";
        let mut config = crate::config::Config {
            flavor: crate::config::Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let root = crate::parse(input, Some(config));
        let offset = input.find("heading-2").unwrap();
        let target = resolve_symbol_target_at_offset(&root, offset);
        assert_eq!(
            target,
            Some(SymbolTarget::Crossref("heading-2".to_string()))
        );
    }

    #[test]
    fn resolves_heading_link_target() {
        let input = "# Heading {#heading}\n\nSee [text](#heading).\n";
        let root = crate::parse(input, None);
        let offset = input.rfind("#heading").unwrap() + 1;
        let target = resolve_symbol_target_at_offset(&root, offset);
        assert_eq!(
            target,
            Some(SymbolTarget::HeadingLink("heading".to_string()))
        );
    }

    #[test]
    fn resolves_chunk_label_target_from_hashpipe_label_value() {
        let input = "```{r}\n#| label: fig-plot\nplot(1:10)\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let root = crate::parse(input, Some(config));
        let offset = input.find("fig-plot").unwrap();
        let target = resolve_symbol_target_at_offset(&root, offset);
        assert_eq!(
            target,
            Some(SymbolTarget::ChunkLabel("fig-plot".to_string()))
        );
    }

    #[test]
    fn resolves_example_label_target() {
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Pandoc,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Pandoc),
            ..Default::default()
        };
        let input = "(@good) First example.\n\nAs (@good) shows.\n";
        let root = crate::parse(input, Some(config));
        let offset = input.rfind("good").unwrap();
        let target = resolve_symbol_target_at_offset(&root, offset);
        assert_eq!(target, Some(SymbolTarget::ExampleLabel("good".to_string())));
    }
}
