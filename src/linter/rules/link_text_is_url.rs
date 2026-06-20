use rowan::ast::AstNode;

use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Edit, Fix, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{Link, SyntaxKind};

pub struct LinkTextIsUrlRule;

impl Rule for LinkTextIsUrlRule {
    fn name(&self) -> &str {
        "link-text-is-url"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "link-text-is-url",
            default_on: true,
            requires: Requirement::Always,
            auto_fix: true,
            codes: const { &[DiagnosticCode::warning("link-text-is-url")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::LINK]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let is_commonmark = panache_parser::Dialect::for_flavor(cx.config.flavor)
            == panache_parser::Dialect::CommonMark;
        let mut diagnostics = Vec::new();

        for link in cx
            .nodes(SyntaxKind::LINK)
            .iter()
            .cloned()
            .filter_map(Link::cast)
        {
            // Skip reference-style links: this rule is only about inline `[text](url)`.
            if link.reference().is_some() {
                continue;
            }
            let Some(text_node) = link.text() else {
                continue;
            };
            // Reject any structural children in the link text (e.g. bold/emph/code).
            // Their rendered bytes might match the URL but the visible link is not a plain URL.
            if text_node.syntax().children().next().is_some() {
                continue;
            }
            let Some(dest_node) = link.dest() else {
                continue;
            };
            let Some((url, title_present)) = split_url_and_title(&dest_node.url()) else {
                continue;
            };
            if title_present {
                continue;
            }
            let text = text_node.text_content();
            if text != url {
                continue;
            }
            // Confirm the URL is autolink-safe in this dialect.
            if panache_parser::parser::inlines::links::try_parse_autolink(
                &format!("<{}>", url),
                is_commonmark,
            )
            .is_none()
            {
                continue;
            }

            let range = link.syntax().text_range();
            let location = Location::from_range(range, input);
            let replacement = format!("<{}>", url);
            let diag = Diagnostic::warning(
                location,
                "link-text-is-url",
                "Link text is identical to the URL; an autolink is shorter and clearer.",
            )
            .with_note(
                DiagnosticNoteKind::Help,
                format!("rewrite as `{}`", replacement),
            )
            .with_fix(Fix {
                message: "Convert to autolink".to_string(),
                edits: vec![Edit { range, replacement }],
            });
            diagnostics.push(diag);
        }

        diagnostics
    }
}

/// Split a `LinkDest` text body into `(url, title_present)`.
///
/// `LinkDest::url()` returns the destination body without the surrounding `(` and `)`
/// (those live as sibling tokens in the CST). Inside that body, a title — when present —
/// is separated from the URL by whitespace and wrapped in `"…"`, `'…'`, or `(…)`.
/// Returns `None` if the body is empty or the URL portion would be empty.
fn split_url_and_title(body: &str) -> Option<(String, bool)> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    let url_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let url = &trimmed[..url_end];
    if url.is_empty() {
        return None;
    }
    let rest = trimmed[url_end..].trim();
    Some((url.to_string(), !rest.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = LinkTextIsUrlRule;
        rule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn fires_when_text_equals_url() {
        let diagnostics =
            parse_and_lint("See [https://example.com/](https://example.com/) for details.\n");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "link-text-is-url");
        let fix = diagnostics[0].fix.as_ref().expect("autofix");
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, "<https://example.com/>");
    }

    #[test]
    fn does_not_fire_on_trailing_slash_mismatch() {
        // The literal issue example: changing destination is unsafe, so we skip it.
        let diagnostics = parse_and_lint("[https://example.net/](https://example.net)\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn does_not_fire_when_destinations_differ() {
        let diagnostics = parse_and_lint("[https://x.com/](https://x.com/?q=1)\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn does_not_fire_when_title_is_present() {
        let diagnostics =
            parse_and_lint("[https://example.com/](https://example.com/ \"Title\")\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn does_not_fire_when_text_has_formatting() {
        // Rendered TEXT bytes match, but the visible link contains structural formatting.
        let diagnostics = parse_and_lint("[**https://example.com/**](https://example.com/)\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn does_not_fire_on_scheme_less_path() {
        // Fails `try_parse_autolink` (no scheme), even though text == URL.
        let diagnostics = parse_and_lint("[/relative/path](/relative/path)\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn does_not_fire_on_existing_autolink() {
        let diagnostics = parse_and_lint("Visit <https://example.com/> today.\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn fires_on_mailto_url_under_commonmark() {
        // Under Pandoc, `[mailto:a@b.com]` is a citation candidate, not a link.
        // Test the mailto case under CommonMark where bracket-shape is unambiguously a link.
        let mut config = Config::default();
        config.flavor = crate::config::Flavor::CommonMark;
        config.extensions = panache_parser::Extensions::for_flavor(config.flavor);
        let input = "[mailto:a@b.com](mailto:a@b.com)\n";
        let tree = crate::parser::parse(input, Some(config.clone()));
        let diagnostics = LinkTextIsUrlRule.check_tree(&tree, input, &config, None);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].fix.as_ref().unwrap().edits[0].replacement,
            "<mailto:a@b.com>"
        );
    }

    #[test]
    fn skips_reference_style_links() {
        let input = "[https://example.com/][site]\n\n[site]: https://example.com/\n";
        let diagnostics = parse_and_lint(input);
        assert!(diagnostics.is_empty());
    }
}
