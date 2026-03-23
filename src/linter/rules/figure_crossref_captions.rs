use crate::config::{Config, Flavor};
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::{AstNode, CodeBlock, Crossref, SyntaxNode};
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

    for code_block in tree.descendants().filter_map(CodeBlock::cast) {
        let labels: Vec<String> = code_block
            .chunk_labels()
            .into_iter()
            .map(|label| normalize_label(&label))
            .filter(|label| !label.is_empty())
            .collect();

        let has_caption = code_block.has_chunk_option_key_with_nonempty_value("fig-cap")
            || code_block.has_chunk_option_key_with_nonempty_value("fig.cap");

        for label in labels {
            out.entry(label).or_insert(has_caption);
        }
    }

    out
}

fn is_bookdown_figure_crossref(label: &str) -> bool {
    label.starts_with("fig:")
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
