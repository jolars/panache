pub mod code_block_collector;
pub mod diagnostics;
pub mod external_linters;
pub mod external_linters_sync;
pub mod rules;
pub mod runner;

pub use diagnostics::{Diagnostic, Fix, Location, Severity};
pub use rules::{Rule, RuleRegistry};
pub use runner::LintRunner;

use crate::config::Config;
use crate::syntax::SyntaxNode;

/// Lint a document and return diagnostics (built-in rules only).
pub fn lint(tree: &SyntaxNode, input: &str, config: &Config) -> Vec<Diagnostic> {
    let registry = default_registry();
    let runner = LintRunner::new(registry);
    runner.run(tree, input, config)
}

/// Lint a document with external linters (sync version for CLI).
pub fn lint_with_external_sync(tree: &SyntaxNode, input: &str, config: &Config) -> Vec<Diagnostic> {
    let registry = default_registry();
    let runner = LintRunner::new(registry);
    runner.run_with_external_linters_sync(tree, input, config)
}

/// Lint a document with external linters (async version for LSP).
pub async fn lint_with_external(
    tree: &SyntaxNode,
    input: &str,
    config: &Config,
) -> Vec<Diagnostic> {
    let registry = default_registry();
    let runner = LintRunner::new(registry);
    runner.run_with_external_linters(tree, input, config).await
}

/// Create the default rule registry with all built-in rules.
fn default_registry() -> RuleRegistry {
    let mut registry = RuleRegistry::new();
    registry.register(Box::new(rules::heading_hierarchy::HeadingHierarchyRule));
    registry
}
