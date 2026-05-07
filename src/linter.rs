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

pub use diagnostics::{
    Diagnostic, DiagnosticNote, DiagnosticNoteKind, DiagnosticOrigin, Fix, Location, Severity,
};
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
///
/// Rules whose diagnostics can only fire when a specific extension/flavor is
/// active are gated at registration time. The rule bodies themselves still
/// early-out defensively, but skipping registration avoids the per-rule
/// dispatch + tree-walk entry cost on each lint invocation. This pays off on
/// large flat-Markdown corpora where most rules would otherwise run for
/// nothing.
fn default_registry(config: &Config) -> RuleRegistry {
    use crate::config::Flavor;

    let mut registry = RuleRegistry::new();
    let ext = &config.extensions;
    let flavor_has_chunks = matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown);

    if config.lint.is_rule_enabled("heading-hierarchy") {
        registry.register(Box::new(rules::heading_hierarchy::HeadingHierarchyRule));
    }
    if ext.footnotes && config.lint.is_rule_enabled("adjacent-footnote-refs") {
        registry.register(Box::new(
            rules::adjacent_footnote_refs::AdjacentFootnoteRefsRule,
        ));
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
    if config.lint.is_rule_enabled("undefined-anchor") {
        registry.register(Box::new(rules::undefined_anchor::UndefinedAnchorRule));
    }
    if config.lint.is_rule_enabled("unused-definitions") {
        registry.register(Box::new(rules::unused_definitions::UnusedDefinitionsRule));
    }
    if ext.citations && config.lint.is_rule_enabled("citation-keys") {
        registry.register(Box::new(rules::citation_keys::CitationKeysRule));
    }
    if ext.fenced_code_attributes && config.lint.is_rule_enabled("chunk-label-spaces") {
        registry.register(Box::new(rules::chunk_label_spaces::ChunkLabelSpacesRule));
    }
    if flavor_has_chunks && config.lint.is_rule_enabled("missing-chunk-labels") {
        registry.register(Box::new(
            rules::missing_chunk_labels::MissingChunkLabelsRule,
        ));
    }
    if flavor_has_chunks && config.lint.is_rule_enabled("figure-crossref-captions") {
        registry.register(Box::new(
            rules::figure_crossref_captions::FigureCrossrefCaptionsRule,
        ));
    }
    if ext.emoji && config.lint.is_rule_enabled("unknown-emoji-alias") {
        registry.register(Box::new(rules::emoji_aliases::EmojiAliasesRule));
    }
    if config.lint.is_rule_enabled("html-entities") {
        registry.register(Box::new(rules::html_entities::HtmlEntitiesRule));
    }
    if ext.fenced_divs && config.lint.is_rule_enabled("stray-fenced-div-markers") {
        registry.register(Box::new(
            rules::stray_fenced_div_markers::StrayFencedDivMarkersRule,
        ));
    }
    registry
}
