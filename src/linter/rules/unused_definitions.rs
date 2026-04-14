use std::collections::HashSet;
use std::path::Path;

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
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let db = crate::salsa::SalsaDb::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, tree, &config.extensions);
        let mut used = collect_usage_labels(tree);

        if let Some(metadata) = metadata {
            extend_usage_labels_from_project(&mut used, metadata, config);
        }

        let mut diagnostics = Vec::new();
        for (label, ranges) in index.reference_definition_entries() {
            if used.reference_labels.contains(label) {
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

        for (id, ranges) in index.footnote_definition_entries() {
            if used.footnote_ids.contains(id) {
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

#[derive(Default)]
struct UsageLabels {
    reference_labels: HashSet<String>,
    footnote_ids: HashSet<String>,
}

fn collect_usage_labels(tree: &SyntaxNode) -> UsageLabels {
    let reference_labels = tree
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

    let footnote_ids = tree
        .descendants()
        .filter_map(FootnoteReference::cast)
        .map(|footnote| normalize_label(&footnote.id()))
        .filter(|id| !id.is_empty())
        .collect();

    UsageLabels {
        reference_labels,
        footnote_ids,
    }
}

fn extend_usage_labels_from_project(
    usage: &mut UsageLabels,
    metadata: &crate::metadata::DocumentMetadata,
    config: &Config,
) {
    let doc_path = metadata
        .source_path
        .canonicalize()
        .unwrap_or_else(|_| metadata.source_path.clone());
    let project_root = crate::includes::find_bookdown_root(&doc_path)
        .or_else(|| crate::includes::find_quarto_root(&doc_path));
    let Some(project_root) = project_root else {
        return;
    };
    let is_bookdown = crate::includes::find_bookdown_root(&doc_path).is_some();

    for path in crate::includes::find_project_documents(&project_root, config, is_bookdown) {
        extend_usage_labels_from_file(usage, &path, config);
    }
}

fn extend_usage_labels_from_file(usage: &mut UsageLabels, path: &Path, config: &Config) {
    if let Ok(other_input) = std::fs::read_to_string(path) {
        let other_tree = crate::parser::parse(&other_input, Some(config.clone()));
        let other_usage = collect_usage_labels(&other_tree);
        usage.reference_labels.extend(other_usage.reference_labels);
        usage.footnote_ids.extend(other_usage.footnote_ids);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Flavor;
    use std::fs;
    use tempfile::TempDir;

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

    #[test]
    fn does_not_report_unused_definition_when_used_in_project_document() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path();
        let doc1 = root.join("1-one.Rmd");
        let doc2 = root.join("2-two.Rmd");
        fs::write(root.join("_bookdown.yml"), "").expect("write _bookdown.yml");
        fs::write(&doc1, "[shared]: https://example.com\n").expect("write doc1");
        fs::write(&doc2, "See [x][shared].\n").expect("write doc2");

        let input = fs::read_to_string(&doc1).expect("read doc1");
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parser::parse(&input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &doc1).expect("metadata");

        let rule = UnusedDefinitionsRule;
        let diagnostics = rule.check(&tree, &input, &config, Some(&metadata));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn reports_unused_definition_when_not_used_in_project_document() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path();
        let doc1 = root.join("1-one.Rmd");
        let doc2 = root.join("2-two.Rmd");
        fs::write(root.join("_bookdown.yml"), "").expect("write _bookdown.yml");
        fs::write(&doc1, "[shared]: https://example.com\n").expect("write doc1");
        fs::write(&doc2, "Plain text.\n").expect("write doc2");

        let input = fs::read_to_string(&doc1).expect("read doc1");
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parser::parse(&input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &doc1).expect("metadata");

        let rule = UnusedDefinitionsRule;
        let diagnostics = rule.check(&tree, &input, &config, Some(&metadata));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-definition-label");
    }

    #[test]
    fn falls_back_to_local_behavior_without_project_root() {
        let temp = TempDir::new().expect("tempdir");
        let doc = temp.path().join("standalone.qmd");
        fs::write(&doc, "[alone]: https://example.com\n").expect("write doc");

        let input = fs::read_to_string(&doc).expect("read doc");
        let config = Config::default();
        let tree = crate::parser::parse(&input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &doc).expect("metadata");

        let rule = UnusedDefinitionsRule;
        let diagnostics = rule.check(&tree, &input, &config, Some(&metadata));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-definition-label");
    }
}
