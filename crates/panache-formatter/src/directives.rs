//! Directive parsing and tracking for ignore comments.
//!
//! This module handles detection and validation of panache directive comments:
//! - `<!-- panache-ignore-start -->` / `<!-- panache-ignore-end -->` - ignore both formatting and linting
//! - `<!-- panache-ignore-format-start -->` / `<!-- panache-ignore-format-end -->` - ignore formatting only
//! - `<!-- panache-ignore-lint-start -->` / `<!-- panache-ignore-lint-end -->` - ignore linting only
//!
//! Future extensibility: The syntax is designed to support rule-specific ignores
//! (e.g., `<!-- panache-ignore-lint heading-hierarchy -->`) though this is not yet implemented.

use crate::syntax::SyntaxNode;

/// Type of ignore directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectiveKind {
    /// Ignore both formatting and linting.
    IgnoreBoth,
    /// Ignore formatting only.
    IgnoreFormat,
    /// Ignore linting only.
    IgnoreLint,
}

/// A parsed directive with its kind and boundary type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Directive {
    /// Start of an ignore region.
    Start(DirectiveKind),
    /// End of an ignore region.
    End(DirectiveKind),
}

impl DirectiveKind {
    /// Check if this directive kind affects formatting.
    pub fn affects_formatting(self) -> bool {
        matches!(
            self,
            DirectiveKind::IgnoreBoth | DirectiveKind::IgnoreFormat
        )
    }

    /// Check if this directive kind affects linting.
    pub fn affects_linting(self) -> bool {
        matches!(self, DirectiveKind::IgnoreBoth | DirectiveKind::IgnoreLint)
    }
}

/// Parse a comment text to detect panache directives.
///
/// Returns `Some(Directive)` if the comment contains a valid directive,
/// `None` otherwise.
///
/// # Examples
///
/// ```
/// use panache_formatter::directives::{parse_directive, Directive, DirectiveKind};
///
/// assert_eq!(
///     parse_directive("<!-- panache-ignore-start -->"),
///     Some(Directive::Start(DirectiveKind::IgnoreBoth))
/// );
///
/// assert_eq!(
///     parse_directive("<!-- panache-ignore-format-end -->"),
///     Some(Directive::End(DirectiveKind::IgnoreFormat))
/// );
///
/// assert_eq!(parse_directive("<!-- regular comment -->"), None);
/// ```
pub fn parse_directive(comment_text: &str) -> Option<Directive> {
    // Strip HTML comment markers
    let content = comment_text.trim();

    if !content.starts_with("<!--") || !content.ends_with("-->") {
        return None;
    }

    // Extract content between <!-- and -->
    let inner = content[4..content.len() - 3].trim();

    // Check for panache directive prefix
    if !inner.starts_with("panache-ignore") {
        return None;
    }

    // Parse the directive
    match inner {
        "panache-ignore-start" => Some(Directive::Start(DirectiveKind::IgnoreBoth)),
        "panache-ignore-end" => Some(Directive::End(DirectiveKind::IgnoreBoth)),
        "panache-ignore-format-start" => Some(Directive::Start(DirectiveKind::IgnoreFormat)),
        "panache-ignore-format-end" => Some(Directive::End(DirectiveKind::IgnoreFormat)),
        "panache-ignore-lint-start" => Some(Directive::Start(DirectiveKind::IgnoreLint)),
        "panache-ignore-lint-end" => Some(Directive::End(DirectiveKind::IgnoreLint)),
        _ => None, // Unknown directive or future extension
    }
}

/// Track active ignore regions during document traversal.
///
/// Uses a stack to handle nested regions (though in practice, nesting should be validated).
#[derive(Debug, Clone)]
pub struct DirectiveTracker {
    /// Stack of active ignore regions (kind).
    stack: Vec<DirectiveKind>,
}

impl DirectiveTracker {
    /// Create a new tracker with no active regions.
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Process a directive, updating the tracker state.
    ///
    /// Returns `true` if the directive was processed successfully,
    /// `false` if there was a mismatch (e.g., end without start).
    pub fn process_directive(&mut self, directive: &Directive) -> bool {
        match directive {
            Directive::Start(kind) => {
                self.stack.push(*kind);
                true
            }
            Directive::End(kind) => {
                // Check if the top of the stack matches
                if let Some(top) = self.stack.last()
                    && top == kind
                {
                    self.stack.pop();
                    return true;
                }
                // Mismatch or end without start
                false
            }
        }
    }

