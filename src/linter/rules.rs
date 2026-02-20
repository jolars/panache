use crate::config::Config;
use crate::linter::diagnostics::Diagnostic;
use crate::syntax::SyntaxNode;

pub mod duplicate_references;
pub mod heading_hierarchy;

pub trait Rule {
    fn name(&self) -> &str;
    fn check(&self, tree: &SyntaxNode, input: &str, config: &Config) -> Vec<Diagnostic>;
}

pub struct RuleRegistry {
    rules: Vec<Box<dyn Rule>>,
}

impl RuleRegistry {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn register(&mut self, rule: Box<dyn Rule>) {
        self.rules.push(rule);
    }

    pub fn rules(&self) -> &[Box<dyn Rule>] {
        &self.rules
    }
}

impl Default for RuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}
