//! Fenced code block parsing utilities.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::blockquotes::count_blockquote_markers;
use super::utils::strip_leading_spaces;

/// Represents the type of code block based on its info string syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeBlockType {
    /// Display-only block with shortcut syntax: ```python
    DisplayShortcut { language: String },
    /// Display-only block with explicit Pandoc syntax: ```{.python}
    DisplayExplicit { classes: Vec<String> },
    /// Executable chunk (Quarto/RMarkdown): ```{python}
    Executable { language: String },
    /// Raw block for specific output format: ```{=html}
    Raw { format: String },
    /// No language specified: ```
    Plain,
}

/// Parsed attributes from a code block info string.
#[derive(Debug, Clone, PartialEq)]
pub struct InfoString {
    pub raw: String,
    pub block_type: CodeBlockType,
    pub attributes: Vec<(String, Option<String>)>, // key-value pairs
}

impl InfoString {
    /// Parse an info string into structured attributes.
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();

        if trimmed.is_empty() {
            return InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::Plain,
                attributes: Vec::new(),
            };
        }

        // Check if it starts with '{' - explicit attribute block
        if let Some(stripped) = trimmed.strip_prefix('{')
            && let Some(content) = stripped.strip_suffix('}')
        {
            return Self::parse_explicit(raw, content);
        }

        // Check for mixed form: python {.numberLines}
        if let Some(brace_start) = trimmed.find('{') {
            let language = trimmed[..brace_start].trim();
            if !language.is_empty() && !language.contains(char::is_whitespace) {
                let attr_part = &trimmed[brace_start..];
                if let Some(stripped) = attr_part.strip_prefix('{')
                    && let Some(content) = stripped.strip_suffix('}')
                {
                    let attrs = Self::parse_attributes(content);
                    return InfoString {
                        raw: raw.to_string(),
                        block_type: CodeBlockType::DisplayShortcut {
                            language: language.to_string(),
                        },
                        attributes: attrs,
                    };
                }
            }
        }

        // Otherwise, it's a shortcut form (just the language name)
        // Only take the first word as language
        let language = trimmed.split_whitespace().next().unwrap_or(trimmed);
        InfoString {
            raw: raw.to_string(),
            block_type: CodeBlockType::DisplayShortcut {
                language: language.to_string(),
            },
            attributes: Vec::new(),
        }
    }

    fn parse_explicit(raw: &str, content: &str) -> Self {
        // Check for raw attribute FIRST: {=format}
        // The content should start with '=' and have only alphanumeric chars after
        let trimmed_content = content.trim();
        if let Some(format_name) = trimmed_content.strip_prefix('=') {
            // Validate format name: alphanumeric only, no spaces
            if !format_name.is_empty()
                && format_name.chars().all(|c| c.is_alphanumeric())
                && !format_name.contains(char::is_whitespace)
            {
                return InfoString {
                    raw: raw.to_string(),
                    block_type: CodeBlockType::Raw {
                        format: format_name.to_string(),
                    },
                    attributes: Vec::new(),
                };
            }
        }

        let attrs = Self::parse_attributes(content);

        // First non-ID, non-attribute token determines if it's executable or display
        let mut first_lang_token = None;
        for (key, val) in attrs.iter() {
            if val.is_none() && !key.starts_with('#') {
                first_lang_token = Some(key.as_str());
                break;
            }
        }

        let first_token = first_lang_token.unwrap_or("");

        if first_token.starts_with('.') {
            // Display block: {.python} or {.haskell .numberLines}
            let classes: Vec<String> = attrs
                .iter()
                .filter(|(k, v)| k.starts_with('.') && v.is_none())
                .map(|(k, _)| k[1..].to_string())
                .collect();

            let non_class_attrs: Vec<(String, Option<String>)> = attrs
                .into_iter()
                .filter(|(k, _)| !k.starts_with('.') || k.contains('='))
                .collect();

            InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::DisplayExplicit { classes },
                attributes: non_class_attrs,
            }
        } else if !first_token.is_empty() && !first_token.starts_with('#') {
            // Executable chunk: {python} or {r}
            // Find the index of the language token
            let lang_index = attrs.iter().position(|(k, _)| k == first_token).unwrap();

            InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::Executable {
                    language: first_token.to_string(),
                },
                attributes: attrs
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| *i != lang_index)
                    .map(|(_, attr)| attr)
                    .collect(),
            }
        } else {
            // Just attributes, no language
            InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::Plain,
                attributes: attrs,
            }
        }
    }

    fn parse_attributes(content: &str) -> Vec<(String, Option<String>)> {
        let mut attrs = Vec::new();
        let mut chars = content.chars().peekable();

        while chars.peek().is_some() {
            // Skip whitespace
            while chars.peek() == Some(&' ') || chars.peek() == Some(&'\t') {
                chars.next();
            }

            if chars.peek().is_none() {
                break;
            }

            // Read key
            let mut key = String::new();
            while let Some(&ch) = chars.peek() {
                if ch == '=' || ch == ' ' || ch == '\t' {
                    break;
                }
                key.push(ch);
                chars.next();
            }

            if key.is_empty() {
                break;
            }

            // Skip whitespace
            while chars.peek() == Some(&' ') || chars.peek() == Some(&'\t') {
                chars.next();
            }

            // Check for value
            if chars.peek() == Some(&'=') {
                chars.next(); // consume '='

                // Skip whitespace after '='
                while chars.peek() == Some(&' ') || chars.peek() == Some(&'\t') {
                    chars.next();
                }

                // Read value (might be quoted)
                let value = if chars.peek() == Some(&'"') {
                    chars.next(); // consume opening quote
                    let mut val = String::new();
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch == '"' {
                            break;
                        }
                        if ch == '\\' {
                            if let Some(&next_ch) = chars.peek() {
                                chars.next();
                                val.push(next_ch);
                            }
                        } else {
                            val.push(ch);
                        }
                    }
                    val
                } else {
                    // Unquoted value
                    let mut val = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch == ' ' || ch == '\t' {
                            break;
                        }
                        val.push(ch);
                        chars.next();
                    }
                    val
                };

                attrs.push((key, Some(value)));
            } else {
                attrs.push((key, None));
            }
        }

        attrs
    }
}

