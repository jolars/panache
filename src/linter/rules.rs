use std::collections::HashSet;

use crate::config::Config;
use crate::linter::diagnostics::Diagnostic;
use crate::linter::index::LintIndex;
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

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

    /// Node kinds this rule wants collected for it by the shared walk. The
    /// runner builds one [`LintIndex`] over the union of every registered
    /// rule's interests; the rule then reads its bucket via
    /// [`LintContext::nodes`] instead of walking the tree itself. Default is
    /// empty (e.g. salsa-index-backed rules that don't bucket-walk at all).
    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[]
    }

    /// Whether this rule scans `TEXT` tokens (read via
    /// [`LintContext::text_tokens`]). Default `false` keeps the token list out
    /// of the shared walk unless some rule needs it.
    fn wants_text_tokens(&self) -> bool {
        false
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic>;

    /// Build a one-off [`LintIndex`] for just this rule's interests and run it.
    /// The runner uses a single shared index instead; this is the convenient
    /// entry point for unit tests and any single-rule caller.
    fn check_tree(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let want_kinds: HashSet<SyntaxKind> = self.node_interests().iter().copied().collect();
        let index = LintIndex::build(tree, &want_kinds, self.wants_text_tokens());
        let cx = LintContext {
            tree,
            input,
            config,
            metadata,
            index: &index,
        };
        self.check(&cx)
    }
}

/// Per-lint-pass inputs handed to every rule's [`Rule::check`]. Bundles the
/// document, its source/config/metadata, and the shared [`LintIndex`] so rules
/// read pre-bucketed nodes instead of re-walking the tree.
pub struct LintContext<'a> {
    pub tree: &'a SyntaxNode,
    pub input: &'a str,
    pub config: &'a Config,
    pub metadata: Option<&'a crate::metadata::DocumentMetadata>,
    pub index: &'a LintIndex,
}

impl LintContext<'_> {
    /// Nodes of `kind` from the shared walk, in document order.
    pub fn nodes(&self, kind: SyntaxKind) -> &[SyntaxNode] {
        self.index.nodes(kind)
    }

    /// All `TEXT` tokens from the shared walk (requires
    /// [`Rule::wants_text_tokens`]).
    pub fn text_tokens(&self) -> &[SyntaxToken] {
        self.index.text_tokens()
    }
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
