use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::{AttributeNode, Link, SyntaxNode};
use crate::utils::implicit_heading_ids;
use rowan::ast::AstNode;
use std::collections::HashSet;

pub struct UndefinedAnchorRule;

impl Rule for UndefinedAnchorRule {
    fn name(&self) -> &str {
        "undefined-anchor"
    }

    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let anchors = collect_anchors(tree, config, metadata);
        let mut diagnostics = Vec::new();

        for link in tree.descendants().filter_map(Link::cast) {
            let Some(dest) = link.dest() else {
                continue;
            };
            let raw = dest.url_content();
            let trimmed = raw.trim();
            if !trimmed.starts_with('#') || trimmed == "#" {
                continue;
            }
            let Some(id) = dest.hash_anchor_id() else {
                continue;
            };
            if id.is_empty() || anchors.contains(&id) {
                continue;
            }
            let range = dest
                .hash_anchor_id_range()
                .unwrap_or_else(|| dest.syntax().text_range());
            diagnostics.push(Diagnostic::warning(
                Location::from_range(range, input),
                "undefined-anchor",
                format!("Anchor '#{}' not found in document", id),
            ));
        }

        diagnostics
    }
}

fn collect_anchors(
    tree: &SyntaxNode,
    config: &Config,
    metadata: Option<&crate::metadata::DocumentMetadata>,
) -> HashSet<String> {
    let mut anchors = HashSet::new();
    extend_anchors(&mut anchors, tree, config);

    let Some(metadata) = metadata else {
        return anchors;
    };

    let doc_path = metadata
        .source_path
        .canonicalize()
        .unwrap_or_else(|_| metadata.source_path.clone());
    let roots = crate::includes::find_project_roots(&doc_path);
    let Some(bookdown_root) = roots.bookdown else {
        return anchors;
    };

    for path in crate::includes::find_project_documents(&bookdown_root, config, true) {
        if path == doc_path {
            continue;
        }
        if let Ok(other_input) = std::fs::read_to_string(&path) {
            let other_tree = crate::parser::parse(&other_input, Some(config.clone()));
            extend_anchors(&mut anchors, &other_tree, config);
        }
    }

    anchors
}

fn extend_anchors(anchors: &mut HashSet<String>, tree: &SyntaxNode, config: &Config) {
    let db = crate::salsa::SalsaDb::default();
    let symbol_index = crate::salsa::symbol_usage_index_from_tree(&db, tree, &config.extensions);

    anchors.extend(
        symbol_index
            .crossref_declaration_entries()
            .map(|(label, _)| label.clone())
            .filter(|label| !label.is_empty()),
    );

    if config.extensions.auto_identifiers {
        for entry in implicit_heading_ids(tree, &config.extensions) {
            if heading_has_explicit_id(&entry.heading) {
                continue;
            }
            if entry.id.is_empty() {
                continue;
            }
            anchors.insert(entry.id);
        }
    }
}

fn heading_has_explicit_id(heading: &SyntaxNode) -> bool {
    heading
        .children()
        .filter_map(AttributeNode::cast)
        .any(|attribute| attribute.id().is_some())
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
        let rule = UndefinedAnchorRule;
        rule.check(&tree, input, &config, None)
    }

    fn parse_and_lint_with_config(input: &str, config: Config) -> Vec<Diagnostic> {
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedAnchorRule;
        rule.check(&tree, input, &config, None)
    }

    #[test]
    fn resolves_explicit_heading_id() {
        let input = "# Heading {#h1}\n\nSee [here](#h1).\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn resolves_explicit_id_on_fenced_div() {
        let input = "::: {#note}\nSome content.\n:::\n\nSee [the note](#note).\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn resolves_implicit_heading_with_auto_identifiers_on() {
        let input = "# Heading Name\n\nSee [there](#heading-name).\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn reports_implicit_heading_when_auto_identifiers_off() {
        let input = "# Heading Name\n\nSee [there](#heading-name).\n";
        let mut config = Config::default();
        config.extensions.auto_identifiers = false;
        let diagnostics = parse_and_lint_with_config(input, config);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-anchor");
        assert!(diagnostics[0].message.contains("#heading-name"));
    }

    #[test]
    fn reports_typo_anchor() {
        let input = "# Heading {#real}\n\nSee [link](#reel).\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-anchor");
        assert!(diagnostics[0].message.contains("#reel"));
    }

    #[test]
    fn case_sensitive_mismatch_reports() {
        let input = "# Heading {#Foo}\n\nSee [link](#foo).\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("#foo"));
    }

    #[test]
    fn ignores_bare_hash() {
        let input = "Back to [top](#).\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn ignores_other_doc_fragment() {
        let input = "See [there](other.md#frag).\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn ignores_absolute_url_with_fragment() {
        let input = "See [there](https://example.com#frag).\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn ignores_inline_link_without_hash() {
        let input = "See [there](https://example.com).\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn cross_file_anchor_resolves_in_bookdown() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("_bookdown.yml"), "").expect("write _bookdown.yml");
        fs::write(
            root.join("01-intro.Rmd"),
            "---\ntitle: Intro\n---\n# Intro {#shared}\n",
        )
        .expect("write 01-intro.Rmd");
        fs::write(root.join("02-body.Rmd"), "See [the intro](#shared).\n")
            .expect("write 02-body.Rmd");

        let path = root.join("02-body.Rmd");
        let input = fs::read_to_string(&path).expect("read 02-body.Rmd");
        let mut config = Config {
            flavor: Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;

        let tree = crate::parser::parse(&input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &path).expect("metadata");
        let rule = UndefinedAnchorRule;
        let diagnostics = rule.check(&tree, &input, &config, Some(&metadata));
        assert!(
            diagnostics.iter().all(|d| d.code != "undefined-anchor"),
            "cross-file anchor should resolve in bookdown projects: {:?}",
            diagnostics
        );
    }

    #[test]
    fn cross_file_anchor_does_not_resolve_in_quarto_book() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("_quarto.yml"), "project:\n  type: book\n").expect("write _quarto.yml");
        fs::write(
            root.join("intro.qmd"),
            "---\ntitle: Intro\n---\n# Intro {#shared}\n",
        )
        .expect("write intro.qmd");
        fs::write(root.join("body.qmd"), "See [the intro](#shared).\n").expect("write body.qmd");

        let path = root.join("body.qmd");
        let input = fs::read_to_string(&path).expect("read body.qmd");
        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = crate::parser::parse(&input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &path).expect("metadata");
        let rule = UndefinedAnchorRule;
        let diagnostics = rule.check(&tree, &input, &config, Some(&metadata));
        assert_eq!(diagnostics.len(), 1, "got {:?}", diagnostics);
        assert_eq!(diagnostics[0].code, "undefined-anchor");
    }
}
