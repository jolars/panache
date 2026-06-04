use crate::config::Config;
use crate::linter::diagnostics::Diagnostic;
use crate::syntax::SyntaxNode;

pub mod adjacent_footnote_refs;
pub mod chunk_label_spaces;
pub mod citation_keys;
pub mod crossref_as_link_target;
pub mod duplicate_references;
pub mod emoji_aliases;
pub mod empty_list_item;
pub mod figure_crossref_captions;
pub mod footnote_ref_in_footnote_def;
pub mod heading_eaten_attrs;
pub mod heading_hierarchy;
pub mod heading_strip_comments_residue;
pub mod html_entities;
pub mod link_text_is_url;
pub mod math_content;
pub mod missing_chunk_labels;
pub mod stray_fenced_div_markers;
pub mod undefined_anchor;
pub mod undefined_references;
pub mod unused_definitions;

pub trait Rule {
    fn name(&self) -> &str;
    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic>;
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
