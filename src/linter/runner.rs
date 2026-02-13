use crate::config::Config;
#[cfg(not(target_arch = "wasm32"))]
use crate::linter::code_block_collector::{collect_code_blocks, concatenate_with_blanks};
use crate::linter::diagnostics::Diagnostic;
#[cfg(not(target_arch = "wasm32"))]
use crate::linter::external_linters::{ExternalLinterRegistry, run_linter};
use crate::linter::rules::RuleRegistry;
use crate::syntax::SyntaxNode;

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
        let mut diagnostics = Vec::new();

        // Run built-in rules
        for rule in self.registry.rules() {
            log::debug!("Running lint rule: {}", rule.name());
            let rule_diagnostics = rule.check(tree, input, config);
            log::debug!(
                "Rule {} found {} diagnostic(s)",
                rule.name(),
                rule_diagnostics.len()
            );
            diagnostics.extend(rule_diagnostics);
        }

        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
        diagnostics
    }

    /// Run external linters on code blocks (async version for LSP).
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn run_with_external_linters(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = self.run(tree, input, config);

        // If no external linters configured, return early
        if config.linters.is_empty() {
            return diagnostics;
        }

        // Collect code blocks by language
        let code_blocks = collect_code_blocks(tree, input);

        // Run external linters for configured languages
        for (language, linter_name) in &config.linters {
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

                // Concatenate blocks with line preservation
                let concatenated = concatenate_with_blanks(blocks);

                // Run the linter
                match run_linter(linter_name, &concatenated, &self.external_linters).await {
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
    ) -> Vec<Diagnostic> {
        let mut diagnostics = self.run(tree, input, config);

        // If no external linters configured, return early
        if config.linters.is_empty() {
            return diagnostics;
        }

        // Collect code blocks by language
        let code_blocks = collect_code_blocks(tree, input);

        // Run external linters for configured languages
        for (language, linter_name) in &config.linters {
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

                // Concatenate blocks with line preservation
                let concatenated = concatenate_with_blanks(blocks);

                // Run the linter (sync version)
                match crate::linter::external_linters_sync::run_linter_sync(
                    linter_name,
                    &concatenated,
                    &self.external_linters,
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
