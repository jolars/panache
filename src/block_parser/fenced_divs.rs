//! Fenced div parsing utilities.

use super::utils::strip_leading_spaces;

/// Information about a detected div fence opening.
pub(crate) struct DivFenceInfo {
    pub attributes: String,
}

/// Try to detect a fenced div opening from content.
/// Returns div fence info if this is a valid opening fence.
///
/// Opening fences MUST have attributes (or the fences are treated as closing).
/// Format: `::: {.class #id}` or `::: classname` or `::::: {#id} :::::`
pub(crate) fn try_parse_div_fence_open(content: &str) -> Option<DivFenceInfo> {
    let trimmed = strip_leading_spaces(content);

    // Check for fence opening (:::)
    if !trimmed.starts_with(':') {
        return None;
    }

    let colon_count = trimmed.chars().take_while(|&c| c == ':').count();

    if colon_count < 3 {
        return None;
    }

    // Get the part after the colons
    let after_colons = trimmed[colon_count..].trim_start();

    // Check if there are attributes
    // Attributes can be:
    // 1. Curly braces: {.class #id key="value"}
    // 2. Single word (treated as class): classname
    // 3. Attributes followed by more colons (optional): {.class} :::

    let attributes = if after_colons.starts_with('{') {
        // Find the closing brace
        if let Some(close_idx) = after_colons.find('}') {
            after_colons[..=close_idx].to_string()
        } else {
            // Unclosed brace, not valid
            return None;
        }
    } else if after_colons.is_empty() {
        // No attributes, this is a closing fence
        return None;
    } else {
        // Single word or words until optional trailing colons
        let content_before_colons = after_colons.trim_end_matches(':').trim_end();

        if content_before_colons.is_empty() {
            // Only colons, no attributes
            return None;
        }

        // Take the first word as the class name
        content_before_colons.split_whitespace().next()?.to_string()
    };

    Some(DivFenceInfo { attributes })
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

    // Rest of line must be empty (only colons are allowed)
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
    fn test_closing_fence_no_attributes() {
        let line = ":::";
        assert!(try_parse_div_fence_open(line).is_none());
        assert!(is_div_closing_fence(line));
    }

    #[test]
    fn test_closing_fence_many_colons() {
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
