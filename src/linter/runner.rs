use std::collections::HashSet;

use crate::config::Config;
#[cfg(not(target_arch = "wasm32"))]
use crate::linter::code_block_collector::concatenate_with_blanks_and_mapping;
use crate::linter::diagnostics::Diagnostic;
#[cfg(not(target_arch = "wasm32"))]
use crate::linter::external_linters::ExternalLinterRegistry;
#[cfg(all(not(target_arch = "wasm32"), feature = "lsp"))]
use crate::linter::external_linters::run_linter;
#[cfg(not(target_arch = "wasm32"))]
use crate::linter::external_linters::{find_missing_linter_commands, log_missing_linter_commands};
use crate::linter::index::LintIndex;
use crate::linter::rules::LintContext;
use crate::linter::rules::RuleRegistry;
use crate::syntax::{SyntaxKind, SyntaxNode};
#[cfg(not(target_arch = "wasm32"))]
use crate::utils::collect_code_blocks;

pub struct LintRunner {
    registry: RuleRegistry,
    #[cfg(not(target_arch = "wasm32"))]
    external_linters: ExternalLinterRegistry,
}

impl LintRunner {
    pub fn new(registry: RuleRegistry) -> Self {
        Self {
            registry,
            #[cfg(not(target_arch = "wasm32"))]
            external_linters: ExternalLinterRegistry::new(),
        }
    }

    pub fn run(&self, tree: &SyntaxNode, input: &str, config: &Config) -> Vec<Diagnostic> {
        self.run_with_metadata(tree, input, config, None)
    }

    pub fn run_with_metadata(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // One shared walk for every registered rule: bucket the nodes/tokens
        // they collectively care about (and compute ignore regions in the same
        // pass) instead of each rule re-walking the whole tree.
        let mut want_kinds: HashSet<SyntaxKind> = HashSet::new();
        let mut want_tokens = false;
        for rule in self.registry.rules() {
            want_kinds.extend(rule.node_interests().iter().copied());
            want_tokens |= rule.wants_text_tokens();
        }
        let index = LintIndex::build(tree, &want_kinds, want_tokens);
        let ignored_ranges = index.ignored_ranges();

        let cx = LintContext {
            tree,
            input,
            config,
            metadata,
            index: &index,
        };

        // Run built-in rules
        for rule in self.registry.rules() {
            log::debug!("Running lint rule: {}", rule.name());
            let rule_diagnostics = rule.check(&cx);
            log::debug!(
                "Rule {} found {} diagnostic(s)",
                rule.name(),
                rule_diagnostics.len()
            );

            // Filter out diagnostics in ignored ranges
            for diagnostic in rule_diagnostics {
                let byte_offset: usize = diagnostic.location.range.start().into();
                let is_ignored = ignored_ranges
                    .iter()
                    .any(|(start, end)| byte_offset >= *start && byte_offset < *end);

                log::debug!(
                    "Diagnostic at byte {}: is_ignored={}",
                    byte_offset,
                    is_ignored
                );

                if !is_ignored {
                    diagnostics.push(diagnostic);
                }
            }
        }

        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
        diagnostics
    }

