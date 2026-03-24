use crate::syntax::SyntaxNode;

use super::helpers;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SymbolTarget {
    Citation(String),
    Crossref(String),
    ChunkLabel(String),
    HeadingLink(String),
    HeadingId(String),
    Reference { label: String, is_footnote: bool },
}

pub(crate) fn resolve_symbol_target_at_offset(
    root: &SyntaxNode,
    offset: usize,
) -> Option<SymbolTarget> {
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
}