    /// Check if formatting is currently ignored.
    pub fn is_formatting_ignored(&self) -> bool {
        self.stack.iter().any(|kind| kind.affects_formatting())
    }

    /// Check if linting is currently ignored.
    pub fn is_linting_ignored(&self) -> bool {
        self.stack.iter().any(|kind| kind.affects_linting())
    }

    /// Check if there are any unclosed ignore regions.
    ///
    /// This is useful for detecting mismatched directives at the end of a document.
    pub fn has_unclosed_regions(&self) -> bool {
        !self.stack.is_empty()
    }

    /// Get the kinds of unclosed regions.
    pub fn unclosed_regions(&self) -> Vec<DirectiveKind> {
        self.stack.clone()
    }

    /// Reset the tracker to initial state.
    pub fn reset(&mut self) {
        self.stack.clear();
    }
}

impl Default for DirectiveTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract directive from a COMMENT, HTML_BLOCK, or INLINE_HTML syntax node.
pub fn extract_directive_from_node(node: &SyntaxNode) -> Option<Directive> {
    use crate::syntax::SyntaxKind;

    // HTML comments can be parsed as COMMENT, HTML_BLOCK, or INLINE_HTML
    // depending on context (block-level vs inline) and dialect.
    if node.kind() != SyntaxKind::COMMENT
        && node.kind() != SyntaxKind::HTML_BLOCK
        && node.kind() != SyntaxKind::INLINE_HTML
    {
        return None;
    }

    let text = node.text().to_string();
    parse_directive(&text)
}

