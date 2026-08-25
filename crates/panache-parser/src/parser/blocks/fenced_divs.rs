//! Fenced div parsing utilities.

use crate::parser::utils::helpers::strip_leading_spaces;

/// Information about a detected div fence opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DivFenceInfo {
    pub attributes: String,
    pub fence_count: usize,
    /// Indentation (in columns) of the opening fence, measured in the
    /// container-prefix-stripped frame. A closing fence more indented than
    /// this is not a closer (pandoc rule); see `FencedDivCloseParser`.
    pub open_indent_cols: usize,
}

/// Try to detect a fenced div opening from content.
/// Returns div fence info if this is a valid opening fence.
///
/// Opening fences MUST have attributes (or the fences are treated as closing).
/// Format: `::: {.class #id}` or `::: classname` or `::::: {#id} :::::`
pub(crate) fn try_parse_div_fence_open(content: &str) -> Option<DivFenceInfo> {
    let trimmed = strip_leading_spaces(content);

    if !trimmed.starts_with(':') {
        return None;
    }

    let colon_count = trimmed.chars().take_while(|&c| c == ':').count();

    if colon_count < 3 {
        return None;
    }

    let after_colons = trimmed[colon_count..].trim_start();

    let attributes = if after_colons.starts_with('{') {
        let close_idx = after_colons.find('}')?;
        after_colons[..=close_idx].to_string()
    } else if after_colons.is_empty() {
        return None;
    } else {
        let word_end = after_colons
            .find(|c: char| c.is_whitespace() || c == ':')
            .unwrap_or(after_colons.len());
        let (first, rest) = after_colons.split_at(word_end);
        if first.is_empty() {
            return None;
        }

        let trailing = rest.trim();
        if !trailing.is_empty() {
            if trailing.chars().any(|c| c != ':') {
                return None;
            }
            if trailing.len() < 3 {
                return None;
            }
        } else {
            let trailing_colons = after_colons[first.len()..].trim();
            if !trailing_colons.is_empty() {
                if trailing_colons.chars().any(|c| c != ':') {
                    return None;
                }
                if trailing_colons.len() < 3 {
                    return None;
                }
            }
        }

        first.to_string()
    };

    Some(DivFenceInfo {
        attributes,
        fence_count: colon_count,
        open_indent_cols: 0,
    })
}

/// Check if a line is a valid closing fence for a div.
/// Closing fences have NO attributes and at least 3 colons.
pub(crate) fn is_div_closing_fence(content: &str) -> bool {
    let trimmed = strip_leading_spaces(content);

    if !trimmed.starts_with(':') {
        return false;
    }

    let colon_count = trimmed.chars().take_while(|&c| c == ':').count();

    if colon_count < 3 {
        return false;
    }

    trimmed[colon_count..].trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_div_fence_open_with_curly_braces() {
        let line = "::: {.callout-note}";
        let fence = try_parse_div_fence_open(line).unwrap();
        assert_eq!(fence.attributes, "{.callout-note}");
    }

    #[test]
    fn test_parse_div_fence_open_with_class_name() {
        let line = "::: Warning";
        let fence = try_parse_div_fence_open(line).unwrap();
        assert_eq!(fence.attributes, "Warning");
    }

    #[test]
    fn test_parse_div_fence_open_with_trailing_colons() {
        let line = "::::: {#special .sidebar} :::::";
        let fence = try_parse_div_fence_open(line).unwrap();
        assert_eq!(fence.attributes, "{#special .sidebar}");
    }

    #[test]
    fn test_parse_div_fence_open_with_class_name_and_trailing_colons() {
        let line = "::: Warning :::";
        let fence = try_parse_div_fence_open(line).unwrap();
        assert_eq!(fence.attributes, "Warning");
    }

    #[test]
    fn test_opening_fence_empty_attributes() {
        let line = ":::";
        assert!(try_parse_div_fence_open(line).is_none());
        assert!(is_div_closing_fence(line));
    }

    #[test]
    fn test_opening_fence_many_colons_empty_attributes() {
        let line = "::::::::::::::";
        assert!(try_parse_div_fence_open(line).is_none());
        assert!(is_div_closing_fence(line));
    }

    #[test]
    fn test_not_a_fence_too_few_colons() {
        let line = ":: something";
        assert!(try_parse_div_fence_open(line).is_none());
        assert!(!is_div_closing_fence(line));
    }
}
