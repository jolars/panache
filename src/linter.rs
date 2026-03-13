pub mod code_block_collector;
pub mod diagnostics;
#[cfg(not(target_arch = "wasm32"))]
pub mod external_linters;
#[cfg(not(target_arch = "wasm32"))]
pub mod external_linters_sync;
pub mod metadata_diagnostics;
pub(crate) mod offsets;
pub mod rules;
pub mod runner;

pub use diagnostics::{Diagnostic, Fix, Location, Severity};
pub use rules::{Rule, RuleRegistry};
pub use runner::LintRunner;

use crate::config::Config;
use crate::syntax::SyntaxNode;

/// Lint a document and return diagnostics (built-in rules only).
pub fn lint(tree: &SyntaxNode, input: &str, config: &Config) -> Vec<Diagnostic> {
    let registry = default_registry(config);
    let runner = LintRunner::new(registry);
    runner.run(tree, input, config)
}

/// Lint a document with external linters (sync version for CLI).
#[cfg(not(target_arch = "wasm32"))]
pub fn lint_with_external_sync(tree: &SyntaxNode, input: &str, config: &Config) -> Vec<Diagnostic> {
    let registry = default_registry(config);
    let runner = LintRunner::new(registry);
    runner.run_with_external_linters_sync(tree, input, config, None)
}

/// Lint a document with external linters (async version for LSP).
#[cfg(all(not(target_arch = "wasm32"), feature = "lsp"))]
pub async fn lint_with_external(
    tree: &SyntaxNode,
    input: &str,
    config: &Config,
) -> Vec<Diagnostic> {
    let registry = default_registry(config);
    let runner = LintRunner::new(registry);
    runner
        .run_with_external_linters(tree, input, config, None)
        .await
}

pub fn lint_with_metadata(
    tree: &SyntaxNode,
    input: &str,
    config: &Config,
    metadata: Option<&crate::metadata::DocumentMetadata>,
) -> Vec<Diagnostic> {
    let registry = default_registry(config);
    let runner = LintRunner::new(registry);
    runner.run_with_metadata(tree, input, config, metadata)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn lint_with_external_sync_and_metadata(
    tree: &SyntaxNode,
    input: &str,
    config: &Config,
    metadata: Option<&crate::metadata::DocumentMetadata>,
) -> Vec<Diagnostic> {
    let registry = default_registry(config);
    let runner = LintRunner::new(registry);
    runner.run_with_external_linters_sync(tree, input, config, metadata)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "lsp"))]
pub async fn lint_with_external_and_metadata(
    tree: &SyntaxNode,
    input: &str,
    config: &Config,
    metadata: Option<&crate::metadata::DocumentMetadata>,
) -> Vec<Diagnostic> {
    let registry = default_registry(config);
    let runner = LintRunner::new(registry);
    runner
        .run_with_external_linters(tree, input, config, metadata)
        .await
}

/// Create the default rule registry with all built-in rules.
fn default_registry(config: &Config) -> RuleRegistry {
    let mut registry = RuleRegistry::new();
    if config.lint.is_rule_enabled("heading-hierarchy") {
        registry.register(Box::new(rules::heading_hierarchy::HeadingHierarchyRule));
    }
    if config.lint.is_rule_enabled("duplicate-reference-labels") {
        registry.register(Box::new(
            rules::duplicate_references::DuplicateReferencesRule,
        ));
    }
    if config.lint.is_rule_enabled("undefined-references") {
        registry.register(Box::new(
            rules::undefined_references::UndefinedReferencesRule,
        ));
    }
    if config.lint.is_rule_enabled("citation-keys") {
        registry.register(Box::new(rules::citation_keys::CitationKeysRule));
    }
    if config.lint.is_rule_enabled("chunk-label-spaces") {
        registry.register(Box::new(rules::chunk_label_spaces::ChunkLabelSpacesRule));
    }
    if config.lint.is_rule_enabled("missing-chunk-labels") {
        registry.register(Box::new(
            rules::missing_chunk_labels::MissingChunkLabelsRule,
        ));
    }
    registry
}
