//! Utilities for calculating list item indentation.
//!
//! This module centralizes the logic for determining how list items should be indented,
//! including marker alignment, spacing, and checkbox handling.

/// Detailed indentation information for a list item.
///
/// This struct encapsulates all the spacing components needed to properly indent
/// a list item and its continuation lines.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ListItemIndent {
    /// Leading spaces before the marker (for right-alignment)
    pub marker_padding: usize,
    /// Width of the marker itself (e.g., "-" is 1, "10." is 3)
    pub marker_width: usize,
    /// Spaces after the marker (1 normally, 2 for uppercase letter markers like "A.")
    pub spaces_after: usize,
    /// Width of task checkbox if present ("[x] " = 4)
    pub checkbox_width: usize,
    /// Whether the `four_space_rule` extension is active (changes where
    /// continuation/nested content is placed).
    four_space_rule: bool,
    /// One tab stop in columns — the indent unit used under `four_space_rule`.
    tab_width: usize,
}

impl ListItemIndent {
    /// Calculate the total content indent (where the first-line text starts).
    /// This is the sum of all spacing components.
    pub fn content_offset(&self) -> usize {
        self.marker_padding + self.marker_width + self.spaces_after + self.checkbox_width
    }

    /// Column (relative to the list's base indent) at which nested *blocks* are
    /// placed: nested lists, and continuation paragraphs/blocks after a blank
    /// line. This is the list *content column* — just past the marker and its
    /// trailing space — and deliberately excludes `checkbox_width`. A task
    /// checkbox (`[ ] `) is inline content, not part of the block indent:
    /// aligning children past it (col 6 for `- [ ] `) lands them at/under the
    /// 4-space code-block threshold, silently reinterpreting a sublist as an
    /// indented code block or lazy paragraph text.
    ///
    /// Note this is *not* where an item's own lazy-wrapped paragraph text goes:
    /// that lines up under the first-line content (`content_offset`, past the
    /// checkbox), which is safe because lazy continuation can't be reinterpreted
    /// as a block. Under the `four_space_rule` extension this is instead a flat
    /// one-tab-width per nesting level, decoupled from marker width — so a wide
    /// `100.` marker still nests its children at four columns, not six.
    pub fn continuation_offset(&self) -> usize {
        if self.four_space_rule {
            self.tab_width
        } else {
            self.marker_padding + self.marker_width + self.spaces_after
        }
    }

    /// Calculate the hanging indent (including base list indent).
    /// This is used for continuation paragraphs, nested lists, and wrapping.
    pub fn hanging_indent(&self, base_indent: usize) -> usize {
        base_indent + self.continuation_offset()
    }
}

/// Calculate indentation for a list item given its marker and context.
///
/// # Arguments
/// * `marker` - The list marker string (e.g., "-", "1.", "a)", "(i)")
/// * `max_marker_width` - Maximum marker width in this list (for alignment), 0 if no alignment
/// * `has_checkbox` - Whether this item has a task checkbox ([x] or [ ])
/// * `four_space_rule` - Whether the `four_space_rule` extension is active
/// * `tab_width` - One tab stop in columns (the four-space-rule indent unit)
pub(super) fn calculate_list_item_indent(
    marker: &str,
    max_marker_width: usize,
    has_checkbox: bool,
    four_space_rule: bool,
    tab_width: usize,
) -> ListItemIndent {
    // Determine if this marker should be right-aligned
    let is_alignable = is_alignable_marker(marker);

    // Calculate marker padding (for right-alignment)
    let marker_padding = if is_alignable && max_marker_width > 0 {
        max_marker_width.saturating_sub(marker.len())
    } else {
        0
    };

    // Spaces after marker (minimum 1, or 2 for uppercase letter markers like "A.")
    // The `four_space_rule` extension does NOT change marker spacing: the marker
    // keeps its normal trailing space and only continuation/nested content moves
    // to a flat tab stop (see `continuation_offset`). Padding the marker so the
    // first line also lands on that tab stop (pandoc's `-   a` / `1.  a` style)
    // is a separate formatting choice we may add as its own knob later.
    let spaces_after = if marker.len() == 2
        && marker.starts_with(|c: char| c.is_ascii_uppercase())
        && marker.ends_with('.')
    {
        2
    } else {
        1
    };

    // Task checkbox width ("[x] " = 4 characters)
    let checkbox_width = if has_checkbox { 4 } else { 0 };

    ListItemIndent {
        marker_padding,
        marker_width: marker.len(),
        spaces_after,
        checkbox_width,
        four_space_rule,
        tab_width,
    }
}

