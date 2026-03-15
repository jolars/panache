use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::{SyntaxKind, SyntaxNode};

pub struct EmojiAliasesRule;

impl Rule for EmojiAliasesRule {
    fn name(&self) -> &str {
        "unknown-emoji-alias"
    }

    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        _metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        if !config.extensions.emoji {
            return Vec::new();
        }

        let mut diagnostics = Vec::new();

        for node in tree
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::EMOJI)
        {
            let raw = node.to_string();
            let Some(alias) = raw.strip_prefix(':').and_then(|s| s.strip_suffix(':')) else {
                continue;
            };

            if emojis::get_by_shortcode(alias).is_none() {
                diagnostics.push(Diagnostic::warning(
                    Location::from_node(&node, input),
                    "unknown-emoji-alias",
                    format!("Unknown emoji alias '{}'", raw),
                ));
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_lint(input: &str, emoji_enabled: bool) -> Vec<Diagnostic> {
        let mut config = Config::default();
        config.extensions.emoji = emoji_enabled;
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = EmojiAliasesRule;
        rule.check(&tree, input, &config, None)
    }

    #[test]
    fn accepts_known_alias() {
        let diagnostics = parse_and_lint("Hello :smile:", true);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn warns_on_unknown_alias() {
        let diagnostics = parse_and_lint("Hello :not-a-real-emoji:", true);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "unknown-emoji-alias");
        assert_eq!(diagnostics[0].location.line, 1);
    }

    #[test]
    fn does_not_run_when_emoji_extension_disabled() {
        let diagnostics = parse_and_lint("Hello :not-a-real-emoji:", false);
        assert!(diagnostics.is_empty());
    }
}
