//! Quarto shortcode parsing.
//!
//! Syntax:
//! - Normal: `{{< name args >}}`
//! - Escaped: `{{{< name args >}}}` (displays as `{{< name args >}}` in output)

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse a shortcode starting from the current position.
/// Returns (total_length, content, is_escaped) if successful.
///
/// A shortcode is: {{< content >}} or {{{< content >}}}
/// - Must start with `{{<` or `{{{<`
/// - Must end with matching `>}}` or `>}}}`
/// - Content between markers is preserved as-is
pub(crate) fn try_parse_shortcode(text: &str) -> Option<(usize, String, bool)> {
    let bytes = text.as_bytes();

    // Check if we have enough characters for the opening marker
    if bytes.len() < 4 {
        return None;
    }

    // Check for escaped shortcode first: {{{<
    let (is_escaped, marker_len) = if bytes.len() >= 4
        && bytes[0] == b'{'
        && bytes[1] == b'{'
        && bytes[2] == b'{'
        && bytes[3] == b'<'
    {
        (true, 4)
    } else if bytes[0] == b'{' && bytes[1] == b'{' && bytes[2] == b'<' {
        (false, 3)
    } else {
        return None;
    };

    // Find the closing marker (>}} or >}}})
    let close_marker = if is_escaped { ">}}}" } else { ">}}" };
    let close_marker_len = close_marker.len();

    // Search for the closing marker
    let mut pos = marker_len;
    let mut brace_depth: i32 = 0; // Track nested braces in content

    while pos < text.len() {
        if pos + close_marker_len <= text.len()
            && &text[pos..pos + close_marker_len] == close_marker
            && brace_depth == 0
        {
            // Found matching close marker with correct brace depth
            let content = &text[marker_len..pos];
            let total_len = pos + close_marker_len;
            return Some((total_len, content.to_string(), is_escaped));
        }

        // Track brace depth to handle nested braces in content
        match bytes[pos] {
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }

        pos += 1;
    }

    // No matching close marker found
    None
}

/// Emit a shortcode node
pub(crate) fn emit_shortcode(builder: &mut GreenNodeBuilder, content: &str, is_escaped: bool) {
    builder.start_node(SyntaxKind::SHORTCODE.into());

    // Opening marker
    let open_marker = if is_escaped { "{{{<" } else { "{{<" };
    builder.token(SyntaxKind::SHORTCODE_MARKER_OPEN.into(), open_marker);

    // Content (preserved as-is, formatter will normalize)
    builder.start_node(SyntaxKind::SHORTCODE_CONTENT.into());

    // Emit content as TEXT, preserving all whitespace
    if !content.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), content);
    }

    builder.finish_node(); // ShortcodeContent

    // Closing marker
    let close_marker = if is_escaped { ">}}}" } else { ">}}" };
    builder.token(SyntaxKind::SHORTCODE_MARKER_CLOSE.into(), close_marker);

    builder.finish_node(); // Shortcode
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_shortcode() {
        let result = try_parse_shortcode("{{< meta title >}}");
        assert!(result.is_some());
        let (len, content, is_escaped) = result.unwrap();
        assert_eq!(len, 18);
        assert_eq!(content, " meta title ");
        assert!(!is_escaped);
    }

    #[test]
    fn parses_shortcode_without_spaces() {
        let result = try_parse_shortcode("{{<meta title>}}");
        assert!(result.is_some());
        let (len, content, is_escaped) = result.unwrap();
        assert_eq!(len, 16);
        assert_eq!(content, "meta title");
        assert!(!is_escaped);
    }

    #[test]
    fn parses_shortcode_with_extra_spaces() {
        let result = try_parse_shortcode("{{<  meta  title  >}}");
        assert!(result.is_some());
        let (len, content, _) = result.unwrap();
        assert_eq!(len, 21);
        assert_eq!(content, "  meta  title  ");
    }

    #[test]
    fn parses_shortcode_with_arguments() {
        let result = try_parse_shortcode("{{< video src=\"url\" >}}");
        assert!(result.is_some());
        let (len, content, _) = result.unwrap();
        assert_eq!(len, 23);
        assert_eq!(content, " video src=\"url\" ");
    }

    #[test]
    fn parses_shortcode_with_multiple_arguments() {
        let result = try_parse_shortcode("{{< env VAR \"default\" >}}");
        assert!(result.is_some());
        let (len, content, _) = result.unwrap();
        assert_eq!(len, 25);
        assert_eq!(content, " env VAR \"default\" ");
    }

    #[test]
    fn parses_escaped_shortcode() {
        let result = try_parse_shortcode("{{{< var version >}}}");
        assert!(result.is_some());
        let (len, content, is_escaped) = result.unwrap();
        assert_eq!(len, 21);
        assert_eq!(content, " var version ");
        assert!(is_escaped);
    }

    #[test]
    fn parses_shortcode_with_nested_braces() {
        let result = try_parse_shortcode("{{< meta key={nested} >}}");
        assert!(result.is_some());
        let (len, content, _) = result.unwrap();
        assert_eq!(len, 25);
        assert_eq!(content, " meta key={nested} ");
    }

    #[test]
    fn parses_shortcode_with_dot_notation() {
        let result = try_parse_shortcode("{{< meta author.1 >}}");
        assert!(result.is_some());
        let (len, content, _) = result.unwrap();
        assert_eq!(len, 21);
        assert_eq!(content, " meta author.1 ");
    }

    #[test]
    fn parses_shortcode_with_escaped_dots() {
        let result = try_parse_shortcode(r"{{< meta field\\.with\\.dots >}}");
        assert!(result.is_some());
        let (len, content, _) = result.unwrap();
        assert_eq!(len, 32);
        assert_eq!(content, r" meta field\\.with\\.dots ");
    }

    #[test]
    fn parses_empty_shortcode() {
        let result = try_parse_shortcode("{{< >}}");
        assert!(result.is_some());
        let (len, content, _) = result.unwrap();
        assert_eq!(len, 7);
        assert_eq!(content, " ");
    }

    #[test]
    fn fails_on_unclosed_shortcode() {
        let result = try_parse_shortcode("{{< meta title");
        assert!(result.is_none());
    }

    #[test]
    fn fails_on_mismatched_braces() {
        let result = try_parse_shortcode("{{< meta >}");
        assert!(result.is_none());
    }

    #[test]
    fn fails_on_mismatched_escape_braces() {
        let result = try_parse_shortcode("{{{< meta >}}");
        assert!(result.is_none());
    }

    #[test]
    fn does_not_parse_regular_braces() {
        let result = try_parse_shortcode("{{not a shortcode}}");
        assert!(result.is_none());
    }

    #[test]
    fn handles_shortcode_with_key_value_pairs() {
        let result = try_parse_shortcode("{{< video src=\"url\" width=\"100%\" >}}");
        assert!(result.is_some());
        let (len, content, _) = result.unwrap();
        assert_eq!(len, 36);
        assert_eq!(content, " video src=\"url\" width=\"100%\" ");
    }
}
