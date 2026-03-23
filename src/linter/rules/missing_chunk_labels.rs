use crate::config::{Config, Flavor};
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::{AstNode, CodeBlock, SyntaxNode};

pub struct MissingChunkLabelsRule;

impl Rule for MissingChunkLabelsRule {
    fn name(&self) -> &str {
        "missing-chunk-labels"
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

        let mut diagnostics = Vec::new();
        for code_block in tree.descendants().filter_map(CodeBlock::cast) {
            if !code_block.is_executable_chunk() {
                continue;
            }

            let Some(info_node) = code_block.info().map(|info| info.syntax().clone()) else {
                continue;
            };

            if !code_block.chunk_label_entries().is_empty() {
                continue;
            }

            diagnostics.push(Diagnostic::warning(
                Location::from_node(&info_node, input),
                "missing-chunk-labels",
                "Executable code chunk has no label; add `#| label: ...`".to_string(),
            ));
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config {
            flavor: Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = MissingChunkLabelsRule;
        rule.check(&tree, input, &config, None)
    }

    #[test]
    fn reports_executable_chunk_without_label() {
        let diagnostics = parse_and_lint("```{r}\n1 + 1\n```\n");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "missing-chunk-labels");
    }

    #[test]
    fn accepts_inline_label() {
        let diagnostics = parse_and_lint("```{r, label=chunk-one}\n1 + 1\n```\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn accepts_hashpipe_label() {
        let diagnostics = parse_and_lint("```{r}\n#| label: chunk-one\n1 + 1\n```\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn ignores_display_blocks() {
        let diagnostics = parse_and_lint("```r\n1 + 1\n```\n");
        assert!(diagnostics.is_empty());
    }
}