/// Information about a detected code fence opening.
pub(crate) struct FenceInfo {
    pub fence_char: char,
    pub fence_count: usize,
    pub info_string: String,
}

/// Try to detect a fenced code block opening from content.
/// Returns fence info if this is a valid opening fence.
pub(crate) fn try_parse_fence_open(content: &str) -> Option<FenceInfo> {
    let trimmed = strip_leading_spaces(content);

    // Check for fence opening (``` or ~~~)
    let (fence_char, fence_count) = if trimmed.starts_with('`') {
        let count = trimmed.chars().take_while(|&c| c == '`').count();
        ('`', count)
    } else if trimmed.starts_with('~') {
        let count = trimmed.chars().take_while(|&c| c == '~').count();
        ('~', count)
    } else {
        return None;
    };

    if fence_count < 3 {
        return None;
    }

    let info_string_raw = &trimmed[fence_count..];
    // Trim trailing newline and at most one leading space
    let info_string_trimmed = info_string_raw.trim_end_matches('\n');
    let info_string = if let Some(stripped) = info_string_trimmed.strip_prefix(' ') {
        stripped.to_string()
    } else {
        info_string_trimmed.to_string()
    };

    Some(FenceInfo {
        fence_char,
        fence_count,
        info_string,
    })
}

/// Check if a line is a valid closing fence for the given fence info.
pub(crate) fn is_closing_fence(content: &str, fence: &FenceInfo) -> bool {
    let trimmed = strip_leading_spaces(content);

    if !trimmed.starts_with(fence.fence_char) {
        return false;
    }

    let closing_count = trimmed
        .chars()
        .take_while(|&c| c == fence.fence_char)
        .count();

    if closing_count < fence.fence_count {
        return false;
    }

    // Rest of line must be empty
    trimmed[closing_count..].trim().is_empty()
}

