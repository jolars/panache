//! Project-wide symbol aggregate shared by the cross-file reference rules
//! (`undefined-references`, `unused-definitions`, `undefined-anchor`).
//!
//! Each of those rules needs to know what labels, anchors, and usages exist
//! across *every* document in the same Quarto/bookdown project, not just the one
//! being linted. Rebuilding that per rule (and per sibling file) meant parsing
//! every project document `rules x files` times on throwaway databases --- an
//! O(n^2) blow-up on large projects.
//!
//! This module centralizes the per-document extraction so both the
//! salsa-memoized production path (`crate::salsa::project_symbol_index`) and the
//! filesystem fallback used by standalone `check_tree` callers fold documents
//! through the *same* logic. The production path folds each document's
//! already-memoized parse + [`SymbolUsageIndex`], so each project document is
//! parsed and indexed once per batch rather than once per rule per file.

use std::collections::HashSet;
use std::path::Path;

use crate::config::{Config, Flavor};
use crate::salsa::SymbolUsageIndex;
use crate::syntax::{
    AstNode, Citation, FootnoteReference, ImageLink, Link, MystTarget, SyntaxKind, SyntaxNode,
    UnresolvedReference,
};
use crate::utils::{
    heading_has_explicit_id, implicit_heading_ids, normalize_anchor_label, normalize_label,
};

/// Definition-side labels a document contributes: the targets that references,
/// footnotes, and cross-references may resolve against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DefinitionLabels {
    pub reference_labels: HashSet<String>,
    pub footnote_ids: HashSet<String>,
    pub crossref_labels: HashSet<String>,
    pub heading_text_labels: HashSet<String>,
}

impl DefinitionLabels {
    pub fn merge(&mut self, other: &DefinitionLabels) {
        self.reference_labels
            .extend(other.reference_labels.iter().cloned());
        self.footnote_ids.extend(other.footnote_ids.iter().cloned());
        self.crossref_labels
            .extend(other.crossref_labels.iter().cloned());
        self.heading_text_labels
            .extend(other.heading_text_labels.iter().cloned());
    }
}

/// Usage-side labels a document contributes: the reference/footnote labels it
/// actually consumes (so a definition used in any project document counts).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageLabels {
    pub reference_labels: HashSet<String>,
    pub footnote_ids: HashSet<String>,
}

/// The aggregated definition labels, anchors, and usage labels for a set of
/// project documents. Built once per project (memoized in salsa) and shared by
/// every cross-file rule.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectSymbolIndex {
    pub definitions: DefinitionLabels,
    pub anchors: HashSet<String>,
    pub usage: UsageLabels,
}

impl ProjectSymbolIndex {
    /// Fold one document's contribution (definitions, anchors, usages) into the
    /// aggregate, reading its parsed tree and memoized symbol index.
    pub fn fold_document(
        &mut self,
        tree: &SyntaxNode,
        symbol_index: &SymbolUsageIndex,
        config: &Config,
    ) {
        extend_labels_from_tree(&mut self.definitions, tree, config, symbol_index);
        extend_anchors(&mut self.anchors, tree, config, symbol_index);
        let usage = collect_usage_labels(tree.descendants());
        self.usage.reference_labels.extend(usage.reference_labels);
        self.usage.footnote_ids.extend(usage.footnote_ids);
    }

    /// Union another aggregate (typically one document's memoized contribution)
    /// into this one.
    pub fn extend(&mut self, other: &ProjectSymbolIndex) {
        self.definitions.merge(&other.definitions);
        self.anchors.extend(other.anchors.iter().cloned());
        self.usage
            .reference_labels
            .extend(other.usage.reference_labels.iter().cloned());
        self.usage
            .footnote_ids
            .extend(other.usage.footnote_ids.iter().cloned());
    }
}

