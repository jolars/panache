use crate::config::{Config, Flavor};
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::{AstNode, ChunkLabel, ChunkOption, Crossref, SyntaxKind, SyntaxNode};
use crate::utils::{crossref_resolution_labels, normalize_label};
use std::collections::HashMap;

pub struct FigureCrossrefCaptionsRule;

impl Rule for FigureCrossrefCaptionsRule {
    fn name(&self) -> &str {
        "figure-crossref-captions"
    }

    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        _metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        if !matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown) {
            return Vec::new();
        }

        let chunk_labels = collect_chunk_figure_caption_state(tree);
        let mut diagnostics = Vec::new();

        for crossref in tree.descendants().filter_map(Crossref::cast) {
            for key in crossref.keys() {
                let label = key.text();
                let normalized = normalize_label(&label);
                if !is_bookdown_figure_crossref(&normalized) {
                    continue;
                }

                let resolved_labels =
                    crossref_resolution_labels(&normalized, config.extensions.bookdown_references);
                let Some(has_caption) = resolved_labels
                    .iter()
                    .find_map(|candidate| chunk_labels.get(candidate))
                    .copied()
                else {
                    continue;
                };

                if has_caption {
                    continue;
                }

                diagnostics.push(Diagnostic::warning(
                    Location::from_range(key.text_range(), input),
                    "figure-crossref-captions",
                    format!(
                        "Figure cross-reference '@{}' targets a chunk label without a figure caption (`fig-cap`/`fig.cap`)",
                        label
                    ),
                ));
            }
        }

        diagnostics
    }
}

fn collect_chunk_figure_caption_state(tree: &SyntaxNode) -> HashMap<String, bool> {
    let mut out = HashMap::new();

    for code_block in tree
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::CODE_BLOCK)
    {
        let mut labels = Vec::new();
        let mut has_caption = false;

        if let Some(info_node) = code_block
            .children()
            .find(|child| child.kind() == SyntaxKind::CODE_FENCE_OPEN)
            .and_then(|open| {
                open.children()
                    .find(|child| child.kind() == SyntaxKind::CODE_INFO)
            })
        {
            if let Some(chunk_options) = info_node
                .children()
                .find(|child| child.kind() == SyntaxKind::CHUNK_OPTIONS)
            {
                for option_or_label in chunk_options.children() {
                    if let Some(chunk_label) = ChunkLabel::cast(option_or_label.clone()) {
                        let label = normalize_label(&chunk_label.text());
                        if !label.is_empty() {
                            labels.push(label);
                        }
                        continue;
                    }

                    let Some(option) = ChunkOption::cast(option_or_label) else {
                        continue;
                    };
                    let Some(key) = option.key() else {
                        continue;
                    };

                    if key.eq_ignore_ascii_case("label") {
                        if let Some(value) = option.value() {
                            let label = normalize_label(&value);
                            if !label.is_empty() {
                                labels.push(label);
                            }
                        }
                        continue;
                    }

                    if is_figure_caption_option_key(&key)
                        && option.value().is_some_and(|v| !v.is_empty())
                    {
                        has_caption = true;
                    }
                }
            }
        }

        if let Some(content) = code_block
            .children()
            .find(|child| child.kind() == SyntaxKind::CODE_CONTENT)
        {
            for option in content.descendants().filter_map(ChunkOption::cast) {
                let Some(key) = option.key() else {
                    continue;
                };
                if key.eq_ignore_ascii_case("label") {
                    if let Some(value) = option.value() {
                        let label = normalize_label(&value);
                        if !label.is_empty() {
                            labels.push(label);
                        }
                    }
                    continue;
                }
                if is_figure_caption_option_key(&key)
                    && option.value().is_some_and(|v| !v.is_empty())
                {
                    has_caption = true;
                }
            }
        }

        for label in labels {
            out.entry(label).or_insert(has_caption);
        }
    }

    out
}

fn is_bookdown_figure_crossref(label: &str) -> bool {
    label.starts_with("fig:")
}

fn is_figure_caption_option_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("fig-cap") || key.eq_ignore_ascii_case("fig.cap")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_lint(input: &str, flavor: Flavor) -> Vec<Diagnostic> {
        let mut config = Config {
            flavor,
            ..Default::default()
        };
        config.extensions = crate::config::Extensions::for_flavor(flavor);
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = FigureCrossrefCaptionsRule;
        rule.check(&tree, input, &config, None)
    }

    #[test]
    fn ignores_quarto_figure_crossref_without_caption() {
        let input = "See @fig-plot.\n\n```{r}\n#| label: fig-plot\nplot(1:10)\n```\n";
        let diagnostics = parse_and_lint(input, Flavor::Quarto);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn reports_missing_caption_for_bookdown_figure_crossref() {
        let input = "Figure \\@ref(fig:plot).\n\n```{r}\n#| label: plot\nplot(1:10)\n```\n";
        let diagnostics = parse_and_lint(input, Flavor::RMarkdown);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "figure-crossref-captions");
        assert!(diagnostics[0].message.contains("@fig:plot"));
    }

    #[test]
    fn accepts_captioned_figure_crossref() {
        let input = "See @fig-plot.\n\n```{r}\n#| label: fig-plot\n#| fig-cap: \"A plot\"\nplot(1:10)\n```\n";
        let diagnostics = parse_and_lint(input, Flavor::Quarto);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn ignores_non_figure_crossrefs() {
        let input = "See @tbl-results.\n\n```{r}\n#| label: tbl-results\nplot(1:10)\n```\n";
        let diagnostics = parse_and_lint(input, Flavor::Quarto);
        assert!(diagnostics.is_empty());
    }
}
