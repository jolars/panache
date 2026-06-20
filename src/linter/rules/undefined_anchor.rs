use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{AttributeNode, Citation, Link, SyntaxKind, SyntaxNode};
use crate::utils::implicit_heading_ids;
use rowan::ast::AstNode;
use std::collections::HashSet;

pub struct UndefinedAnchorRule;

impl Rule for UndefinedAnchorRule {
    fn name(&self) -> &str {
        "undefined-anchor"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "undefined-anchor",
            default_on: true,
            requires: Requirement::Always,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("undefined-anchor")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::LINK]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let (tree, input, config, metadata) = (cx.tree, cx.input, cx.config, cx.metadata);
        let anchors = collect_anchors(tree, config, metadata);
        let mut diagnostics = Vec::new();

        for link in cx
            .nodes(SyntaxKind::LINK)
            .iter()
            .cloned()
            .filter_map(Link::cast)
        {
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

    if config.extensions.citations {
        for citation in tree.descendants().filter_map(Citation::cast) {
            for key in citation.key_texts() {
                if key.is_empty() {
                    continue;
                }
                anchors.insert(format!("ref-{key}"));
            }
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
        rule.check_tree(&tree, input, &config, None)
    }

    fn parse_and_lint_with_config(input: &str, config: Config) -> Vec<Diagnostic> {
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = UndefinedAnchorRule;
        rule.check_tree(&tree, input, &config, None)
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
    fn resolves_explicit_id_on_bracketed_span() {
        let input = "[Justin Wallace]{#APA2023}\n\nSee [the source](#APA2023).\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn resolves_explicit_id_on_html_div_block() {
        // Regression for issue #263: <div id="..."> blocks under Pandoc
        // dialect should expose their id structurally so anchor links
        // resolve. The Pandoc-dialect parser lifts the block to
        // HTML_BLOCK_DIV and the salsa indexer reads the open tag.
        let input =
            "<div id=\"anchor-c\">Content in a div with id.</div>\n\nSee [link](#anchor-c).\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn resolves_explicit_id_on_html_div_block_multiline() {
        let input =
            "<div id=\"sec-a\">\n\n**Important** content.\n\n</div>\n\nSee [section A](#sec-a).\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn resolves_explicit_id_on_html_inline_span() {
        // Issue #263 sibling for inline <span id="...">: the Pandoc-dialect
        // parser lifts the inline tag pair to INLINE_HTML_SPAN with HTML_ATTRS
        // exposed structurally, so the existing AttributeNode walk registers
        // the id as a crossref declaration.
        let input = "<span id=\"anchor-c\">marker</span>\n\nSee [link](#anchor-c).\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn resolves_explicit_id_on_html_strict_block_inside_blockquote() {
        // Sibling of resolves_explicit_id_on_html_div_block for non-div
        // strict-block tags inside a blockquote. The parser's bq clean-shape
        // lift covers `<section>`/`<form>`/... inside `>` quotes; for the
        // salsa anchor walk to pick up `id` the open tag's attribute region
        // must be tokenized as HTML_ATTRS even at bq_depth > 0.
        let input = "> <section id=\"sec-a\">\n>\n> Body text.\n>\n> </section>\n\nSee [the section](#sec-a).\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn resolves_explicit_id_on_html_strict_block_same_line_inside_blockquote() {
        // Same-line bq lift counterpart: `> <section id="x">body</section>`
        // routes through the parser's `same_line_bq_lift_tag` path which
        // also tokenizes the open tag's HTML_ATTRS so the salsa anchor
        // walk finds the id.
        let input =
            "> <section id=\"sec-a\">Inline body.</section>\n\nSee [the section](#sec-a).\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn resolves_explicit_id_on_html_strict_block_messy_inside_blockquote() {
        // Messy-shape bq lift counterpart: `> <section id="x">first\n>
        // second</section>` (open-trailing + butted-close) routes through
        // `bq_messy_lift_tag` which also tokenizes HTML_ATTRS so the salsa
        // anchor walk finds the id.
        let input =
            "> <section id=\"sec-a\">first\n> second</section>\n\nSee [the section](#sec-a).\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn resolves_explicit_id_on_html_inline_span_inside_paragraph() {
        let input =
            "Body text with a <span id=\"sec-a\">marker</span> inline.\n\nLink: [here](#sec-a).\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            diagnostics
        );
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
    fn resolves_ref_anchor_for_in_document_citation() {
        // Pandoc renders bibliography entries with id="ref-<citekey>"; per the
        // pandoc maintainer this is the canonical way to override a citation's
        // link text. See https://github.com/jgm/pandoc/issues/11657.
        let input = "See @laws1 [@laws1].\n\nLater: [my label](#ref-laws1).\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn reports_ref_anchor_when_no_matching_citation() {
        let input = "Just text. See [label](#ref-laws1).\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-anchor");
        assert!(diagnostics[0].message.contains("#ref-laws1"));
    }

    #[test]
    fn ref_anchor_ignored_when_citations_extension_disabled() {
        let input = "See @laws1.\n\n[label](#ref-laws1).\n";
        let mut config = Config::default();
        config.extensions.citations = false;
        let diagnostics = parse_and_lint_with_config(input, config);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-anchor");
        assert!(diagnostics[0].message.contains("#ref-laws1"));
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
        let diagnostics = rule.check_tree(&tree, &input, &config, Some(&metadata));
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
        let diagnostics = rule.check_tree(&tree, &input, &config, Some(&metadata));
        assert_eq!(diagnostics.len(), 1, "got {:?}", diagnostics);
        assert_eq!(diagnostics[0].code, "undefined-anchor");
    }
}