/// Build the aggregate for a project by reading every sibling document and its
/// includes off disk. Used only by non-salsa callers (`check_tree` and unit
/// tests); the production lint path uses the salsa-memoized
/// [`crate::salsa::project_symbol_index`] instead so nothing is re-parsed per
/// file. `doc_path` (the document being linted) is skipped: rules fold their own
/// document locally.
pub fn build_from_fs(
    project_root: &Path,
    doc_path: &Path,
    config: &Config,
    is_bookdown: bool,
) -> ProjectSymbolIndex {
    let mut aggregate = ProjectSymbolIndex::default();
    let db = crate::salsa::SalsaDb::default();
    let mut pending = crate::includes::find_project_documents(project_root, config, is_bookdown);
    let mut visited = HashSet::new();
    while let Some(path) = pending.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        if let Ok(other_input) = std::fs::read_to_string(&path) {
            let tree = crate::parser::parse(&other_input, Some(config.clone()));
            let resolution = crate::includes::collect_includes(
                &tree,
                &other_input,
                path.parent().unwrap_or(project_root),
                Some(project_root),
                config,
            );
            pending.extend(resolution.includes.into_iter().map(|include| include.path));
            // The caller owns the current document's symbols, but its include
            // edges are still needed to reach partials that only it includes.
            if path == doc_path {
                continue;
            }
            let index = crate::salsa::symbol_usage_index_from_tree_with_flavor(
                &db,
                &tree,
                &config.extensions,
                config.flavor,
            );
            aggregate.fold_document(&tree, &index, config);
        }
    }
    aggregate
}

pub fn extend_labels_from_tree(
    labels: &mut DefinitionLabels,
    tree: &SyntaxNode,
    config: &Config,
    symbol_index: &SymbolUsageIndex,
) {
    labels.reference_labels.extend(
        symbol_index
            .reference_definition_entries()
            .map(|(label, _)| label.clone())
            .filter(|label| !label.is_empty()),
    );
    labels.footnote_ids.extend(
        symbol_index
            .footnote_definition_entries()
            .map(|(id, _)| id.clone())
            .filter(|id| !id.is_empty()),
    );
    labels.crossref_labels.extend(
        symbol_index
            .crossref_declaration_entries()
            .map(|(label, _)| label.clone())
            .filter(|label| !label.is_empty()),
    );

    if config.extensions.implicit_header_references && config.extensions.auto_identifiers {
        labels.heading_text_labels.extend(
            symbol_index
                .heading_label_entries()
                .map(|(label, _)| label.clone())
                .filter(|label| !label.is_empty()),
        );
    }

    if config.extensions.bookdown_references && config.extensions.auto_identifiers {
        labels
            .crossref_labels
            .extend(collect_implicit_heading_ids(tree, config));
    }
}

fn collect_implicit_heading_ids(tree: &SyntaxNode, config: &Config) -> HashSet<String> {
    implicit_heading_ids(tree, &config.extensions, config.flavor)
        .into_iter()
        .map(|entry| entry.id)
        .collect()
}

pub fn extend_anchors(
    anchors: &mut HashSet<String>,
    tree: &SyntaxNode,
    config: &Config,
    symbol_index: &SymbolUsageIndex,
) {
    anchors.extend(
        symbol_index
            .crossref_declaration_entries()
            .map(|(label, _)| label.clone())
            .filter(|label| !label.is_empty()),
    );

    if config.extensions.myst_targets {
        anchors.extend(
            tree.descendants()
                .filter_map(MystTarget::cast)
                .filter_map(|target| target.label())
                .map(|label| normalize_anchor_label(&label))
                .filter(|label| !label.is_empty()),
        );
    }

    if config.extensions.auto_identifiers {
        for entry in implicit_heading_ids(tree, &config.extensions, config.flavor) {
            if config.flavor != Flavor::VitePress && heading_has_explicit_id(&entry.heading) {
                continue;
            }
            if entry.id.is_empty() {
                continue;
            }
            anchors.insert(entry.id);
        }
    }

    if config.extensions.citations {
        for citation in tree.descendants().filter_map(Citation::cast) {
            for key in citation.key_texts() {
                if key.is_empty() {
                    continue;
                }
                anchors.insert(format!("ref-{key}"));
            }
        }
    }
}

