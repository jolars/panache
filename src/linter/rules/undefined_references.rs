use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::{AstNode, Crossref, FootnoteReference, Link, SyntaxNode};
use crate::utils::normalize_label;
use std::collections::HashSet;

pub struct UndefinedReferencesRule;

impl Rule for UndefinedReferencesRule {
    fn name(&self) -> &str {
        "undefined-references"
    }

    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        _metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let db = crate::salsa::SalsaDb::default();
        let symbol_index = crate::salsa::symbol_usage_index_from_tree(&db, tree);
        let mut reference_labels: HashSet<String> = symbol_index
            .reference_definition_entries()
            .map(|(label, _)| label.clone())
            .filter(|label| !label.is_empty())
            .collect();
        reference_labels.extend(
            symbol_index
                .crossref_declaration_entries()
                .map(|(label, _)| label.clone())
                .filter(|label| !label.is_empty()),
        );

        if config.extensions.implicit_header_references && config.extensions.auto_identifiers {
            reference_labels.extend(
                symbol_index
                    .heading_label_entries()
                    .map(|(label, _)| label.clone())
                    .filter(|label| !label.is_empty()),
            );
        }

        let footnote_ids: HashSet<String> = symbol_index
            .footnote_definition_entries()
            .map(|(id, _)| id.clone())
            .filter(|id| !id.is_empty())
            .collect();

        for link in tree.descendants().filter_map(Link::cast) {
            if link.dest().is_some() {
                continue;
            }

            let Some((label_text, location_node)) = extract_reference_label_and_node(&link) else {
                continue;
            };
            let normalized_label = normalize_label(&label_text);
            if normalized_label.is_empty() || reference_labels.contains(&normalized_label) {
                continue;
            }

            diagnostics.push(Diagnostic::warning(
                Location::from_node(&location_node, input),
                "undefined-reference-label",
                format!("Reference label '[{}]' not found", label_text),
            ));
        }

        for footnote_ref in tree.descendants().filter_map(FootnoteReference::cast) {
            let id = footnote_ref.id();
            let normalized = normalize_label(&id);
            if normalized.is_empty() || footnote_ids.contains(&normalized) {
                continue;
            }

            diagnostics.push(Diagnostic::warning(
                Location::from_node(footnote_ref.syntax(), input),
                "undefined-footnote-id",
                format!("Footnote '[^{}]' not found", id),
            ));
        }

        for crossref in tree.descendants().filter_map(Crossref::cast) {
            for key in crossref.keys() {
                let label = key.text();
                let normalized = normalize_label(&label);
                if normalized.is_empty() || reference_labels.contains(&normalized) {
                    continue;
                }
                diagnostics.push(Diagnostic::warning(
                    Location::from_range(key.text_range(), input),
                    "undefined-reference-label",
                    format!("Cross-reference label '@{}' not found", label),
                ));
            }
        }

        diagnostics
    }
}

fn extract_reference_label_and_node(link: &Link) -> Option<(String, SyntaxNode)> {
    if let Some(link_ref) = link.reference() {
        let label = link_ref.label();
        if !label.trim().is_empty() {
            return Some((label, link_ref.syntax().clone()));
        }
    }

    link.text()
        .map(|text| (text.text_content(), link.syntax().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Flavor;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        rule.check(&tree, input, &config, None)
    }

    #[test]
    fn reports_missing_reference_labels() {
        let input = "Text with [link][missing].\n\n[ok]: https://example.com\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-reference-label");
        assert!(diagnostics[0].message.contains("[missing]"));
    }

    #[test]
    fn reports_missing_footnotes() {
        let input = "Text with footnote[^missing].\n\n[^ok]: Defined.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-footnote-id");
        assert!(diagnostics[0].message.contains("[^missing]"));
    }

    #[test]
    fn accepts_collapsed_and_shortcut_reference_links() {
        let input = "Collapsed [GitHub][] and shortcut [Wiki].\n\n[GitHub]: https://github.com\n[Wiki]: https://wikipedia.org\n";
        let diagnostics = parse_and_lint(input);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn accepts_implicit_heading_references() {
        let input = "# Heading Name\n\nSee [Heading Name].\n";
        let diagnostics = parse_and_lint(input);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn implicit_heading_references_require_auto_identifiers() {
        let input = "# Heading Name\n\nSee [Heading Name].\n";
        let mut config = Config::default();
        config.extensions.implicit_header_references = true;
        config.extensions.auto_identifiers = false;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check(&tree, input, &config, None);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-reference-label");
    }

    #[test]
    fn accepts_quarto_crossref_to_chunk_label() {
        let input = "See @fig-plot.\n\n```{r}\n#| label: fig-plot\nplot(1:10)\n```\n";
        let mut config = Config {
            flavor: Flavor::Quarto,
            ..Default::default()
        };
        config.extensions.quarto_crossrefs = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn reports_missing_quarto_crossref_label() {
        let input = "See @fig-missing.\n";
        let mut config = Config {
            flavor: Flavor::Quarto,
            ..Default::default()
        };
        config.extensions.quarto_crossrefs = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check(&tree, input, &config, None);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-reference-label");
        assert!(diagnostics[0].message.contains("@fig-missing"));
    }
}