/// Parse a fenced code block, consuming lines from the parser.
/// Returns the new position after the code block.
/// Parse a fenced code block, consuming lines from the parser.
/// Returns the new position after the code block.
/// base_indent accounts for container indentation (e.g., footnotes) that should be stripped.
pub(crate) fn parse_fenced_code_block(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    fence: FenceInfo,
    bq_depth: usize,
    base_indent: usize,
) -> usize {
    // Start code block
    builder.start_node(SyntaxKind::CodeBlock.into());

    // Opening fence
    let first_line = lines[start_pos];
    let (_, first_inner) = count_blockquote_markers(first_line);
    // Strip base indent (footnote context)
    let first_stripped = if base_indent > 0 && first_inner.len() >= base_indent {
        &first_inner[base_indent..]
    } else {
        first_inner
    };
    let first_trimmed = strip_leading_spaces(first_stripped);

    builder.start_node(SyntaxKind::CodeFenceOpen.into());
    builder.token(
        SyntaxKind::CodeFenceMarker.into(),
        &first_trimmed[..fence.fence_count],
    );
    if !fence.info_string.is_empty() {
        builder.token(SyntaxKind::CodeInfo.into(), &fence.info_string);
    }
    builder.token(SyntaxKind::NEWLINE.into(), "\n");
    builder.finish_node(); // CodeFenceOpen

    let mut current_pos = start_pos + 1;
    let mut content_lines: Vec<&str> = Vec::new();
    let mut found_closing = false;

    while current_pos < lines.len() {
        let line = lines[current_pos];

        // Strip blockquote markers to get inner content
        let (line_bq_depth, inner) = count_blockquote_markers(line);

        // If blockquote depth decreases, code block ends (we've left the blockquote)
        if line_bq_depth < bq_depth {
            break;
        }

        // Strip base indent (footnote context) from content lines
        let inner_stripped = if base_indent > 0 && inner.len() >= base_indent {
            &inner[base_indent..]
        } else {
            inner
        };

        // Check for closing fence
        if is_closing_fence(inner_stripped, &fence) {
            found_closing = true;
            current_pos += 1;
            break;
        }

        content_lines.push(inner_stripped);
        current_pos += 1;
    }

    // Add content
    if !content_lines.is_empty() {
        builder.start_node(SyntaxKind::CodeContent.into());
        for content_line in content_lines.iter() {
            // Split off trailing newline if present (from split_inclusive)
            let (line_without_newline, has_newline) =
                if let Some(stripped) = content_line.strip_suffix('\n') {
                    (stripped, true)
                } else {
                    (*content_line, false)
                };

            if !line_without_newline.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), line_without_newline);
            }

            if has_newline {
                builder.token(SyntaxKind::NEWLINE.into(), "\n");
            }
        }
        builder.finish_node(); // CodeContent
    }

    // Closing fence (if found)
    if found_closing {
        let closing_line = lines[current_pos - 1];
        let (_, closing_inner) = count_blockquote_markers(closing_line);
        // Strip base indent
        let closing_stripped = if base_indent > 0 && closing_inner.len() >= base_indent {
            &closing_inner[base_indent..]
        } else {
            closing_inner
        };
        let closing_trimmed = strip_leading_spaces(closing_stripped);
        let closing_count = closing_trimmed
            .chars()
            .take_while(|&c| c == fence.fence_char)
            .count();

        builder.start_node(SyntaxKind::CodeFenceClose.into());
        builder.token(
            SyntaxKind::CodeFenceMarker.into(),
            &closing_trimmed[..closing_count],
        );
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
        builder.finish_node(); // CodeFenceClose
    }

    builder.finish_node(); // CodeBlock

    current_pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backtick_fence() {
        let fence = try_parse_fence_open("```python").unwrap();
        assert_eq!(fence.fence_char, '`');
        assert_eq!(fence.fence_count, 3);
        assert_eq!(fence.info_string, "python");
    }

    #[test]
    fn test_tilde_fence() {
        let fence = try_parse_fence_open("~~~").unwrap();
        assert_eq!(fence.fence_char, '~');
        assert_eq!(fence.fence_count, 3);
        assert_eq!(fence.info_string, "");
    }

    #[test]
    fn test_long_fence() {
        let fence = try_parse_fence_open("`````").unwrap();
        assert_eq!(fence.fence_count, 5);
    }

    #[test]
    fn test_two_backticks_invalid() {
        assert!(try_parse_fence_open("``").is_none());
    }

    #[test]
    fn test_closing_fence() {
        let fence = FenceInfo {
            fence_char: '`',
            fence_count: 3,
            info_string: String::new(),
        };
        assert!(is_closing_fence("```", &fence));
        assert!(is_closing_fence("````", &fence));
        assert!(!is_closing_fence("``", &fence));
        assert!(!is_closing_fence("~~~", &fence));
    }

    #[test]
    fn test_info_string_plain() {
        let info = InfoString::parse("");
        assert_eq!(info.block_type, CodeBlockType::Plain);
        assert!(info.attributes.is_empty());
    }

    #[test]
    fn test_info_string_shortcut() {
        let info = InfoString::parse("python");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayShortcut {
                language: "python".to_string()
            }
        );
        assert!(info.attributes.is_empty());
    }

    #[test]
    fn test_info_string_shortcut_with_trailing() {
        let info = InfoString::parse("python extra stuff");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayShortcut {
                language: "python".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_display_explicit() {
        let info = InfoString::parse("{.python}");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayExplicit {
                classes: vec!["python".to_string()]
            }
        );
    }

    #[test]
    fn test_info_string_display_explicit_multiple() {
        let info = InfoString::parse("{.python .numberLines}");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayExplicit {
                classes: vec!["python".to_string(), "numberLines".to_string()]
            }
        );
    }

    #[test]
    fn test_info_string_executable() {
        let info = InfoString::parse("{python}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Executable {
                language: "python".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_executable_with_options() {
        let info = InfoString::parse("{python echo=false warning=true}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Executable {
                language: "python".to_string()
            }
        );
        assert_eq!(info.attributes.len(), 2);
        assert_eq!(
            info.attributes[0],
            ("echo".to_string(), Some("false".to_string()))
        );
        assert_eq!(
            info.attributes[1],
            ("warning".to_string(), Some("true".to_string()))
        );
    }

    #[test]
    fn test_info_string_mixed_shortcut_and_attrs() {
        let info = InfoString::parse("python {.numberLines}");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayShortcut {
                language: "python".to_string()
            }
        );
        assert_eq!(info.attributes.len(), 1);
        assert_eq!(info.attributes[0], (".numberLines".to_string(), None));
    }

    #[test]
    fn test_info_string_mixed_with_key_value() {
        let info = InfoString::parse("python {.numberLines startFrom=\"100\"}");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayShortcut {
                language: "python".to_string()
            }
        );
        assert_eq!(info.attributes.len(), 2);
        assert_eq!(info.attributes[0], (".numberLines".to_string(), None));
        assert_eq!(
            info.attributes[1],
            ("startFrom".to_string(), Some("100".to_string()))
        );
    }

    #[test]
    fn test_info_string_explicit_with_id_and_classes() {
        let info = InfoString::parse("{#mycode .haskell .numberLines startFrom=\"100\"}");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayExplicit {
                classes: vec!["haskell".to_string(), "numberLines".to_string()]
            }
        );
        // Non-class attributes
        let has_id = info.attributes.iter().any(|(k, _)| k == "#mycode");
        let has_start = info
            .attributes
            .iter()
            .any(|(k, v)| k == "startFrom" && v == &Some("100".to_string()));
        assert!(has_id);
        assert!(has_start);
    }

    #[test]
    fn test_info_string_raw_html() {
        let info = InfoString::parse("{=html}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Raw {
                format: "html".to_string()
            }
        );
        assert!(info.attributes.is_empty());
    }

    #[test]
    fn test_info_string_raw_latex() {
        let info = InfoString::parse("{=latex}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Raw {
                format: "latex".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_raw_openxml() {
        let info = InfoString::parse("{=openxml}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Raw {
                format: "openxml".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_raw_ms() {
        let info = InfoString::parse("{=ms}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Raw {
                format: "ms".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_raw_html5() {
        let info = InfoString::parse("{=html5}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Raw {
                format: "html5".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_raw_not_combined_with_attrs() {
        // If there are other attributes with =format, it should not be treated as raw
        let info = InfoString::parse("{=html .class}");
        // This should NOT be parsed as raw because there's more than one attribute
        assert_ne!(
            info.block_type,
            CodeBlockType::Raw {
                format: "html".to_string()
            }
        );
    }
}
