use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::{AstNode, ChunkLabel, ChunkOption, SyntaxKind, SyntaxNode};
use rowan::TextRange;

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

        for chunk_options in tree
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::CHUNK_OPTIONS)
        {
            diagnostics.extend(check_implicit_label_spaces(&chunk_options, input));
            diagnostics.extend(check_explicit_label_spaces(&chunk_options, input));
        }

        diagnostics
    }
}

fn check_implicit_label_spaces(chunk_options: &SyntaxNode, input: &str) -> Vec<Diagnostic> {
    let mut leading_labels = Vec::new();

    for child in chunk_options.children() {
        if let Some(label) = ChunkLabel::cast(child) {
            leading_labels.push(label);
        } else {
            break;
        }
    }

    if leading_labels.len() <= 1 {
        return Vec::new();
    }

    let first = leading_labels
        .first()
        .unwrap()
        .syntax()
        .text_range()
        .start();
    let last = leading_labels.last().unwrap().syntax().text_range().end();
    let label = leading_labels
        .iter()
        .map(|part| part.text())
        .collect::<Vec<_>>()
        .join(" ");
    let location = Location::from_range(TextRange::new(first, last), input);

    vec![Diagnostic::warning(
        location,
        "chunk-label-spaces",
        format!(
            "Chunk label '{}' contains spaces and may break Quarto cross-references; use hyphens or underscores",
            label
        ),
    )]
}

fn check_explicit_label_spaces(chunk_options: &SyntaxNode, input: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for option in chunk_options.children().filter_map(ChunkOption::cast) {
        let Some(key) = option.key() else {
            continue;
        };
        if !key.eq_ignore_ascii_case("label") {
            continue;
        }
        let Some(value) = option.value() else {
            continue;
        };
        if !value.chars().any(char::is_whitespace) {
            continue;
        }

        diagnostics.push(Diagnostic::warning(
            Location::from_node(option.syntax(), input),
            "chunk-label-spaces",
            format!(
                "Chunk label '{}' contains spaces and may break Quarto cross-references; use hyphens or underscores",
                value
            ),
        ));
    }

    diagnostics
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