/// Collect all inline-html directives nested inside `node`, in document order.
///
/// Returns each (directive, INLINE_HTML node text) pair. Used by the formatter
/// to replay inline directive transitions after a paragraph/plain emits, so
/// the tracker stays in sync when comments inline into a paragraph (the
/// Pandoc-dialect default).
pub fn collect_inline_directives(node: &SyntaxNode) -> Vec<Directive> {
    use crate::syntax::SyntaxKind;

    let mut out = Vec::new();
    for descendant in node.descendants() {
        if descendant.kind() == SyntaxKind::INLINE_HTML
            && let Some(directive) = extract_directive_from_node(&descendant)
        {
            out.push(directive);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_directive_ignore_both() {
        assert_eq!(
            parse_directive("<!-- panache-ignore-start -->"),
            Some(Directive::Start(DirectiveKind::IgnoreBoth))
        );
        assert_eq!(
            parse_directive("<!-- panache-ignore-end -->"),
            Some(Directive::End(DirectiveKind::IgnoreBoth))
        );
    }

    #[test]
    fn test_parse_directive_ignore_format() {
        assert_eq!(
            parse_directive("<!-- panache-ignore-format-start -->"),
            Some(Directive::Start(DirectiveKind::IgnoreFormat))
        );
        assert_eq!(
            parse_directive("<!-- panache-ignore-format-end -->"),
            Some(Directive::End(DirectiveKind::IgnoreFormat))
        );
    }

    #[test]
    fn test_parse_directive_ignore_lint() {
        assert_eq!(
            parse_directive("<!-- panache-ignore-lint-start -->"),
            Some(Directive::Start(DirectiveKind::IgnoreLint))
        );
        assert_eq!(
            parse_directive("<!-- panache-ignore-lint-end -->"),
            Some(Directive::End(DirectiveKind::IgnoreLint))
        );
    }

    #[test]
    fn test_parse_directive_with_whitespace() {
        assert_eq!(
            parse_directive("<!--   panache-ignore-start   -->"),
            Some(Directive::Start(DirectiveKind::IgnoreBoth))
        );
        assert_eq!(
            parse_directive("<!--\npanache-ignore-end\n-->"),
            Some(Directive::End(DirectiveKind::IgnoreBoth))
        );
    }

    #[test]
    fn test_parse_directive_not_directive() {
        assert_eq!(parse_directive("<!-- regular comment -->"), None);
        assert_eq!(parse_directive("<!-- panache-something -->"), None);
        assert_eq!(parse_directive("not a comment"), None);
    }

    #[test]
    fn test_parse_directive_future_extension() {
        // Future syntax for rule-specific ignores should return None for now
        assert_eq!(
            parse_directive("<!-- panache-ignore-lint heading-hierarchy -->"),
            None
        );
    }

    #[test]
    fn test_directive_kind_affects_formatting() {
        assert!(DirectiveKind::IgnoreBoth.affects_formatting());
        assert!(DirectiveKind::IgnoreFormat.affects_formatting());
        assert!(!DirectiveKind::IgnoreLint.affects_formatting());
    }

    #[test]
    fn test_directive_kind_affects_linting() {
        assert!(DirectiveKind::IgnoreBoth.affects_linting());
        assert!(!DirectiveKind::IgnoreFormat.affects_linting());
        assert!(DirectiveKind::IgnoreLint.affects_linting());
    }

    #[test]
    fn test_tracker_basic() {
        let mut tracker = DirectiveTracker::new();

        assert!(!tracker.is_formatting_ignored());
        assert!(!tracker.is_linting_ignored());

        tracker.process_directive(&Directive::Start(DirectiveKind::IgnoreBoth));
        assert!(tracker.is_formatting_ignored());
        assert!(tracker.is_linting_ignored());

        tracker.process_directive(&Directive::End(DirectiveKind::IgnoreBoth));
        assert!(!tracker.is_formatting_ignored());
        assert!(!tracker.is_linting_ignored());
    }

    #[test]
    fn test_tracker_format_only() {
        let mut tracker = DirectiveTracker::new();

        tracker.process_directive(&Directive::Start(DirectiveKind::IgnoreFormat));
        assert!(tracker.is_formatting_ignored());
        assert!(!tracker.is_linting_ignored());
    }

    #[test]
    fn test_tracker_lint_only() {
        let mut tracker = DirectiveTracker::new();

        tracker.process_directive(&Directive::Start(DirectiveKind::IgnoreLint));
        assert!(!tracker.is_formatting_ignored());
        assert!(tracker.is_linting_ignored());
    }

    #[test]
    fn test_tracker_mismatch() {
        let mut tracker = DirectiveTracker::new();

        // End without start
        let result = tracker.process_directive(&Directive::End(DirectiveKind::IgnoreBoth));
        assert!(!result);

        // Mismatched kinds
        tracker.process_directive(&Directive::Start(DirectiveKind::IgnoreFormat));
        let result = tracker.process_directive(&Directive::End(DirectiveKind::IgnoreLint));
        assert!(!result);
    }

    #[test]
    fn test_tracker_nested() {
        let mut tracker = DirectiveTracker::new();

        tracker.process_directive(&Directive::Start(DirectiveKind::IgnoreBoth));
        tracker.process_directive(&Directive::Start(DirectiveKind::IgnoreFormat));

        assert!(tracker.is_formatting_ignored());
        assert!(tracker.is_linting_ignored());

        tracker.process_directive(&Directive::End(DirectiveKind::IgnoreFormat));
        assert!(tracker.is_formatting_ignored()); // Still ignored by IgnoreBoth
        assert!(tracker.is_linting_ignored());

        tracker.process_directive(&Directive::End(DirectiveKind::IgnoreBoth));
        assert!(!tracker.is_formatting_ignored());
        assert!(!tracker.is_linting_ignored());
    }

    #[test]
    fn test_tracker_unclosed_regions() {
        let mut tracker = DirectiveTracker::new();

        assert!(!tracker.has_unclosed_regions());

        tracker.process_directive(&Directive::Start(DirectiveKind::IgnoreBoth));
        assert!(tracker.has_unclosed_regions());

        let unclosed = tracker.unclosed_regions();
        assert_eq!(unclosed.len(), 1);
        assert_eq!(unclosed[0], DirectiveKind::IgnoreBoth);

        tracker.process_directive(&Directive::End(DirectiveKind::IgnoreBoth));
        assert!(!tracker.has_unclosed_regions());
    }

    #[test]
    fn test_tracker_reset() {
        let mut tracker = DirectiveTracker::new();

        tracker.process_directive(&Directive::Start(DirectiveKind::IgnoreBoth));
        tracker.process_directive(&Directive::Start(DirectiveKind::IgnoreFormat));

        tracker.reset();
        assert!(!tracker.has_unclosed_regions());
        assert!(!tracker.is_formatting_ignored());
        assert!(!tracker.is_linting_ignored());
    }
}
