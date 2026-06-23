pub mod code_block_collector;
pub mod diagnostics;
#[cfg(not(target_arch = "wasm32"))]
pub mod external_linters;
#[cfg(not(target_arch = "wasm32"))]
pub mod external_linters_sync;
pub mod index;
pub mod metadata_diagnostics;
pub(crate) mod offsets;
pub mod quarto_schema;
pub mod rules;
pub mod runner;
pub(crate) mod yaml_resolve;

pub use diagnostics::{
    Diagnostic, DiagnosticNote, DiagnosticNoteKind, DiagnosticOrigin, Fix, FixSafety, Location,
    Severity,
};
pub use rules::{DiagnosticCode, Requirement, Rule, RuleMeta, RuleRegistry};
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

/// Every built-in rule, in registration order, regardless of config.
///
/// `default_registry` filters this list by config; `builtin_rule_metadata`
/// reads each rule's [`RuleMeta`] off it. Adding a rule means adding one entry
/// here (plus its `impl Rule`), nothing else.
fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(rules::heading_hierarchy::HeadingHierarchyRule),
        Box::new(rules::empty_list_item::EmptyListItemRule),
        Box::new(rules::empty_values::EmptyValuesRule),
        Box::new(rules::consumer_divergence::ConsumerDivergenceRule),
        Box::new(rules::math_content::MathContentRule),
        Box::new(rules::heading_eaten_attrs::HeadingEatenAttrsRule),
        Box::new(rules::heading_strip_comments_residue::HeadingStripCommentsResidueRule),
        Box::new(rules::adjacent_footnote_refs::AdjacentFootnoteRefsRule),
        Box::new(rules::footnote_ref_in_footnote_def::FootnoteRefInFootnoteDefRule),
        Box::new(rules::duplicate_references::DuplicateReferencesRule),
        Box::new(rules::undefined_references::UndefinedReferencesRule),
        Box::new(rules::undefined_anchor::UndefinedAnchorRule),
        Box::new(rules::unused_definitions::UnusedDefinitionsRule),
        Box::new(rules::citation_keys::CitationKeysRule),
        Box::new(rules::crossref_as_link_target::CrossrefAsLinkTargetRule),
        Box::new(rules::chunk_label_spaces::ChunkLabelSpacesRule),
        Box::new(rules::missing_chunk_labels::MissingChunkLabelsRule),
        Box::new(rules::quarto_schema::QuartoSchemaRule),
        Box::new(rules::figure_crossref_captions::FigureCrossrefCaptionsRule),
        Box::new(rules::emoji_aliases::EmojiAliasesRule),
        Box::new(rules::html_entities::HtmlEntitiesRule),
        Box::new(rules::link_text_is_url::LinkTextIsUrlRule),
        Box::new(rules::stray_fenced_div_markers::StrayFencedDivMarkersRule),
    ]
}

/// Metadata for every built-in rule, independent of config. The reference docs
/// (`docs/reference/linter-rules.qmd`) are validated against this in
/// `tests/linter_rules_docs.rs`.
pub fn builtin_rule_metadata() -> Vec<RuleMeta> {
    all_rules().iter().map(|rule| rule.metadata()).collect()
}

/// Create the default rule registry with all built-in rules.
///
/// Each rule declares its config preconditions via [`RuleMeta::requires`] and
/// whether it is on by default via [`RuleMeta::default_on`]; this function is
/// the single consumer of those facts. Rules whose preconditions are not met
/// are not registered: besides matching the documented "Requirements", skipping
/// registration avoids the per-rule dispatch + tree-walk entry cost on each
/// lint invocation (this pays off on large flat-Markdown corpora where most
/// rules would otherwise run for nothing).
fn default_registry(config: &Config) -> RuleRegistry {
    let mut registry = RuleRegistry::new();
    for rule in all_rules() {
        let meta = rule.metadata();
        let enabled = if meta.default_on {
            config.lint.is_rule_enabled(meta.name)
        } else {
            config.lint.is_rule_explicitly_enabled(meta.name)
        };
        if enabled && meta.requires.is_satisfied(config) {
            registry.register(rule);
        }
    }
    registry
}
