use crate::config::Config;
use crate::linter::diagnostics::Diagnostic;
use crate::linter::rules::RuleRegistry;
use crate::syntax::SyntaxNode;

pub struct LintRunner {
    registry: RuleRegistry,
}

impl LintRunner {
    pub fn new(registry: RuleRegistry) -> Self {
        Self { registry }
    }

    pub fn run(&self, tree: &SyntaxNode, input: &str, config: &Config) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

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
}
