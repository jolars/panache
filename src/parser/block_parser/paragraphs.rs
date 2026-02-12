//! Paragraph handling utilities.
//!
//! Note: Most paragraph logic is in the main BlockParser since paragraphs
//! are tightly integrated with container handling.

use super::container_stack::{byte_index_at_column, leading_indent};

/// Strip leading whitespace up to the content column.
#[allow(dead_code)]
pub(crate) fn strip_to_content_col(line: &str, target: usize) -> &str {
    if target == 0 {
        return line;
    }
    let (indent_cols, _) = leading_indent(line);
    if indent_cols >= target {
        let idx = byte_index_at_column(line, target);
        &line[idx..]
    } else {
        line.trim_start()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_no_target() {
        assert_eq!(strip_to_content_col("  hello", 0), "  hello");
    }

    #[test]
    fn test_strip_exact_indent() {
        assert_eq!(strip_to_content_col("  hello", 2), "hello");
    }

    #[test]
    fn test_strip_less_indent() {
        assert_eq!(strip_to_content_col(" hello", 2), "hello");
    }
}
