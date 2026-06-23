use crate::linter::diagnostics::{Diagnostic, Edit, Fix, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{ImageLink, Link, LinkDest, SyntaxKind};
use rowan::ast::AstNode;
use rowan::{TextRange, TextSize};

pub struct CrossrefAsLinkTargetRule;

impl Rule for CrossrefAsLinkTargetRule {
    fn name(&self) -> &str {
        "crossref-as-link-target"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "crossref-as-link-target",
            default_on: true,
            requires: Requirement::Citations,
            auto_fix: true,
            codes: const { &[DiagnosticCode::warning("crossref-as-link-target")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::LINK, SyntaxKind::IMAGE_LINK]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let mut diagnostics = Vec::new();

        for link in cx
            .nodes(SyntaxKind::LINK)
            .iter()
            .cloned()
            .filter_map(Link::cast)
        {
            if let Some(dest) = link.dest() {
                push_if_at_target(&dest, input, &mut diagnostics);
            }
        }

        for image in cx
            .nodes(SyntaxKind::IMAGE_LINK)
            .iter()
            .cloned()
            .filter_map(ImageLink::cast)
        {
            if let Some(dest) = image.dest() {
                push_if_at_target(&dest, input, &mut diagnostics);
            }
        }

        diagnostics
    }
}

fn push_if_at_target(dest: &LinkDest, input: &str, diagnostics: &mut Vec<Diagnostic>) {
    let url = dest.url_content();
    if !url.trim_start().starts_with('@') {
        return;
    }

    let dest_text = dest.syntax().text().to_string();
    let Some(at_offset) = dest_text.find('@') else {
        return;
    };
    let dest_start: usize = dest.syntax().text_range().start().into();
    let at_pos = dest_start + at_offset;
    let at_range = TextRange::new(
        TextSize::from(at_pos as u32),
        TextSize::from((at_pos + 1) as u32),
    );

    let diagnostic = Diagnostic::warning(
        Location::from_range(at_range, input),
        "crossref-as-link-target",
        "Link target starts with '@'; cross-references and citation keys must \
         stand alone, not appear as a link destination",
    )
    .with_fix(Fix::safe(
        "Replace '@' with '#' to link to an anchor",
        vec![Edit {
            range: at_range,
            replacement: "#".to_string(),
        }],
    ));

    diagnostics.push(diagnostic);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Extensions, Flavor};

    fn pandoc_config() -> Config {
        Config {
            flavor: Flavor::Pandoc,
            extensions: Extensions::for_flavor(Flavor::Pandoc),
            ..Default::default()
        }
    }

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = pandoc_config();
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = CrossrefAsLinkTargetRule;
        rule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn flags_inline_link_with_at_target() {
        let input = "See [Figure 2](@fig-2).\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "crossref-as-link-target");
        let fix = diagnostics[0].fix.as_ref().expect("fix");
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, "#");
        let range = fix.edits[0].range;
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "@");
    }

    #[test]
    fn flags_link_with_citation_key_target() {
        let input = "See [Smith 2020](@smith2020).\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "crossref-as-link-target");
    }

    #[test]
    fn flags_image_link_with_at_target() {
        let input = "![Alt text](@fig-bar)\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "crossref-as-link-target");
    }

    #[test]
    fn ignores_normal_anchor_link() {
        let input = "See [Figure 2](#fig-2).\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn ignores_url_with_internal_at() {
        let input = "Email [me](mailto:foo@bar.com).\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn ignores_bare_citation() {
        // `[@key]` is a citation, not a link — should not be flagged.
        let input = "As shown by @smith2020.\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn ignores_bracket_only_citation() {
        let input = "Earlier work [@smith2020] showed this.\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn fix_targets_only_the_at_character() {
        let input = "See [圖2](@fig-2).\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        let fix = diagnostics[0].fix.as_ref().expect("fix");
        let range = fix.edits[0].range;
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(end - start, 1);
        assert_eq!(&input[start..end], "@");
    }

    #[test]
    fn flags_inside_paragraph_with_surrounding_text() {
        let input = "bla([圖2](@fig-2))bla\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "crossref-as-link-target");
    }
}
