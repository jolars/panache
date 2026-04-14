use std::collections::HashSet;

use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::{AstNode, FootnoteReference, Link, SyntaxKind, SyntaxNode};
use crate::utils::normalize_label;

pub struct UnusedDefinitionsRule;

impl Rule for UnusedDefinitionsRule {
    fn name(&self) -> &str {
        "unused-definitions"
    }

    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        _metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let db = crate::salsa::SalsaDb::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, tree, &config.extensions);
        let mut diagnostics = Vec::new();

        let used_reference_labels: HashSet<String> = tree
            .descendants()
            .filter_map(Link::cast)
            .filter_map(|link| {
                if link
                    .syntax()
                    .ancestors()
                    .any(|ancestor| ancestor.kind() == SyntaxKind::REFERENCE_DEFINITION)
                {
                    return None;
                }
                if link.dest().is_some() {
                    return None;
                }
                if let Some(link_ref) = link.reference() {
                    let label = normalize_label(&link_ref.label());
                    if !label.is_empty() {
                        return Some(label);
                    }
                }
                link.text()
                    .map(|text| normalize_label(&text.text_content()))
            })
            .filter(|label| !label.is_empty())
            .collect();

        for (label, ranges) in index.reference_definition_entries() {
            if used_reference_labels.contains(label) {
                continue;
            }
            for range in ranges {
                diagnostics.push(Diagnostic::warning(
                    Location::from_range(*range, input),
                    "unused-definition-label",
                    format!("Reference definition '[{}]' is never used", label),
                ));
            }
        }

        let used_footnote_ids: HashSet<String> = tree
            .descendants()
            .filter_map(FootnoteReference::cast)
            .map(|footnote| normalize_label(&footnote.id()))
            .filter(|id| !id.is_empty())
            .collect();

        for (id, ranges) in index.footnote_definition_entries() {
            if used_footnote_ids.contains(id) {
                continue;
            }
            for range in ranges {
                diagnostics.push(Diagnostic::warning(
                    Location::from_range(*range, input),
                    "unused-footnote-id",
                    format!("Footnote '[^{}]' is never used", id),
                ));
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UnusedDefinitionsRule;
        rule.check(&tree, input, &config, None)
    }

    #[test]
    fn reports_unused_reference_definition() {
        let input =
            "[used]: https://example.com\n[unused]: https://example.org\n\nSee [x][used].\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-definition-label");
        assert!(diagnostics[0].message.contains("[unused]"));
    }

    #[test]
    fn reports_unused_footnote_definition() {
        let input = "Text with footnote[^1].\n\n[^1]: Used.\n[^2]: Unused.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-footnote-id");
        assert!(diagnostics[0].message.contains("[^2]"));
    }

    #[test]
    fn accepts_used_shortcut_reference_definition() {
        let input = "See [Label].\n\n[Label]: https://example.com\n";
        let diagnostics = parse_and_lint(input);
        assert!(diagnostics.is_empty());
    }
}
