use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::{AstNode, ChunkInfoItem, ChunkLabel, ChunkLabelSource, CodeBlock, SyntaxNode};

pub struct ChunkLabelSpacesRule;

impl Rule for ChunkLabelSpacesRule {
    fn name(&self) -> &str {
        "chunk-label-spaces"
    }

    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        _config: &Config,
        _metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for block in tree.descendants().filter_map(CodeBlock::cast) {
            diagnostics.extend(check_chunk_label_spaces(&block, input));
        }

        diagnostics
    }
}

fn check_chunk_label_spaces(block: &CodeBlock, input: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    diagnostics.extend(check_implicit_label_spaces(block, input));

    for label in block.chunk_label_entries() {
        if label.source() == ChunkLabelSource::InlineLabel {
            continue;
        }
        let value = label.value();
        if !value.chars().any(char::is_whitespace) {
            continue;
        }

        diagnostics.push(Diagnostic::warning(
            Location::from_range(label.value_range(), input),
            "chunk-label-spaces",
            format!(
                "Chunk label '{}' contains spaces and may break Quarto cross-references; use hyphens or underscores",
                value
            ),
        ));
    }

    diagnostics
}

fn check_implicit_label_spaces(block: &CodeBlock, input: &str) -> Vec<Diagnostic> {
    let Some(info) = block.info() else {
        return Vec::new();
    };

    let mut leading_labels: Vec<ChunkLabel> = Vec::new();
    for item in info.chunk_items() {
        match item {
            ChunkInfoItem::Label(label) => leading_labels.push(label),
            ChunkInfoItem::Option(_) => break,
        }
    }

    if leading_labels.len() <= 1 {
        return Vec::new();
    }

    let first = leading_labels
        .first()
        .expect("non-empty labels")
        .range()
        .start();
    let last = leading_labels
        .last()
        .expect("non-empty labels")
        .range()
        .end();
    let value = leading_labels
        .iter()
        .map(|label| label.text())
        .collect::<Vec<_>>()
        .join(" ");

    vec![Diagnostic::warning(
        Location::from_range(rowan::TextRange::new(first, last), input),
        "chunk-label-spaces",
        format!(
            "Chunk label '{}' contains spaces and may break Quarto cross-references; use hyphens or underscores",
            value
        ),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Flavor;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config {
            flavor: Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = ChunkLabelSpacesRule;
        rule.check(&tree, input, &config, None)
    }

    #[test]
    fn reports_implicit_label_with_spaces() {
        let diagnostics = parse_and_lint("```{r several words}\n1 + 1\n```\n");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "chunk-label-spaces");
        assert!(diagnostics[0].message.contains("several words"));
    }

    #[test]
    fn reports_explicit_label_with_spaces() {
        let diagnostics = parse_and_lint("```{r, label=\"several words\"}\n1 + 1\n```\n");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "chunk-label-spaces");
        assert!(diagnostics[0].message.contains("several words"));
    }

    #[test]
    fn accepts_label_without_spaces() {
        let diagnostics = parse_and_lint("```{r several-words}\n1 + 1\n```\n");
        assert!(diagnostics.is_empty());
    }
}
