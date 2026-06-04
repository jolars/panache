use std::collections::{HashMap, HashSet};

use crate::directives::{Directive, DirectiveTracker, extract_directive_from_node};
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// Result of a single shared pre-order walk of the document CST.
///
/// Historically every lint rule walked the whole tree itself via
/// `tree.descendants()`, so a lint pass did one full Preorder traversal *per
/// rule*. `LintIndex` collapses that to one traversal: nodes whose kind any
/// registered rule cares about are bucketed by [`SyntaxKind`], and each rule
/// then iterates its (small) bucket via [`LintContext::nodes`] instead of
/// re-walking the tree. Token-scanning rules read [`LintIndex::text_tokens`].
///
/// The same walk also computes the lint-ignore regions that used to require a
/// second standalone `tree.preorder()` pass in the runner.
pub struct LintIndex {
    nodes_by_kind: HashMap<SyntaxKind, Vec<SyntaxNode>>,
    text_tokens: Vec<SyntaxToken>,
    ignored_ranges: Vec<(usize, usize)>,
}

impl LintIndex {
    /// Walk `tree` once, bucketing nodes whose kind is in `want_kinds` and
    /// (when `want_tokens`) collecting `TEXT` tokens. Buckets preserve Preorder
    /// order, identical to `tree.descendants()`, so rules relying on document
    /// order are unaffected.
    pub fn build(tree: &SyntaxNode, want_kinds: &HashSet<SyntaxKind>, want_tokens: bool) -> Self {
        let mut nodes_by_kind: HashMap<SyntaxKind, Vec<SyntaxNode>> = HashMap::new();
        let mut text_tokens = Vec::new();

        let mut tracker = DirectiveTracker::new();
        let mut ignored_ranges = Vec::new();
        let mut current_ignore_start: Option<usize> = None;

        for event in tree.preorder_with_tokens() {
            let rowan::WalkEvent::Enter(element) = event else {
                continue;
            };
            match element {
                rowan::NodeOrToken::Node(node) => {
                    let kind = node.kind();
                    if want_kinds.contains(&kind) {
                        nodes_by_kind.entry(kind).or_default().push(node.clone());
                    }
                    track_ignore_region(
                        &node,
                        &mut tracker,
                        &mut ignored_ranges,
                        &mut current_ignore_start,
                    );
                }
                rowan::NodeOrToken::Token(token) => {
                    if want_tokens && token.kind() == SyntaxKind::TEXT {
                        text_tokens.push(token);
                    }
                }
            }
        }

        // Unclosed ignore region extends to end of document.
        if let Some(start) = current_ignore_start {
            log::debug!("Unclosed ignore region from byte {}", start);
            ignored_ranges.push((start, usize::MAX));
        }
        log::debug!("Total ignored ranges: {:?}", ignored_ranges);

        Self {
            nodes_by_kind,
            text_tokens,
            ignored_ranges,
        }
    }

    /// Nodes of `kind` collected during the walk, in document order. Returns an
    /// empty slice when no rule requested `kind` or none were present.
    pub fn nodes(&self, kind: SyntaxKind) -> &[SyntaxNode] {
        self.nodes_by_kind
            .get(&kind)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// All `TEXT` tokens, in document order (only populated when a rule sets
    /// [`Rule::wants_text_tokens`](crate::linter::rules::Rule::wants_text_tokens)).
    pub fn text_tokens(&self) -> &[SyntaxToken] {
        &self.text_tokens
    }

    /// Byte ranges suppressed by `panache-ignore` directives.
    pub fn ignored_ranges(&self) -> &[(usize, usize)] {
        &self.ignored_ranges
    }
}

/// Track entry/exit of lint-ignore regions for a single node. Folded out of the
/// runner's old `build_ignored_ranges`; the directive state machine is byte-for-
/// byte identical, just driven off the shared walk's `Enter(Node)` events.
fn track_ignore_region(
    node: &SyntaxNode,
    tracker: &mut DirectiveTracker,
    ignored_ranges: &mut Vec<(usize, usize)>,
    current_ignore_start: &mut Option<usize>,
) {
    let Some(directive) = extract_directive_from_node(node) else {
        return;
    };
    tracker.process_directive(&directive);

    if matches!(directive, Directive::Start(_))
        && tracker.is_linting_ignored()
        && current_ignore_start.is_none()
    {
        let start: usize = node.text_range().end().into();
        *current_ignore_start = Some(start);
        log::debug!("Ignore region starts at byte {}", start);
    } else if matches!(directive, Directive::End(_))
        && !tracker.is_linting_ignored()
        && let Some(start) = *current_ignore_start
    {
        let end: usize = node.text_range().start().into();
        log::debug!(
            "Ignore region ends at byte {}, adding range ({}, {})",
            end,
            start,
            end
        );
        ignored_ranges.push((start, end));
        *current_ignore_start = None;
    }
}
