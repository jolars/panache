use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::project_index::extend_anchors;
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{Link, SyntaxKind};
use rowan::ast::AstNode;
use std::collections::HashSet;

pub struct UndefinedAnchorRule;

impl Rule for UndefinedAnchorRule {
    fn description(&self) -> &'static str {
        include_str!("docs/undefined-anchor.md")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn examples(&self) -> &'static [crate::linter::docs::Example] {
        use crate::linter::docs::Example;
        const { &[Example::new("[Methods](#methods)\n", &["undefined-anchor"])] }
    }

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
        let input = cx.input;
        // Build only the current document's anchors (cheap); resolve cross-file
        // against the shared project aggregate by membership rather than copying
        // it into a local set per file (that copy was an O(n^2) hot spot).
        let doc_symbols = cx.symbol_index();
        let mut local = HashSet::new();
        extend_anchors(&mut local, cx.tree, cx.config, &doc_symbols);
        let project = cx.project_symbol_index();
        let project = project.as_deref();
        let anchor_defined =
            |id: &str| local.contains(id) || project.is_some_and(|p| p.anchors.contains(id));
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
            if id.is_empty() || anchor_defined(&id) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Flavor};
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
    fn resolves_myst_targets_with_and_without_blank_lines() {
        let input = "See [a](#tgt) and [b](#tgt2).\n\n(tgt)=\n\n## Foo\n\n(tgt2)=\n## Bar\n";
        let config = Config {
            flavor: Flavor::Myst,
            extensions: crate::config::Extensions::for_flavor(Flavor::Myst),
            ..Default::default()
        };
        let diagnostics = parse_and_lint_with_config(input, config);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn resolves_myst_targets_in_blockquotes() {
        let input = "> (saved)=\n>\n> Content.\n\nSee [link](#saved).\n";
        let config = Config {
            flavor: Flavor::Myst,
            extensions: crate::config::Extensions::for_flavor(Flavor::Myst),
            ..Default::default()
        };
        let diagnostics = parse_and_lint_with_config(input, config);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn myst_targets_respect_extension_overrides() {
        let input = "(saved)=\n\nContent.\n\nSee [link](#saved).\n";
        for flavor in [Flavor::Myst, Flavor::CommonMark] {
            for enabled in [false, true] {
                let mut config = Config {
                    flavor,
                    extensions: crate::config::Extensions::for_flavor(flavor),
                    ..Default::default()
                };
                config.extensions.myst_targets = enabled;
                let diagnostics = parse_and_lint_with_config(input, config);
                assert_eq!(
                    diagnostics.len(),
                    usize::from(!enabled),
                    "{flavor:?}, myst-targets={enabled}: {diagnostics:?}"
                );
            }
        }
    }

    #[test]
    fn myst_targets_in_literal_content_do_not_resolve() {
        for content in [
            "`(saved)=`",
            "```markdown\n(saved)=\n```",
            "    (saved)=",
            "\\(saved)=",
            "```{code} markdown\n(saved)=\n```",
        ] {
            let input = format!("{content}\n\nSee [link](#saved).\n");
            let config = Config {
                flavor: Flavor::Myst,
                extensions: crate::config::Extensions::for_flavor(Flavor::Myst),
                ..Default::default()
            };
            let diagnostics = parse_and_lint_with_config(&input, config);
            assert_eq!(diagnostics.len(), 1, "{content:?}: {diagnostics:?}");
            assert!(diagnostics[0].message.contains("#saved"));
        }
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
    fn resolves_empty_native_div_ids() {
        for flavor in [Flavor::Pandoc, Flavor::Quarto, Flavor::RMarkdown] {
            for html in [
                "<div café=\"a\" id=\"saved\"></div>",
                "<div café=\"a\" id=\"saved\">\n</div>",
                "> <div café=\"a\" id=\"saved\"></div>",
            ] {
                let input = format!("{html}\n\n[link](#saved)\n");
                let config = Config {
                    flavor,
                    extensions: crate::config::Extensions::for_flavor(flavor),
                    ..Default::default()
                };
                let diagnostics = parse_and_lint_with_config(&input, config);
                assert!(
                    diagnostics.is_empty(),
                    "{flavor:?}: {input:?}: {diagnostics:?}"
                );
            }
        }
    }

    #[test]
    fn resolves_rendered_figure_caption_ids() {
        for caption in ["[Span]{#saved}", "<span id=\"saved\">Span</span>"] {
            for prefix in ["", "> "] {
                let input = format!("{prefix}![Caption {caption}](image.png)\n\n[link](#saved)\n");
                let diagnostics = parse_and_lint(&input);
                assert!(diagnostics.is_empty(), "{input:?}: {diagnostics:?}");
            }
        }
    }

    #[test]
    fn excludes_alt_only_image_description_ids_in_pandoc() {
        for image in [
            "Before ![Caption [Span]{#saved}](image.png)",
            "[![Caption [Span]{#saved}](image.png)](page.html)",
            "![Caption ![[Span]{#saved}](nested.png)](image.png)",
        ] {
            let input = format!("{image}\n\n[link](#saved)\n");
            let diagnostics = parse_and_lint(&input);
            assert_eq!(diagnostics.len(), 1, "{input:?}: {diagnostics:?}");
            assert_eq!(diagnostics[0].code, "undefined-anchor");
        }
        let input = "![Caption [Span]{#saved}](image.png)\n\n[link](#saved)\n";
        let mut config = Config::default();
        config.extensions.implicit_figures = false;
        let diagnostics = parse_and_lint_with_config(input, config);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "undefined-anchor");
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
    fn reports_implicit_heading_under_mdsvex() {
        // mdsvex ships no `rehype-slug`, so a heading gets no id and the link
        // would be dead in the rendered site.
        let input = "# Heading Name\n\nSee [there](#heading-name).\n";
        let config = Config {
            flavor: Flavor::Mdsvex,
            extensions: crate::config::Extensions::for_flavor(Flavor::Mdsvex),
            ..Default::default()
        };
        let diagnostics = parse_and_lint_with_config(input, config);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "undefined-anchor");
        assert!(diagnostics[0].message.contains("#heading-name"));
    }

    #[test]
    fn resolves_raw_html_ids_in_commonmark_flavors() {
        for flavor in [
            Flavor::CommonMark,
            Flavor::Gfm,
            Flavor::Mdsvex,
            Flavor::Myst,
            Flavor::Hugo,
        ] {
            for html in [
                "<h2 id=\"saved\">Saved</h2>",
                "**<span id='saved'></span>Saved**",
                "<div><span id=saved></span></div>",
                "<div>\n<span id=\"saved\"></span>\n</div>",
                "<h2\n  ID = \"saved\">Saved</h2>",
                "> <h2\n> id=\"saved\">Saved</h2>",
                "- <div\n    id=\"saved\">Saved</div>",
                "<script id=\"saved\">const x = 1;</script>",
                "<template id=\"saved\"><span></span></template>",
                "Before <template id=\"saved\"><span></span></template>",
            ] {
                let input = format!("{html}\n\n[link](#saved)\n");
                let config = Config {
                    flavor,
                    extensions: crate::config::Extensions::for_flavor(flavor),
                    ..Default::default()
                };
                let diagnostics = parse_and_lint_with_config(&input, config);
                assert!(
                    diagnostics.is_empty(),
                    "{flavor:?}: {input:?}: {diagnostics:?}"
                );
            }
        }
    }

    #[test]
    fn raw_html_ids_ignore_literal_content_in_commonmark() {
        for html in [
            "`<span id=\"saved\"></span>`",
            "```html\n<span id=\"saved\"></span>\n```",
            "\\<span id=\"saved\"></span>",
            "<!-- <span id=\"saved\"></span> -->",
            "<!--\n<span id=\"saved\"></span>\n-->",
            "<?example <span id=\"saved\"></span> ?>",
            "<![CDATA[<span id=\"saved\"></span>]]>",
            "<script>const s = '<span id=\"saved\"></span>';</script>",
            "<div>\n<style>\n/* <span id=\"saved\"></span> */\n</style>\n</div>",
            "<div id=\"saved>unfinished</div>",
        ] {
            let input = format!("{html}\n\n[link](#saved)\n");
            let config = Config {
                flavor: Flavor::CommonMark,
                extensions: crate::config::Extensions::for_flavor(Flavor::CommonMark),
                ..Default::default()
            };
            let diagnostics = parse_and_lint_with_config(&input, config);
            assert_eq!(diagnostics.len(), 1, "{input:?}: {diagnostics:?}");
            assert!(diagnostics[0].message.contains("#saved"));
        }
    }

    #[test]
    fn html_template_descendants_do_not_declare_anchors() {
        for flavor in [
            Flavor::CommonMark,
            Flavor::Gfm,
            Flavor::Mdsvex,
            Flavor::Myst,
            Flavor::Hugo,
        ] {
            for html in [
                "<template id=\"element\"><span id=\"saved\"></span></template>",
                "Before <template id=\"element\"><span id=\"saved\"></span></template>",
                "Before <TEMPLATE id=\"element\"><template></template><span id=\"saved\"></span></TEMPLATE >",
                "<template id=\"element\">\n\nBefore <template></template><span id=\"saved\"></span>\n\n</template>",
                "Before <template id=\"element\">\n\n<div><template></template><span id=\"saved\"></span></div>\n\n</template>",
                "<template id=\"element\"><script>const s = '</template>';</script><span id=\"saved\"></span></template>",
                "Before <template id=\"element\"><textarea></template></textarea><span id=\"saved\"></span></template>",
                "Before <template id=\"element\"><!-- </template> --><span id=\"saved\"></span></template>",
                "Before <template id=\"element\">![</template>Alt](image.png)<span id=\"saved\"></span></template>",
            ] {
                let input = format!(
                    "{html}\n\n<span id=\"after\"></span>\n\n\
                     [hidden](#saved) [opener](#element) [after](#after)\n"
                );
                let config = Config {
                    flavor,
                    extensions: crate::config::Extensions::for_flavor(flavor),
                    ..Default::default()
                };
                let diagnostics = parse_and_lint_with_config(&input, config);
                assert_eq!(
                    diagnostics.len(),
                    1,
                    "{flavor:?}: {input:?}: {diagnostics:?}"
                );
                assert!(diagnostics[0].message.contains("#saved"));
            }
        }
    }

    #[test]
    fn resolves_heading_ids_without_html_attributes() {
        for flavor in [Flavor::Gfm, Flavor::Hugo] {
            let input = "# <span class=\"badge\">Title</span>\n\n[link](#title)\n";
            let config = Config {
                flavor,
                extensions: crate::config::Extensions::for_flavor(flavor),
                ..Default::default()
            };
            let diagnostics = parse_and_lint_with_config(input, config);
            assert!(diagnostics.is_empty(), "{flavor:?}: {diagnostics:?}");
        }
    }

    #[test]
    fn raw_html_ids_exclude_image_descriptions() {
        for flavor in [
            Flavor::CommonMark,
            Flavor::Gfm,
            Flavor::Mdsvex,
            Flavor::Myst,
            Flavor::Hugo,
        ] {
            for image in [
                "![<span id=\"saved\"></span>Alt](image.png)",
                "![<span id=\"saved\"></span>Alt][image]\n\n[image]: image.png",
                "[![<span id=\"saved\"></span>Alt](image.png)](page.html)",
            ] {
                let input = format!("{image}\n\n[link](#saved)\n");
                let config = Config {
                    flavor,
                    extensions: crate::config::Extensions::for_flavor(flavor),
                    ..Default::default()
                };
                let diagnostics = parse_and_lint_with_config(&input, config);
                assert_eq!(
                    diagnostics.len(),
                    1,
                    "{flavor:?}: {input:?}: {diagnostics:?}"
                );
                assert!(diagnostics[0].message.contains("#saved"));
            }
        }
    }

    #[test]
    fn inline_html_ids_exclude_literal_element_contents() {
        for flavor in [
            Flavor::CommonMark,
            Flavor::Gfm,
            Flavor::Mdsvex,
            Flavor::Myst,
            Flavor::Hugo,
        ] {
            for tag in [
                "script", "STYLE", "textarea", "title", "xmp", "iframe", "noembed", "noframes",
            ] {
                let input = format!(
                    "Before <{tag} id=\"element\">const s = '<span id=\"saved\"></span>';</{tag}> \
                     <span id=\"after\"></span>\n\n[link](#saved) [opener](#element) [after](#after)\n"
                );
                let config = Config {
                    flavor,
                    extensions: crate::config::Extensions::for_flavor(flavor),
                    ..Default::default()
                };
                let diagnostics = parse_and_lint_with_config(&input, config);
                assert_eq!(
                    diagnostics.len(),
                    1,
                    "{flavor:?}: {input:?}: {diagnostics:?}"
                );
                assert!(diagnostics[0].message.contains("#saved"));
            }
        }
    }

    #[test]
    fn inline_html_literal_context_crosses_nested_inlines() {
        for html in [
            "Before <script>const s = '**<span id=\"saved\"></span>**';</script>",
            "Before <script>const s = '[<span id=\"saved\"></span>](page.html)';</script>",
            "Before <script>![</script>Alt](image.png)<span id=\"saved\"></span></script>",
            "Before **<script>**const s = '<span id=\"saved\"></span>';**</script>**",
            "Before <textarea>\n<span id=\"saved\"></span>\n</textarea>",
            "Before <script></textarea><span id=\"saved\"></span></SCRIPT >",
            "Before <script><script><span id=\"saved\"></span></script>",
            "Before <plaintext><span id=\"saved\"></span></plaintext>",
        ] {
            let input = format!("{html}\n\n[link](#saved)\n");
            let config = Config {
                flavor: Flavor::CommonMark,
                extensions: crate::config::Extensions::for_flavor(Flavor::CommonMark),
                ..Default::default()
            };
            let diagnostics = parse_and_lint_with_config(&input, config);
            assert_eq!(diagnostics.len(), 1, "{input:?}: {diagnostics:?}");
            assert!(diagnostics[0].message.contains("#saved"));
        }
    }

    #[test]
    fn inline_html_literal_context_survives_top_level_blocks() {
        for flavor in [
            Flavor::CommonMark,
            Flavor::Gfm,
            Flavor::Mdsvex,
            Flavor::Myst,
            Flavor::Hugo,
        ] {
            let input = "Before <script id=\"element\">\n\n\
                         <span id=\"saved\"></span>\n\n</script>\n\n\
                         <span id=\"after\"></span>\n\n\
                         [link](#saved) [opener](#element) [after](#after)\n";
            let config = Config {
                flavor,
                extensions: crate::config::Extensions::for_flavor(flavor),
                ..Default::default()
            };
            let diagnostics = parse_and_lint_with_config(input, config);
            assert_eq!(diagnostics.len(), 1, "{flavor:?}: {diagnostics:?}");
            assert!(diagnostics[0].message.contains("#saved"));
        }
    }

    #[test]
    fn inline_html_block_literal_closers_resume_declarations() {
        for html in [
            "Before <script>\n\n<pre>\n</script>\n</pre>\n\n<div id=\"saved\">Saved</div>\n",
            "> Before <script>\n>\n> </script>\n>\n> <span id=\"saved\">Saved</span>\n",
            "- Before <script>\n\n  </script>\n\n  <span id=\"saved\">Saved</span>\n",
        ] {
            let input = format!("{html}\n[link](#saved)\n");
            assert!(parse_and_lint(&input).is_empty(), "{input:?}");
        }
    }

    #[test]
    fn literal_closers_inside_raw_block_content_resume_declarations() {
        let input = "Before <script>\n\n<div>\n</script>\n\
                     <span id=\"saved\">Saved</span>\n</div>\n\n[link](#saved)\n";
        let config = Config {
            flavor: Flavor::CommonMark,
            extensions: crate::config::Extensions::for_flavor(Flavor::CommonMark),
            ..Default::default()
        };
        assert!(parse_and_lint_with_config(input, config).is_empty());
    }

    #[test]
    fn raw_html_closing_line_multiline_attributes_resolve_anchors() {
        let input = "<pre>\n<span\nid=\"saved\">Saved</span></pre>\n\n[link](#saved)\n";
        let config = Config {
            flavor: Flavor::CommonMark,
            extensions: crate::config::Extensions::for_flavor(Flavor::CommonMark),
            ..Default::default()
        };
        assert!(parse_and_lint_with_config(input, config).is_empty());
    }

    #[test]
    fn inline_html_literal_close_inside_a_raw_tag_resumes_declarations() {
        for tag in ["script", "textarea"] {
            let input = format!(
                "Before <{tag}><span title=\"</{tag}>\" id=\"saved\"></span> \
                 <span id=\"after\"></span>\n\n[link](#saved) [after](#after)\n"
            );
            let config = Config {
                flavor: Flavor::CommonMark,
                extensions: crate::config::Extensions::for_flavor(Flavor::CommonMark),
                ..Default::default()
            };
            let diagnostics = parse_and_lint_with_config(&input, config);
            assert_eq!(diagnostics.len(), 1, "{input:?}: {diagnostics:?}");
            assert!(diagnostics[0].message.contains("#saved"));
        }
    }

    #[test]
    fn image_description_html_does_not_change_literal_context() {
        let input =
            "Before ![<script>Alt](image.png) <span id=\"saved\"></span>\n\n[link](#saved)\n";
        let config = Config {
            flavor: Flavor::CommonMark,
            extensions: crate::config::Extensions::for_flavor(Flavor::CommonMark),
            ..Default::default()
        };
        assert!(parse_and_lint_with_config(input, config).is_empty());
    }

    #[test]
    fn raw_html_id_values_preserve_case_and_decode_entities() {
        let input = "<div id=\"a&amp;b\"></div>\n\n\
                     **<span id=\"#Saved\"></span>Saved**\n\n\
                     [entity](#a&b) [hash](##Saved) [typo](##saved)\n";
        let config = Config {
            flavor: Flavor::CommonMark,
            extensions: crate::config::Extensions::for_flavor(Flavor::CommonMark),
            ..Default::default()
        };
        let diagnostics = parse_and_lint_with_config(input, config);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].message.contains("##saved"));
        assert_eq!(
            &input[std::ops::Range::<usize>::from(diagnostics[0].location.range)],
            "#saved"
        );
    }

    #[test]
    fn raw_html_ids_require_raw_html_extension() {
        let input = "<h2 id=\"saved\">Saved</h2>\n\n[link](#saved)\n";
        let mut config = Config {
            flavor: Flavor::CommonMark,
            extensions: crate::config::Extensions::for_flavor(Flavor::CommonMark),
            ..Default::default()
        };
        config.extensions.raw_html = false;
        let diagnostics = parse_and_lint_with_config(input, config);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("#saved"));
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
    fn anchor_resolves_in_excluded_quarto_include() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::write(
            root.join("_quarto.yml"),
            "project:\n  render:\n    - main.qmd\n",
        )
        .unwrap();
        let path = root.join("main.qmd");
        let input =
            "See [Definition of Done](#definition-of-done).\n\n{{< include _success.qmd >}}\n";
        fs::write(&path, input).unwrap();
        fs::write(root.join("_success.qmd"), "# Definition of Done\n").unwrap();
        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config.clone()));
        let metadata = crate::metadata::extract_project_metadata(&tree, &path).unwrap();
        let diagnostics = UndefinedAnchorRule.check_tree(&tree, input, &config, Some(&metadata));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn cross_file_anchor_resolves_in_quarto_book() {
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
        assert!(
            diagnostics.iter().all(|d| d.code != "undefined-anchor"),
            "cross-file anchor should resolve in Quarto projects too: {:?}",
            diagnostics
        );
    }
}