/// Collect reference/footnote usage labels from a stream of candidate nodes.
///
/// Driven off pre-bucketed nodes for the local document (one shared walk) and
/// off `tree.descendants()` for project sibling files; both feed the same
/// per-node classifiers so the logic stays single-sourced.
pub fn collect_usage_labels(nodes: impl Iterator<Item = SyntaxNode>) -> UsageLabels {
    let mut reference_labels: HashSet<String> = HashSet::new();
    let mut footnote_ids: HashSet<String> = HashSet::new();

    for node in nodes {
        match node.kind() {
            SyntaxKind::LINK => {
                if let Some(label) = Link::cast(node).and_then(usage_label_from_link) {
                    reference_labels.insert(label);
                }
            }
            // Reference-style images (`![alt][label]`, collapsed `![label][]`,
            // shortcut `![label]`) resolve to `IMAGE_LINK` rather than `LINK`,
            // so they count as usages of the label they reference too.
            SyntaxKind::IMAGE_LINK => {
                if let Some(label) = ImageLink::cast(node).and_then(usage_label_from_image) {
                    reference_labels.insert(label);
                }
            }
            // Bracket-shape patterns whose label didn't resolve as a refdef
            // still count as a usage of the label they reference — so a
            // `[GitHub]` shortcut counts as using the `[github]:` definition
            // even if that definition lives in another file.
            SyntaxKind::UNRESOLVED_REFERENCE => {
                if let Some(label) =
                    UnresolvedReference::cast(node).and_then(usage_label_from_unresolved)
                {
                    reference_labels.insert(label);
                }
            }
            SyntaxKind::FOOTNOTE_REFERENCE => {
                if let Some(footnote) = FootnoteReference::cast(node) {
                    let id = normalize_label(&footnote.id());
                    if !id.is_empty() {
                        footnote_ids.insert(id);
                    }
                }
            }
            _ => {}
        }
    }

    UsageLabels {
        reference_labels,
        footnote_ids,
    }
}

fn usage_label_from_link(link: Link) -> Option<String> {
    if link
        .syntax()
        .ancestors()
        .any(|ancestor| ancestor.kind() == SyntaxKind::REFERENCE_DEFINITION)
    {
        return None;
    }
    if link.dest().is_some() {
        return None;
    }
    if let Some(link_ref) = link.reference() {
        let label = normalize_label(&link_ref.label());
        if !label.is_empty() {
            return Some(label);
        }
    }
    // Match on the raw label source, not rendered text: a shortcut label may
    // parse as inline structure (e.g. a code span `` [`insta`] ``) that
    // `text_content` drops, which would fail to match the definition's label.
    link.text()
        .map(|text| normalize_label(&text.raw_label()))
        .filter(|label| !label.is_empty())
}

fn usage_label_from_image(image: ImageLink) -> Option<String> {
    if image
        .syntax()
        .ancestors()
        .any(|ancestor| ancestor.kind() == SyntaxKind::REFERENCE_DEFINITION)
    {
        return None;
    }
    if image.dest().is_some() {
        return None;
    }
    if let Some(link_ref) = image.reference() {
        let label = normalize_label(&link_ref.label());
        if !label.is_empty() {
            return Some(label);
        }
    }
    image
        .alt()
        .map(|alt| normalize_label(&alt.raw_label()))
        .filter(|label| !label.is_empty())
}

fn usage_label_from_unresolved(unresolved: UnresolvedReference) -> Option<String> {
    if let Some(label) = unresolved.label() {
        let normalized = normalize_label(&label);
        if !normalized.is_empty() {
            return Some(normalized);
        }
    }
    let normalized = normalize_label(&unresolved.raw_label());
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}
