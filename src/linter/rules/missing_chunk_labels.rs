use crate::config::{Config, Flavor};
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::parser::blocks::code_blocks::{CodeBlockType, InfoString};
use crate::syntax::{AstNode, ChunkLabel, ChunkOption, SyntaxKind, SyntaxNode};

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
        for node in tree
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::CODE_BLOCK)
        {
            let Some(info_node) = node
                .children()
                .find(|child| child.kind() == SyntaxKind::CODE_FENCE_OPEN)
                .and_then(|open| {
                    open.children()
                        .find(|child| child.kind() == SyntaxKind::CODE_INFO)
                })
            else {
                continue;
            };

            let info = InfoString::parse(&info_node.text().to_string());
            if !matches!(info.block_type, CodeBlockType::Executable { .. }) {
                continue;
            }

            let has_inline_label = info_node.children().any(|child| {
                if child.kind() != SyntaxKind::CHUNK_OPTIONS {
                    return false;
                }
                child.children().any(|opt_or_label| {
                    ChunkLabel::cast(opt_or_label.clone()).is_some()
                        || ChunkOption::cast(opt_or_label)
                            .and_then(|opt| opt.key())
                            .is_some_and(|key| key.eq_ignore_ascii_case("label"))
                })
            });

            let has_hashpipe_label = node
                .children()
                .find(|child| child.kind() == SyntaxKind::CODE_CONTENT)
                .map(|content| {
                    content.descendants().any(|child| {
                        ChunkOption::cast(child)
                            .and_then(|opt| opt.key())
                            .is_some_and(|key| key.eq_ignore_ascii_case("label"))
                    })
                })
                .unwrap_or(false);

            if has_inline_label || has_hashpipe_label {
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
