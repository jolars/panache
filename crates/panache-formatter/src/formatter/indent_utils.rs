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
}

impl ListItemIndent {
    /// Calculate the total content indent (where the actual text starts).
    /// This is the sum of all spacing components.
    pub fn content_offset(&self) -> usize {
        self.marker_padding + self.marker_width + self.spaces_after + self.checkbox_width
    }

    /// Calculate the hanging indent (including base list indent).
    /// This is used for wrapping continuation lines.
    pub fn hanging_indent(&self, base_indent: usize) -> usize {
        base_indent + self.content_offset()
    }
}

/// Calculate indentation for a list item given its marker and context.
///
/// # Arguments
/// * `marker` - The list marker string (e.g., "-", "1.", "a)", "(i)")
/// * `max_marker_width` - Maximum marker width in this list (for alignment), 0 if no alignment
/// * `has_checkbox` - Whether this item has a task checkbox ([x] or [ ])
pub(super) fn calculate_list_item_indent(
    marker: &str,
    max_marker_width: usize,
    has_checkbox: bool,
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
        let indent = calculate_list_item_indent("-", 0, false);
        assert_eq!(indent.marker_padding, 0);
        assert_eq!(indent.marker_width, 1);
        assert_eq!(indent.spaces_after, 1);
        assert_eq!(indent.checkbox_width, 0);
        assert_eq!(indent.content_offset(), 2);
    }

    #[test]
    fn test_numeric_marker_no_alignment() {
        let indent = calculate_list_item_indent("1.", 0, false);
        assert_eq!(indent.marker_padding, 0);
        assert_eq!(indent.marker_width, 2);
        assert_eq!(indent.spaces_after, 1);
        assert_eq!(indent.content_offset(), 3);
    }

    #[test]
    fn test_roman_numeral_with_alignment() {
        // "i." in a list where max width is 4 (e.g., "iv.")
        let indent = calculate_list_item_indent("i.", 4, false);
        assert_eq!(indent.marker_padding, 2); // Pad "i." to align with "iv."
        assert_eq!(indent.marker_width, 2);
        assert_eq!(indent.spaces_after, 1);
        assert_eq!(indent.content_offset(), 5); // 2 + 2 + 1
    }

    #[test]
    fn test_uppercase_letter_marker() {
        let indent = calculate_list_item_indent("A.", 0, false);
        assert_eq!(indent.marker_padding, 0);
        assert_eq!(indent.marker_width, 2);
        assert_eq!(indent.spaces_after, 2); // Uppercase letters get 2 spaces
        assert_eq!(indent.content_offset(), 4);
    }

    #[test]
    fn test_task_checkbox() {
        let indent = calculate_list_item_indent("-", 0, true);
        assert_eq!(indent.checkbox_width, 4);
        assert_eq!(indent.content_offset(), 6); // 1 + 1 + 4
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
        let indent = calculate_list_item_indent("i.", 4, false);
        assert_eq!(indent.hanging_indent(0), 5); // No base indent
        assert_eq!(indent.hanging_indent(2), 7); // With base indent of 2
    }
}
