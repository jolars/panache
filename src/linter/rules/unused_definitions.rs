use std::collections::HashSet;
use std::path::Path;

use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{
    AstNode, FootnoteReference, ImageLink, Link, SyntaxKind, SyntaxNode, UnresolvedReference,
};
use crate::utils::normalize_label;

pub struct UnusedDefinitionsRule;

impl Rule for UnusedDefinitionsRule {
    fn name(&self) -> &str {
        "unused-definitions"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "unused-definitions",
            default_on: true,
            requires: Requirement::Always,
            auto_fix: false,
            codes: const {
                &[
                    DiagnosticCode::warning("unused-definition-label"),
                    DiagnosticCode::warning("unused-footnote-id"),
                ]
            },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[
            SyntaxKind::LINK,
            SyntaxKind::IMAGE_LINK,
            SyntaxKind::UNRESOLVED_REFERENCE,
            SyntaxKind::FOOTNOTE_REFERENCE,
        ]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let (tree, input, config, metadata) = (cx.tree, cx.input, cx.config, cx.metadata);
        let db = crate::salsa::SalsaDb::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, tree, &config.extensions);
        let mut used = collect_usage_labels(
            cx.nodes(SyntaxKind::LINK)
                .iter()
                .chain(cx.nodes(SyntaxKind::IMAGE_LINK).iter())
                .chain(cx.nodes(SyntaxKind::UNRESOLVED_REFERENCE).iter())
                .chain(cx.nodes(SyntaxKind::FOOTNOTE_REFERENCE).iter())
                .cloned(),
        );

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

/// Collect reference/footnote usage labels from a stream of candidate nodes.
///
/// Driven off pre-bucketed nodes for the local document (one shared walk) and
/// off `other_tree.descendants()` for project sibling files; both feed the same
/// per-node classifiers so the logic stays single-sourced.
fn collect_usage_labels(nodes: impl Iterator<Item = SyntaxNode>) -> UsageLabels {
    let mut reference_labels: HashSet<String> = HashSet::new();
    let mut footnote_ids: HashSet<String> = HashSet::new();

    for node in nodes {
        match node.kind() {
            SyntaxKind::LINK => {
                if let Some(label) = Link::cast(node).and_then(usage_label_from_link) {
                    reference_labels.insert(label);
                }
            }
            // Reference-style images (`![alt][label]`, collapsed `![label][]`,
            // shortcut `![label]`) resolve to `IMAGE_LINK` rather than `LINK`,
            // so they count as usages of the label they reference too.
            SyntaxKind::IMAGE_LINK => {
                if let Some(label) = ImageLink::cast(node).and_then(usage_label_from_image) {
                    reference_labels.insert(label);
                }
            }
            // Bracket-shape patterns whose label didn't resolve as a refdef
            // still count as a usage of the label they reference — so a
            // `[GitHub]` shortcut counts as using the `[github]:` definition
            // even if that definition lives in another file.
            SyntaxKind::UNRESOLVED_REFERENCE => {
                if let Some(label) =
                    UnresolvedReference::cast(node).and_then(usage_label_from_unresolved)
                {
                    reference_labels.insert(label);
                }
            }
            SyntaxKind::FOOTNOTE_REFERENCE => {
                if let Some(footnote) = FootnoteReference::cast(node) {
                    let id = normalize_label(&footnote.id());
                    if !id.is_empty() {
                        footnote_ids.insert(id);
                    }
                }
            }
            _ => {}
        }
    }

    UsageLabels {
        reference_labels,
        footnote_ids,
    }
}

fn usage_label_from_link(link: Link) -> Option<String> {
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
        .filter(|label| !label.is_empty())
}

fn usage_label_from_image(image: ImageLink) -> Option<String> {
    if image
        .syntax()
        .ancestors()
        .any(|ancestor| ancestor.kind() == SyntaxKind::REFERENCE_DEFINITION)
    {
        return None;
    }
    if image.dest().is_some() {
        return None;
    }
    if let Some(link_ref) = image.reference() {
        let label = normalize_label(&link_ref.label());
        if !label.is_empty() {
            return Some(label);
        }
    }
    image
        .alt()
        .map(|alt| normalize_label(&alt.text()))
        .filter(|label| !label.is_empty())
}

fn usage_label_from_unresolved(unresolved: UnresolvedReference) -> Option<String> {
    if let Some(label) = unresolved.label() {
        let normalized = normalize_label(&label);
        if !normalized.is_empty() {
            return Some(normalized);
        }
    }
    let normalized = normalize_label(&unresolved.text());
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
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
        let other_usage = collect_usage_labels(other_tree.descendants());
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
        rule.check_tree(&tree, input, &config, None)
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
    fn accepts_definition_used_by_full_reference_image() {
        let input = "![This is an image][image-path]\n\n[image-path]: https://example.com/i.png\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "full reference image should count as a usage: {diagnostics:?}"
        );
    }

    #[test]
    fn accepts_definition_used_by_collapsed_reference_image() {
        let input = "![image-path][]\n\n[image-path]: https://example.com/i.png\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "collapsed reference image should count as a usage: {diagnostics:?}"
        );
    }

    #[test]
    fn accepts_definition_used_by_shortcut_reference_image() {
        let input = "![image-path]\n\n[image-path]: https://example.com/i.png\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "shortcut reference image should count as a usage: {diagnostics:?}"
        );
    }

    #[test]
    fn accepts_definitions_used_by_reference_image_inside_reference_link() {
        let input = "[![example][example-badge]][example-url]\n\n[example-badge]: https://example.com\n[example-url]: https://example.com\n";
        let diagnostics = parse_and_lint(input);
        assert!(
            diagnostics.is_empty(),
            "reference image nested inside a reference link should count both labels as used: {diagnostics:?}"
        );
    }

    #[test]
    fn still_reports_unused_definition_with_only_inline_image() {
        let input = "![alt](https://example.com/i.png)\n\n[unused]: https://example.org\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-definition-label");
        assert!(diagnostics[0].message.contains("[unused]"));
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
        let diagnostics = rule.check_tree(&tree, &input, &config, Some(&metadata));
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
        let diagnostics = rule.check_tree(&tree, &input, &config, Some(&metadata));
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
        let diagnostics = rule.check_tree(&tree, &input, &config, Some(&metadata));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unused-definition-label");
    }
}