    /// Run external linters on code blocks (async version for LSP).
    #[cfg(all(not(target_arch = "wasm32"), feature = "lsp"))]
    pub async fn run_with_external_linters(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = self.run_with_metadata(tree, input, config, metadata);

        // If no external linters configured, return early
        if config.linters.is_empty() {
            return diagnostics;
        }

        let missing_linter_commands = find_missing_linter_commands(
            config.linters.values().map(String::as_str),
            &self.external_linters,
        );
        log_missing_linter_commands(&missing_linter_commands);

        // Collect code blocks by language
        let code_blocks = collect_code_blocks(tree, input);

        // Run external linters for configured languages
        for (language, linter_name) in &config.linters {
            let Some(linter_info) = self.external_linters.get(linter_name) else {
                log::warn!(
                    "Skipping unknown external linter '{}' configured for language '{}'",
                    linter_name,
                    language
                );
                continue;
            };
            if missing_linter_commands.contains(linter_info.command) {
                continue;
            }

            if !self
                .external_linters
                .supports_language(linter_name, language)
                .unwrap_or(false)
            {
                log::warn!(
                    "Skipping external linter '{}' for unsupported language '{}'; supported languages: {}",
                    linter_name,
                    language,
                    linter_info.supported_languages.join(", ")
                );
                continue;
            }

            if let Some(blocks) = code_blocks.get(language) {
                if blocks.is_empty() {
                    continue;
                }

                log::debug!(
                    "Running external linter '{}' for {} code blocks in language '{}'",
                    linter_name,
                    blocks.len(),
                    language
                );

                // Concatenate blocks with line preservation and get mappings
                let concatenated_result = concatenate_with_blanks_and_mapping(blocks);

                // Run the linter with mapping info
                match run_linter(
                    linter_name,
                    language,
                    &concatenated_result.content,
                    input,
                    &self.external_linters,
                    Some(&concatenated_result.mappings),
                )
                .await
                {
                    Ok(external_diagnostics) => {
                        log::debug!(
                            "External linter '{}' found {} diagnostic(s)",
                            linter_name,
                            external_diagnostics.len()
                        );
                        diagnostics.extend(external_diagnostics);
                    }
                    Err(e) => {
                        log::warn!("External linter '{}' failed: {}", linter_name, e);
                        // Continue with other linters - don't fail the whole lint operation
                    }
                }
            }
        }

        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
        diagnostics
    }

    /// Run external linters on code blocks (sync version for CLI).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run_with_external_linters_sync(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = self.run_with_metadata(tree, input, config, metadata);

        // If no external linters configured, return early
        if config.linters.is_empty() {
            return diagnostics;
        }

        let missing_linter_commands = find_missing_linter_commands(
            config.linters.values().map(String::as_str),
            &self.external_linters,
        );
        log_missing_linter_commands(&missing_linter_commands);

        // Collect code blocks by language
        let code_blocks = collect_code_blocks(tree, input);

        // Run external linters for configured languages
        for (language, linter_name) in &config.linters {
            let Some(linter_info) = self.external_linters.get(linter_name) else {
                log::warn!(
                    "Skipping unknown external linter '{}' configured for language '{}'",
                    linter_name,
                    language
                );
                continue;
            };
            if missing_linter_commands.contains(linter_info.command) {
                continue;
            }

            if !self
                .external_linters
                .supports_language(linter_name, language)
                .unwrap_or(false)
            {
                log::warn!(
                    "Skipping external linter '{}' for unsupported language '{}'; supported languages: {}",
                    linter_name,
                    language,
                    linter_info.supported_languages.join(", ")
                );
                continue;
            }

            if let Some(blocks) = code_blocks.get(language) {
                if blocks.is_empty() {
                    continue;
                }

                log::debug!(
                    "Running external linter '{}' for {} code blocks in language '{}'",
                    linter_name,
                    blocks.len(),
                    language
                );

                // Concatenate blocks with line preservation and get mappings
                let concatenated_result = concatenate_with_blanks_and_mapping(blocks);

                // Run the linter (sync version) with mapping info
                match crate::linter::external_linters_sync::run_linter_sync(
                    linter_name,
                    language,
                    &concatenated_result.content,
                    input,
                    &self.external_linters,
                    Some(&concatenated_result.mappings),
                ) {
                    Ok(external_diagnostics) => {
                        log::debug!(
                            "External linter '{}' found {} diagnostic(s)",
                            linter_name,
                            external_diagnostics.len()
                        );
                        diagnostics.extend(external_diagnostics);
                    }
                    Err(e) => {
                        log::warn!("External linter '{}' failed: {}", linter_name, e);
                        // Continue with other linters - don't fail the whole lint operation
                    }
                }
            }
        }

        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
        diagnostics
    }
}
