pub mod diagnostics;
pub mod rules;
pub mod runner;

pub use diagnostics::{Diagnostic, Fix, Location, Severity};
pub use rules::{Rule, RuleRegistry};
pub use runner::LintRunner;

use crate::config::Config;
use crate::syntax::SyntaxNode;

/// Lint a document and return diagnostics.
pub fn lint(tree: &SyntaxNode, input: &str, config: &Config) -> Vec<Diagnostic> {
    let registry = default_registry();
    let runner = LintRunner::new(registry);
    runner.run(tree, input, config)
}

/// Create the default rule registry with all built-in rules.
fn default_registry() -> RuleRegistry {
    let mut registry = RuleRegistry::new();
    registry.register(Box::new(rules::heading_hierarchy::HeadingHierarchyRule));
    registry
}
