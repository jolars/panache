//! Code block and chunk AST node wrappers.

use super::{
    AstNode, ChunkInfoItem, ChunkLabel, ChunkLabelEntry, ChunkLabelSource, ChunkOption,
    ChunkOptionEntry, ChunkOptionSource, ChunkOptions, HashpipeYamlPreamble, PanacheLanguage,
    SyntaxKind, SyntaxNode, collect_option_entries_from_descendants,
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

    pub fn content_range(&self) -> Option<rowan::TextRange> {
        self.0
            .children()
            .find(|child| child.kind() == SyntaxKind::CODE_CONTENT)
            .map(|child| child.text_range())
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
        self.hashpipe_chunk_option_entries()
            .into_iter()
            .map(ChunkOptionEntry::into_option)
            .collect()
    }

    pub fn inline_chunk_options(&self) -> Vec<ChunkOption> {
        self.inline_chunk_option_entries()
            .into_iter()
            .map(ChunkOptionEntry::into_option)
            .collect()
    }

    pub fn inline_chunk_option_entries(&self) -> Vec<ChunkOptionEntry> {
        self.info()
            .map(|info| {
                info.chunk_options()
                    .map(|option| ChunkOptionEntry::new(option, ChunkOptionSource::InlineInfo))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn hashpipe_chunk_option_entries(&self) -> Vec<ChunkOptionEntry> {
        self.hashpipe_yaml_preamble()
            .map(|preamble| {
                collect_option_entries_from_descendants(
                    preamble.syntax(),
                    ChunkOptionSource::HashpipeYaml,
                )
            })
            .unwrap_or_default()
    }

    pub fn merged_chunk_option_entries(&self) -> Vec<ChunkOptionEntry> {
        fn normalized_key(entry: &ChunkOptionEntry) -> Option<String> {
            entry.key().map(|key| key.trim().to_ascii_lowercase())
        }

        let mut seen_inline_keys = std::collections::HashSet::new();
        let mut merged = self.inline_chunk_option_entries();
        for entry in &merged {
            if let Some(key) = normalized_key(entry) {
                seen_inline_keys.insert(key);
            }
        }

        for entry in self.hashpipe_chunk_option_entries() {
            if normalized_key(&entry).is_some_and(|key| seen_inline_keys.contains(&key)) {
                continue;
            }
            merged.push(entry);
        }

        merged
    }

    pub fn chunk_options(&self) -> Vec<ChunkOption> {
        self.merged_chunk_option_entries()
            .into_iter()
            .map(ChunkOptionEntry::into_option)
            .collect()
    }

    pub fn inline_chunk_options_node(&self) -> Option<ChunkOptions> {
        self.info().and_then(|info| info.chunk_options_node())
    }

    pub fn chunk_label_entries(&self) -> Vec<ChunkLabelEntry> {
        let mut labels = Vec::new();

        if let Some(info) = self.info() {
            for label in info.chunk_labels() {
                let text = label.text();
                if text.is_empty() {
                    continue;
                }
                let range = label.syntax().text_range();
                labels.push(ChunkLabelEntry::new(
                    text,
                    range,
                    range,
                    ChunkLabelSource::InlineLabel,
                ));
            }
        }

        for entry in self.merged_chunk_option_entries() {
            let Some(key) = entry.key() else {
                continue;
            };
            if !key.eq_ignore_ascii_case("label") {
                continue;
            }
            let Some(value) = entry.value() else {
                continue;
            };
            if value.is_empty() {
                continue;
            }
            let value_range = entry
                .value_range()
                .unwrap_or_else(|| entry.option().syntax().text_range());
            labels.push(ChunkLabelEntry::new(
                value,
                entry.option().syntax().text_range(),
                value_range,
                ChunkLabelSource::LabelOption,
            ));
        }

        labels
    }

    pub fn chunk_labels(&self) -> Vec<String> {
        self.chunk_label_entries()
            .into_iter()
            .map(|entry| entry.value().to_string())
            .collect()
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
        self.chunk_options_node().is_some()
    }

    pub fn chunk_options(&self) -> impl Iterator<Item = ChunkOption> {
        self.chunk_options_node()
            .map(|chunk_options| chunk_options.options().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
    }

    pub fn chunk_labels(&self) -> impl Iterator<Item = ChunkLabel> {
        self.chunk_options_node()
            .map(|chunk_options| chunk_options.labels().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
    }

    pub fn chunk_items(&self) -> impl Iterator<Item = ChunkInfoItem> {
        self.chunk_options_node()
            .map(|chunk_options| chunk_options.items().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
    }

    pub fn chunk_options_node(&self) -> Option<ChunkOptions> {
        self.0.children().find_map(ChunkOptions::cast)
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
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
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
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
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
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
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

    #[test]
    fn merged_chunk_options_prefer_inline_over_hashpipe() {
        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse(
            "```{r, label=inline, echo=true}\n#| label: hashpipe\n#| echo: false\n1 + 1\n```\n",
            Some(config),
        );
        let block = tree
            .descendants()
            .find_map(CodeBlock::cast)
            .expect("code block");

        let merged = block.merged_chunk_option_entries();
        let mut labels = merged
            .iter()
            .filter_map(|entry| {
                let key = entry.key()?;
                key.eq_ignore_ascii_case("label").then(|| {
                    (
                        entry.value().unwrap_or_default(),
                        entry.source() == ChunkOptionSource::InlineInfo,
                    )
                })
            })
            .collect::<Vec<_>>();
        labels.sort();
        assert_eq!(labels, vec![("inline".to_string(), true)]);

        let mut echoes = merged
            .iter()
            .filter_map(|entry| {
                let key = entry.key()?;
                key.eq_ignore_ascii_case("echo")
                    .then(|| entry.value().unwrap_or_default())
            })
            .collect::<Vec<_>>();
        echoes.sort();
        assert_eq!(echoes, vec!["true".to_string()]);
    }

    #[test]
    fn chunk_label_entries_include_ranges() {
        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse("```{r chunk_a, label=chunk_b}\n1 + 1\n```\n", Some(config));
        let block = tree
            .descendants()
            .find_map(CodeBlock::cast)
            .expect("code block");

        let labels = block.chunk_label_entries();
        assert_eq!(labels.len(), 2);
        assert!(labels.iter().any(|entry| {
            entry.value() == "chunk_a"
                && entry.source() == ChunkLabelSource::InlineLabel
                && !entry.value_range().is_empty()
        }));
        assert!(labels.iter().any(|entry| {
            entry.value() == "chunk_b"
                && entry.source() == ChunkLabelSource::LabelOption
                && !entry.value_range().is_empty()
        }));
    }
}