/// Check if a marker should be right-aligned.
///
/// Right-alignable markers include:
/// - Roman numerals (i., iv., IX., etc.)
/// - Alphabetic markers (a., z., A., Z.)
///
/// Not alignable:
/// - Bullet markers (-, *, +)
/// - Example lists ((@) or (@label))
/// - Numeric markers (unless they contain letters)
pub(super) fn is_alignable_marker(marker: &str) -> bool {
    // Don't align example lists (they start with '(@')
    if marker.starts_with("(@") {
        return false;
    }

    // Don't align bullet lists
    if marker.len() == 1 && (marker == "-" || marker == "*" || marker == "+") {
        return false;
    }

    // Align all ordered list styles with letters or Roman numerals:
    // Period: a., i., A., I.
    // Right-paren: a), i), A), I)
    // Parens: (a), (i), (A), (I)
    if marker.len() < 2 {
        return false;
    }

    // Check if the marker contains a letter (handles all three delimiter styles)
    marker.chars().any(|c| c.is_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bullet_marker_no_alignment() {
        let indent = calculate_list_item_indent("-", 0, false, false, 4);
        assert_eq!(indent.marker_padding, 0);
        assert_eq!(indent.marker_width, 1);
        assert_eq!(indent.spaces_after, 1);
        assert_eq!(indent.checkbox_width, 0);
        assert_eq!(indent.content_offset(), 2);
        assert_eq!(indent.continuation_offset(), 2);
    }

    #[test]
    fn test_numeric_marker_no_alignment() {
        let indent = calculate_list_item_indent("1.", 0, false, false, 4);
        assert_eq!(indent.marker_padding, 0);
        assert_eq!(indent.marker_width, 2);
        assert_eq!(indent.spaces_after, 1);
        assert_eq!(indent.content_offset(), 3);
    }

    #[test]
    fn test_roman_numeral_with_alignment() {
        // "i." in a list where max width is 4 (e.g., "iv.")
        let indent = calculate_list_item_indent("i.", 4, false, false, 4);
        assert_eq!(indent.marker_padding, 2); // Pad "i." to align with "iv."
        assert_eq!(indent.marker_width, 2);
        assert_eq!(indent.spaces_after, 1);
        assert_eq!(indent.content_offset(), 5); // 2 + 2 + 1
    }

    #[test]
    fn test_uppercase_letter_marker() {
        let indent = calculate_list_item_indent("A.", 0, false, false, 4);
        assert_eq!(indent.marker_padding, 0);
        assert_eq!(indent.marker_width, 2);
        assert_eq!(indent.spaces_after, 2); // Uppercase letters get 2 spaces
        assert_eq!(indent.content_offset(), 4);
    }

    #[test]
    fn test_task_checkbox() {
        let indent = calculate_list_item_indent("-", 0, true, false, 4);
        assert_eq!(indent.checkbox_width, 4);
        assert_eq!(indent.content_offset(), 6); // 1 + 1 + 4
    }

    #[test]
    fn four_space_rule_keeps_bullet_marker_spacing_but_nests_at_tab_stop() {
        // The marker keeps its single trailing space (`- a`); only continuation
        // and nested content move to the flat tab stop.
        let indent = calculate_list_item_indent("-", 0, false, true, 4);
        assert_eq!(indent.spaces_after, 1);
        assert_eq!(indent.content_offset(), 2); // first-line content stays at col 2
        assert_eq!(indent.continuation_offset(), 4);
        assert_eq!(indent.hanging_indent(0), 4);
        assert_eq!(indent.hanging_indent(4), 8); // depth 2 = 8 columns
    }

    #[test]
    fn four_space_rule_keeps_ordered_marker_spacing_but_nests_at_tab_stop() {
        let indent = calculate_list_item_indent("1.", 0, false, true, 4);
        assert_eq!(indent.spaces_after, 1);
        assert_eq!(indent.content_offset(), 3); // first-line content stays at col 3
        assert_eq!(indent.continuation_offset(), 4);
    }

    #[test]
    fn four_space_rule_wide_marker_nests_at_flat_tab_stop() {
        // "100." overhangs the tab stop; children still sit at a flat tab-width
        // (col 4), never rounded up.
        let indent = calculate_list_item_indent("100.", 0, false, true, 4);
        assert_eq!(indent.spaces_after, 1);
        assert_eq!(indent.content_offset(), 5);
        assert_eq!(indent.continuation_offset(), 4);
        assert_eq!(indent.hanging_indent(0), 4);
    }

    #[test]
    fn test_is_alignable_marker() {
        // Should align
        assert!(is_alignable_marker("i."));
        assert!(is_alignable_marker("iv."));
        assert!(is_alignable_marker("a."));
        assert!(is_alignable_marker("z."));
        assert!(is_alignable_marker("A."));
        assert!(is_alignable_marker("(a)"));
        assert!(is_alignable_marker("i)"));

        // Should not align
        assert!(!is_alignable_marker("-"));
        assert!(!is_alignable_marker("*"));
        assert!(!is_alignable_marker("+"));
        assert!(!is_alignable_marker("1."));
        assert!(!is_alignable_marker("10."));
        assert!(!is_alignable_marker("(@)"));
        assert!(!is_alignable_marker("(@label)"));
    }

    #[test]
    fn test_hanging_indent() {
        let indent = calculate_list_item_indent("i.", 4, false, false, 4);
        assert_eq!(indent.hanging_indent(0), 5); // No base indent
        assert_eq!(indent.hanging_indent(2), 7); // With base indent of 2
    }
}
