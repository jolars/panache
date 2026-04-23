use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::{AstNode, Crossref, FootnoteReference, Link, SyntaxNode};
use crate::utils::{
    crossref_resolution_labels, implicit_heading_ids, normalize_anchor_label, normalize_label,
};
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
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let labels = collect_definition_labels(tree, config, metadata);

        for link in tree.descendants().filter_map(Link::cast) {
            if link.dest().is_some() {
                continue;
            }

            let Some((label_text, location_node)) = extract_reference_label_and_node(&link) else {
                continue;
            };
            let normalized_label = normalize_label(&label_text);
            if normalized_label.is_empty()
                || labels.reference_labels.contains(&normalized_label)
                || labels.heading_text_labels.contains(&normalized_label)
            {
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
            if normalized.is_empty() || labels.footnote_ids.contains(&normalized) {
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
                let normalized = normalize_anchor_label(&label);
                if normalized.is_empty() {
                    continue;
                }

                let candidates =
                    crossref_resolution_labels(&normalized, config.extensions.bookdown_references);
                if candidates
                    .iter()
                    .any(|candidate| labels.crossref_labels.contains(candidate))
                {
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

#[derive(Default)]
struct DefinitionLabels {
    reference_labels: HashSet<String>,
    footnote_ids: HashSet<String>,
    crossref_labels: HashSet<String>,
    heading_text_labels: HashSet<String>,
}

fn collect_definition_labels(
    tree: &SyntaxNode,
    config: &Config,
    metadata: Option<&crate::metadata::DocumentMetadata>,
) -> DefinitionLabels {
    let mut labels = DefinitionLabels::default();
    extend_labels_from_tree(&mut labels, tree, config);

    let Some(metadata) = metadata else {
        return labels;
    };

    // Canonicalize to absolute path so project root discovery and path comparisons work
    // correctly regardless of whether the path was given as relative or absolute.
    let doc_path = metadata
        .source_path
        .canonicalize()
        .unwrap_or_else(|_| metadata.source_path.clone());
    let project_root = crate::includes::find_bookdown_root(&doc_path)
        .or_else(|| crate::includes::find_quarto_root(&doc_path));
    let Some(project_root) = project_root else {
        return labels;
    };
    let is_bookdown = crate::includes::find_bookdown_root(&doc_path).is_some();

    for path in crate::includes::find_project_documents(&project_root, config, is_bookdown) {
        if path == doc_path {
            continue;
        }
        if let Ok(other_input) = std::fs::read_to_string(&path) {
            let other_tree = crate::parser::parse(&other_input, Some(config.clone()));
            extend_labels_from_tree(&mut labels, &other_tree, config);
        }
    }

    labels
}

fn extend_labels_from_tree(labels: &mut DefinitionLabels, tree: &SyntaxNode, config: &Config) {
    let db = crate::salsa::SalsaDb::default();
    let symbol_index = crate::salsa::symbol_usage_index_from_tree(&db, tree, &config.extensions);

    labels.reference_labels.extend(
        symbol_index
            .reference_definition_entries()
            .map(|(label, _)| label.clone())
            .filter(|label| !label.is_empty()),
    );
    labels.footnote_ids.extend(
        symbol_index
            .footnote_definition_entries()
            .map(|(id, _)| id.clone())
            .filter(|id| !id.is_empty()),
    );
    labels.crossref_labels.extend(
        symbol_index
            .crossref_declaration_entries()
            .map(|(label, _)| label.clone())
            .filter(|label| !label.is_empty()),
    );

    if config.extensions.implicit_header_references && config.extensions.auto_identifiers {
        labels.heading_text_labels.extend(
            symbol_index
                .heading_label_entries()
                .map(|(label, _)| label.clone())
                .filter(|label| !label.is_empty()),
        );
    }

    if config.extensions.bookdown_references && config.extensions.auto_identifiers {
        labels
            .crossref_labels
            .extend(collect_implicit_heading_ids(tree, &config.extensions));
    }
}

fn collect_implicit_heading_ids(
    tree: &SyntaxNode,
    extensions: &crate::config::Extensions,
) -> HashSet<String> {
    implicit_heading_ids(tree, extensions)
        .into_iter()
        .map(|entry| entry.id)
        .collect()
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
    use std::fs;
    use tempfile::TempDir;

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
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
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
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
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

    #[test]
    fn accepts_bookdown_prefixed_crossref_to_chunk_label() {
        let input = "See \\@ref(fig:plot).\n\n```{r}\n#| label: plot\n#| fig-cap: \"Plot\"\nplot(1:10)\n```\n";
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn accepts_bookdown_theorem_environment_crossref() {
        let input = "Exercise \\@ref(exr:mu)\n\n::: {#mu .exercise}\nfoobar\n:::\n";
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn accepts_bookdown_equation_crossref_with_mixed_case_label() {
        let input =
            "\\begin{equation}\n  1 = 1\n  (\\#eq:solveG)\n\\end{equation}\n\n\\@ref(eq:solveG)\n";
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        config.extensions.bookdown_equation_references = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn accepts_bookdown_section_crossref_with_hyphenated_slug() {
        let input = "# Heading\n\nA ref to \\@ref(heading).\n\n## Heading 2\n\nA ref to \\@ref(heading-2).\n";
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check(&tree, input, &config, None);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn resolves_bookdown_crossref_with_empty_bookdown_yml() {
        // When _bookdown.yml exists but is empty, bookdown auto-discovers all .Rmd files.
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("_bookdown.yml"), "").expect("write _bookdown.yml");
        fs::write(
            root.join("1-one.Rmd"),
            "---\ntitle: Test\n---\n# One {#one}\n",
        )
        .expect("write 1-one.Rmd");
        fs::write(root.join("2-two.Rmd"), "\\@ref(one)\n").expect("write 2-two.Rmd");

        let input = fs::read_to_string(root.join("2-two.Rmd")).expect("read 2-two.Rmd");
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;

        let tree = crate::parser::parse(&input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &root.join("2-two.Rmd"))
            .expect("metadata");
        let rule = UndefinedReferencesRule;
        let diagnostics = rule.check(&tree, &input, &config, Some(&metadata));
        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.code != "undefined-reference-label"),
            "empty _bookdown.yml should auto-discover .Rmd files in the project"
        );
    }
}
