//! Code block and chunk AST node wrappers.

use super::ast::support;
use super::{
    AstNode, ChunkLabel, ChunkOption, HashpipeYamlPreamble, PanacheLanguage, SyntaxKind, SyntaxNode,
};

pub struct CodeBlock(SyntaxNode);

impl AstNode for CodeBlock {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CODE_BLOCK
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl CodeBlock {
    pub fn info(&self) -> Option<CodeInfo> {
        self.0.descendants().find_map(CodeInfo::cast)
    }

    pub fn language(&self) -> Option<String> {
        self.info()
            .and_then(|info| info.language())
            .filter(|language| !language.is_empty())
    }

    pub fn content_text(&self) -> String {
        self.0
            .children()
            .find(|child| child.kind() == SyntaxKind::CODE_CONTENT)
            .map(|child| child.text().to_string())
            .unwrap_or_default()
    }

    pub fn is_executable_chunk(&self) -> bool {
        self.info().is_some_and(|info| info.is_executable())
    }

    pub fn is_display_code_block(&self) -> bool {
        self.language().is_some() && !self.is_executable_chunk()
    }

    pub fn hashpipe_yaml_preamble(&self) -> Option<HashpipeYamlPreamble> {
        self.0.descendants().find_map(HashpipeYamlPreamble::cast)
    }

    pub fn hashpipe_chunk_options(&self) -> Vec<ChunkOption> {
        self.hashpipe_yaml_preamble()
            .map(|preamble| {
                preamble
                    .syntax()
                    .descendants()
                    .filter_map(ChunkOption::cast)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn inline_chunk_options(&self) -> Vec<ChunkOption> {
        self.info()
            .map(|info| info.chunk_options().collect())
            .unwrap_or_default()
    }

    pub fn chunk_options(&self) -> Vec<ChunkOption> {
        let mut options = self.inline_chunk_options();
        options.extend(self.hashpipe_chunk_options());
        options
    }

    pub fn chunk_labels(&self) -> Vec<String> {
        let mut labels = Vec::new();

        if let Some(info) = self.info() {
            for node in info.syntax().descendants() {
                if let Some(chunk_label) = ChunkLabel::cast(node) {
                    let text = chunk_label.text();
                    if !text.is_empty() {
                        labels.push(text);
                    }
                }
            }
        }

        for option in self.chunk_options() {
            let Some(key) = option.key() else {
                continue;
            };
            if key.eq_ignore_ascii_case("label")
                && let Some(value) = option.value()
                && !value.is_empty()
            {
                labels.push(value);
            }
        }

        labels
    }

    pub fn has_chunk_option_key_with_nonempty_value(&self, key_name: &str) -> bool {
        self.chunk_options().into_iter().any(|option| {
            option
                .key()
                .is_some_and(|key| key.eq_ignore_ascii_case(key_name))
                && option.value().is_some_and(|value| !value.is_empty())
        })
    }

    pub fn has_chunk_label(&self) -> bool {
        !self.chunk_labels().is_empty()
    }
}

pub struct CodeInfo(SyntaxNode);

impl AstNode for CodeInfo {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CODE_INFO
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl CodeInfo {
    pub fn language(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|child| {
            child.into_token().and_then(|token| {
                (token.kind() == SyntaxKind::CODE_LANGUAGE).then(|| token.text().to_string())
            })
        })
    }

    pub fn is_executable(&self) -> bool {
        support::children::<ChunkOptions>(&self.0).next().is_some()
    }

    pub fn chunk_options(&self) -> impl Iterator<Item = ChunkOption> {
        support::children::<ChunkOptions>(&self.0)
            .flat_map(|chunk_options| chunk_options.options().collect::<Vec<_>>())
    }
}

pub struct ChunkOptions(SyntaxNode);

impl AstNode for ChunkOptions {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CHUNK_OPTIONS
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl ChunkOptions {
    pub fn options(&self) -> impl Iterator<Item = ChunkOption> {
        support::children(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Flavor};
    use crate::parse;

    #[test]
    fn code_block_display_shortcut_wrapper() {
        let tree = parse("```python\nprint('hi')\n```\n", None);
        let block = tree
            .descendants()
            .find_map(CodeBlock::cast)
            .expect("code block");

        assert_eq!(block.language().as_deref(), Some("python"));
        assert!(block.is_display_code_block());
        assert!(!block.is_executable_chunk());
        assert!(block.content_text().contains("print('hi')"));
    }

    #[test]
    fn code_block_executable_chunk_wrapper() {
        let config = Config {
            flavor: Flavor::Quarto,
            ..Default::default()
        };
        let tree = parse("```{r, echo=FALSE}\nx <- 1\n```\n", Some(config));
        let block = tree
            .descendants()
            .find_map(CodeBlock::cast)
            .expect("code block");

        assert_eq!(block.language().as_deref(), Some("r"));
        assert!(block.is_executable_chunk());
        assert!(!block.is_display_code_block());

        let info = block.info().expect("code info");
        let keys: Vec<String> = info.chunk_options().filter_map(|opt| opt.key()).collect();
        assert!(keys.contains(&"echo".to_string()));
    }

    #[test]
    fn code_block_hashpipe_preamble_wrapper() {
        let config = Config {
            flavor: Flavor::Quarto,
            ..Default::default()
        };
        let tree = parse(
            "```{python}\n#| echo: false\nprint('hi')\n```\n",
            Some(config),
        );
        let block = tree
            .descendants()
            .find_map(CodeBlock::cast)
            .expect("code block");

        assert!(block.hashpipe_yaml_preamble().is_some());
    }

    #[test]
    fn code_block_collects_chunk_labels_and_options() {
        let config = Config {
            flavor: Flavor::Quarto,
            ..Default::default()
        };
        let tree = parse(
            "```{r chunk_inline, echo=FALSE}\n#| label: chunk_hashpipe\n#| fig-cap: \"Caption\"\n1 + 1\n```\n",
            Some(config),
        );
        let block = tree
            .descendants()
            .find_map(CodeBlock::cast)
            .expect("code block");

        let labels = block.chunk_labels();
        assert!(labels.iter().any(|label| label == "chunk_inline"));
        assert!(labels.iter().any(|label| label == "chunk_hashpipe"));
        assert!(block.has_chunk_label());
        assert!(block.has_chunk_option_key_with_nonempty_value("fig-cap"));
    }
}
