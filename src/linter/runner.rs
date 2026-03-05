use crate::config::Config;
use crate::directives::DirectiveTracker;
#[cfg(not(target_arch = "wasm32"))]
use crate::linter::code_block_collector::concatenate_with_blanks_and_mapping;
use crate::linter::diagnostics::Diagnostic;
#[cfg(not(target_arch = "wasm32"))]
use crate::linter::external_linters::{ExternalLinterRegistry, run_linter};
use crate::linter::rules::RuleRegistry;
use crate::syntax::SyntaxNode;
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

    /// Build a list of text ranges that should be ignored for linting.
    fn build_ignored_ranges(&self, tree: &SyntaxNode) -> Vec<(usize, usize)> {
        use crate::directives::extract_directive_from_node;

        let mut tracker = DirectiveTracker::new();
        let mut ignored_ranges = Vec::new();
        let mut current_ignore_start: Option<usize> = None;

        // Walk the tree and track ignore regions
        for node in tree.preorder() {
            let node = match node {
                rowan::WalkEvent::Enter(n) => n,
                rowan::WalkEvent::Leave(_) => continue,
            };

            // Check for directive comments
            if let Some(directive) = extract_directive_from_node(&node) {
                tracker.process_directive(&directive);

                // Track when we enter/exit ignore regions
                if matches!(directive, crate::directives::Directive::Start(_))
                    && tracker.is_linting_ignored()
                    && current_ignore_start.is_none()
                {
                    let start: usize = node.text_range().end().into();
                    current_ignore_start = Some(start);
                    log::debug!("Ignore region starts at byte {}", start);
                } else if matches!(directive, crate::directives::Directive::End(_))
                    && !tracker.is_linting_ignored()
                    && let Some(start) = current_ignore_start
                {
                    let end: usize = node.text_range().start().into();
                    log::debug!(
                        "Ignore region ends at byte {}, adding range ({}, {})",
                        end,
                        start,
                        end
                    );
                    ignored_ranges.push((start, end));
                    current_ignore_start = None;
                }
            }
        }

        // Handle unclosed ignore region (extends to end of document)
        if let Some(start) = current_ignore_start {
            log::debug!("Unclosed ignore region from byte {}", start);
            ignored_ranges.push((start, usize::MAX));
        }

        log::debug!("Total ignored ranges: {:?}", ignored_ranges);
        ignored_ranges
    }

    pub fn run_with_metadata(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Build map of ignored ranges
        let ignored_ranges = self.build_ignored_ranges(tree);

        // Run built-in rules
        for rule in self.registry.rules() {
            log::debug!("Running lint rule: {}", rule.name());
            let rule_diagnostics = rule.check(tree, input, config, metadata);
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
    #[cfg(not(target_arch = "wasm32"))]
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

                // Concatenate blocks with line preservation and get mappings
                let concatenated_result = concatenate_with_blanks_and_mapping(blocks);

                // Run the linter with mapping info
                match run_linter(
                    linter_name,
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

                // Concatenate blocks with line preservation and get mappings
                let concatenated_result = concatenate_with_blanks_and_mapping(blocks);

                // Run the linter (sync version) with mapping info
                match crate::linter::external_linters_sync::run_linter_sync(
                    linter_name,
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
