//! Fenced code block parsing utilities.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::blockquotes::count_blockquote_markers;
use super::utils::{strip_leading_spaces, strip_newline};

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

        // First, do a preliminary parse to determine block type
        // Use chunk options parser (comma-aware) for initial detection
        let prelim_attrs = Self::parse_chunk_options(content);

        // First non-ID, non-attribute token determines if it's executable or display
        let mut first_lang_token = None;
        for (key, val) in prelim_attrs.iter() {
            if val.is_none() && !key.starts_with('#') {
                first_lang_token = Some(key.as_str());
                break;
            }
        }

        let first_token = first_lang_token.unwrap_or("");

        if first_token.starts_with('.') {
            // Display block: {.python} or {.haskell .numberLines}
            // Re-parse with Pandoc-style parser (space-delimited)
            let attrs = Self::parse_pandoc_attributes(content);

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
            // Use chunk options parser (comma-delimited)
            let attrs = Self::parse_chunk_options(content);
            let lang_index = attrs.iter().position(|(k, _)| k == first_token).unwrap();

            // Check if there's a second bareword (implicit label in R/Quarto chunks)
            // Pattern: {r mylabel} is equivalent to {r, label=mylabel}
            let mut has_implicit_label = false;
            let implicit_label_value = if lang_index + 1 < attrs.len() {
                if let (label_key, None) = &attrs[lang_index + 1] {
                    // Second bareword after language
                    has_implicit_label = true;
                    Some(label_key.clone())
                } else {
                    None
                }
            } else {
                None
            };

            let mut final_attrs: Vec<(String, Option<String>)> = attrs
                .into_iter()
                .enumerate()
                .filter(|(i, _)| {
                    // Remove language token
                    if *i == lang_index {
                        return false;
                    }
                    // Remove implicit label token (will be added back explicitly)
                    if has_implicit_label && *i == lang_index + 1 {
                        return false;
                    }
                    true
                })
                .map(|(_, attr)| attr)
                .collect();

            // Add explicit label if we found an implicit one
            if let Some(label_val) = implicit_label_value {
                final_attrs.insert(0, ("label".to_string(), Some(label_val)));
            }

            InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::Executable {
                    language: first_token.to_string(),
                },
                attributes: final_attrs,
            }
        } else {
            // Just attributes, no language - use Pandoc parser
            let attrs = Self::parse_pandoc_attributes(content);
            InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::Plain,
                attributes: attrs,
            }
        }
    }

    /// Parse Pandoc-style attributes for display blocks: {.class #id key="value"}
    /// Spaces are the primary delimiter. Pandoc spec prefers explicit quoting.
    fn parse_pandoc_attributes(content: &str) -> Vec<(String, Option<String>)> {
        let mut attrs = Vec::new();
        let mut chars = content.chars().peekable();

        while chars.peek().is_some() {
            // Skip whitespace
            while matches!(chars.peek(), Some(&' ') | Some(&'\t')) {
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
            while matches!(chars.peek(), Some(&' ') | Some(&'\t')) {
                chars.next();
            }

            // Check for value
            if chars.peek() == Some(&'=') {
                chars.next(); // consume '='

                // Skip whitespace after '='
                while matches!(chars.peek(), Some(&' ') | Some(&'\t')) {
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
                    // Unquoted value - read until space
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

    /// Parse Quarto/RMarkdown chunk options: {language, option=value, option2=value2}
    /// Commas are the primary delimiter (R CSV style). Supports unquoted barewords.
    fn parse_chunk_options(content: &str) -> Vec<(String, Option<String>)> {
        let mut attrs = Vec::new();
        let mut chars = content.chars().peekable();

        while chars.peek().is_some() {
            // Skip whitespace and commas
            while matches!(chars.peek(), Some(&' ') | Some(&'\t') | Some(&',')) {
                chars.next();
            }

            if chars.peek().is_none() {
                break;
            }

            // Read key
            let mut key = String::new();
            while let Some(&ch) = chars.peek() {
                if ch == '=' || ch == ' ' || ch == '\t' || ch == ',' {
                    break;
                }
                key.push(ch);
                chars.next();
            }

            if key.is_empty() {
                break;
            }

            // Skip whitespace and commas
            while matches!(chars.peek(), Some(&' ') | Some(&'\t') | Some(&',')) {
                chars.next();
            }

            // Check for value
            if chars.peek() == Some(&'=') {
                chars.next(); // consume '='

                // Skip whitespace and commas after '='
                while matches!(chars.peek(), Some(&' ') | Some(&'\t') | Some(&',')) {
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
                    // Unquoted value - read until comma, space, or tab
                    let mut val = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch == ' ' || ch == '\t' || ch == ',' {
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

    /// Legacy function - kept for backward compatibility in mixed-form parsing
    /// For new code, use parse_pandoc_attributes or parse_chunk_options
    fn parse_attributes(content: &str) -> Vec<(String, Option<String>)> {
        // Default to chunk options parsing (comma-aware)
        Self::parse_chunk_options(content)
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
    // Strip trailing newline (LF or CRLF) and at most one leading space
    let (info_string_trimmed, _) = strip_newline(info_string_raw);
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

/// Helper to parse info string and emit CodeInfo node with parsed components.
/// This breaks down the info string into its logical parts while preserving all bytes.
fn emit_code_info_node(builder: &mut GreenNodeBuilder<'static>, info_string: &str) {
    builder.start_node(SyntaxKind::CodeInfo.into());

    let info = InfoString::parse(info_string);

    match &info.block_type {
        CodeBlockType::DisplayShortcut { language } => {
            // Simple case: python or python {.class}
            builder.token(SyntaxKind::CodeLanguage.into(), language);

            // If there's more after the language, emit it as TEXT
            let after_lang = &info_string[language.len()..];
            if !after_lang.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), after_lang);
            }
        }
        CodeBlockType::Executable { language } => {
            // Quarto: {r} or {r my-label, echo=FALSE}
            builder.token(SyntaxKind::TEXT.into(), "{");
            builder.token(SyntaxKind::CodeLanguage.into(), language);

            // Everything after language
            let start_offset = 1 + language.len(); // Skip "{r"
            if start_offset < info_string.len() {
                let rest = &info_string[start_offset..];
                builder.token(SyntaxKind::TEXT.into(), rest);
            }
        }
        CodeBlockType::DisplayExplicit { classes } => {
            // Pandoc: {.python} or {#id .haskell .numberLines}
            // We need to find the first class in the raw string and emit everything around it

            if let Some(lang) = classes.first() {
                // Find where ".lang" appears in the info string
                let needle = format!(".{}", lang);
                if let Some(lang_start) = info_string.find(&needle) {
                    // Emit everything before the language
                    if lang_start > 0 {
                        builder.token(SyntaxKind::TEXT.into(), &info_string[..lang_start]);
                    }

                    // Emit the dot
                    builder.token(SyntaxKind::TEXT.into(), ".");

                    // Emit the language
                    builder.token(SyntaxKind::CodeLanguage.into(), lang);

                    // Emit everything after
                    let after_lang_start = lang_start + 1 + lang.len();
                    if after_lang_start < info_string.len() {
                        builder.token(SyntaxKind::TEXT.into(), &info_string[after_lang_start..]);
                    }
                } else {
                    // Couldn't find it, just emit as TEXT
                    builder.token(SyntaxKind::TEXT.into(), info_string);
                }
            } else {
                // No classes
                builder.token(SyntaxKind::TEXT.into(), info_string);
            }
        }
        CodeBlockType::Raw { .. } | CodeBlockType::Plain => {
            // No language, just emit as TEXT
            builder.token(SyntaxKind::TEXT.into(), info_string);
        }
    }

    builder.finish_node(); // CodeInfo
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

    // For lossless parsing: emit the base indent before stripping it
    let first_stripped = if base_indent > 0 && first_inner.len() >= base_indent {
        let indent_str = &first_inner[..base_indent];
        if !indent_str.is_empty() {
            builder.token(SyntaxKind::WHITESPACE.into(), indent_str);
        }
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

    // Emit any space between fence and info string (for losslessness)
    let after_fence = &first_trimmed[fence.fence_count..];
    if let Some(_space_stripped) = after_fence.strip_prefix(' ') {
        // There was a space - emit it as WHITESPACE
        builder.token(SyntaxKind::WHITESPACE.into(), " ");
        // Parse and emit the info string as a structured node
        if !fence.info_string.is_empty() {
            emit_code_info_node(builder, &fence.info_string);
        }
    } else if !fence.info_string.is_empty() {
        // No space - parse and emit info_string as a structured node
        emit_code_info_node(builder, &fence.info_string);
    }

    // Extract and emit the actual newline from the opening fence line
    let (_, newline_str) = strip_newline(first_trimmed);
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }
    builder.finish_node(); // CodeFenceOpen

    let mut current_pos = start_pos + 1;
    let mut content_lines: Vec<&str> = Vec::new(); // Store original lines for lossless parsing
    let mut found_closing = false;

    while current_pos < lines.len() {
        let line = lines[current_pos];

        // Strip blockquote markers to get inner content
        let (line_bq_depth, inner) = count_blockquote_markers(line);

        // If blockquote depth decreases, code block ends (we've left the blockquote)
        if line_bq_depth < bq_depth {
            break;
        }

        // Strip base indent (footnote context) from content lines for fence detection
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

        // Store the ORIGINAL inner line (after blockquote strip only) for lossless parsing
        content_lines.push(inner);
        current_pos += 1;
    }

    // Add content
    if !content_lines.is_empty() {
        builder.start_node(SyntaxKind::CodeContent.into());
        for content_line in content_lines.iter() {
            // Emit base indent for lossless parsing (if present in original line)
            if base_indent > 0 && content_line.len() >= base_indent {
                let indent_str = &content_line[..base_indent];
                if !indent_str.is_empty() {
                    builder.token(SyntaxKind::WHITESPACE.into(), indent_str);
                }
            }

            // Get the content after base indent
            let after_indent = if base_indent > 0 && content_line.len() >= base_indent {
                &content_line[base_indent..]
            } else {
                content_line
            };

            // Split off trailing newline if present (from split_inclusive)
            let (line_without_newline, newline_str) = strip_newline(after_indent);

            if !line_without_newline.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), line_without_newline);
            }

            if !newline_str.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }
        }
        builder.finish_node(); // CodeContent
    }

    // Closing fence (if found)
    if found_closing {
        let closing_line = lines[current_pos - 1];
        let (_, closing_inner) = count_blockquote_markers(closing_line);

        // Emit base indent for lossless parsing
        if base_indent > 0 && closing_inner.len() >= base_indent {
            let indent_str = &closing_inner[..base_indent];
            if !indent_str.is_empty() {
                builder.token(SyntaxKind::WHITESPACE.into(), indent_str);
            }
        }

        // Strip base indent to get fence
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

        // Extract the actual newline from the closing line
        let (_, newline_str) = strip_newline(closing_trimmed);

        builder.start_node(SyntaxKind::CodeFenceClose.into());
        builder.token(
            SyntaxKind::CodeFenceMarker.into(),
            &closing_trimmed[..closing_count],
        );
        if !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
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
    fn test_info_string_executable_with_commas() {
        let info = InfoString::parse("{r, echo=FALSE, warning=TRUE}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Executable {
                language: "r".to_string()
            }
        );
        assert_eq!(info.attributes.len(), 2);
        assert_eq!(
            info.attributes[0],
            ("echo".to_string(), Some("FALSE".to_string()))
        );
        assert_eq!(
            info.attributes[1],
            ("warning".to_string(), Some("TRUE".to_string()))
        );
    }

    #[test]
    fn test_info_string_executable_mixed_commas_spaces() {
        // R-style with commas and spaces
        let info = InfoString::parse("{r, echo=FALSE, label=\"my chunk\"}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Executable {
                language: "r".to_string()
            }
        );
        assert_eq!(info.attributes.len(), 2);
        assert_eq!(
            info.attributes[0],
            ("echo".to_string(), Some("FALSE".to_string()))
        );
        assert_eq!(
            info.attributes[1],
            ("label".to_string(), Some("my chunk".to_string()))
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

    #[test]
    fn test_parse_pandoc_attributes_spaces() {
        // Pandoc display blocks use spaces as delimiters
        let attrs = InfoString::parse_pandoc_attributes(".python .numberLines startFrom=\"10\"");
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], (".python".to_string(), None));
        assert_eq!(attrs[1], (".numberLines".to_string(), None));
        assert_eq!(attrs[2], ("startFrom".to_string(), Some("10".to_string())));
    }

    #[test]
    fn test_parse_pandoc_attributes_no_commas() {
        // Commas in Pandoc attributes should be treated as part of the value
        let attrs = InfoString::parse_pandoc_attributes("#id .class key=value");
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("#id".to_string(), None));
        assert_eq!(attrs[1], (".class".to_string(), None));
        assert_eq!(attrs[2], ("key".to_string(), Some("value".to_string())));
    }

    #[test]
    fn test_parse_chunk_options_commas() {
        // Quarto/RMarkdown chunks use commas as delimiters
        let attrs = InfoString::parse_chunk_options("r, echo=FALSE, warning=TRUE");
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("r".to_string(), None));
        assert_eq!(attrs[1], ("echo".to_string(), Some("FALSE".to_string())));
        assert_eq!(attrs[2], ("warning".to_string(), Some("TRUE".to_string())));
    }

    #[test]
    fn test_parse_chunk_options_no_spaces() {
        // Should handle comma-separated without spaces
        let attrs = InfoString::parse_chunk_options("r,echo=FALSE,warning=TRUE");
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("r".to_string(), None));
        assert_eq!(attrs[1], ("echo".to_string(), Some("FALSE".to_string())));
        assert_eq!(attrs[2], ("warning".to_string(), Some("TRUE".to_string())));
    }

    #[test]
    fn test_parse_chunk_options_mixed() {
        // Handle both commas and spaces
        let attrs = InfoString::parse_chunk_options("python echo=False, warning=True");
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("python".to_string(), None));
        assert_eq!(attrs[1], ("echo".to_string(), Some("False".to_string())));
        assert_eq!(attrs[2], ("warning".to_string(), Some("True".to_string())));
    }

    #[test]
    fn test_display_vs_executable_parsing() {
        // Display block should use Pandoc parser (spaces)
        let info1 = InfoString::parse("{.python .numberLines startFrom=\"10\"}");
        assert!(matches!(
            info1.block_type,
            CodeBlockType::DisplayExplicit { .. }
        ));

        // Executable chunk should use chunk options parser (commas)
        let info2 = InfoString::parse("{r, echo=FALSE, warning=TRUE}");
        assert!(matches!(info2.block_type, CodeBlockType::Executable { .. }));
        assert_eq!(info2.attributes.len(), 2);
    }

    #[test]
    fn test_info_string_executable_implicit_label() {
        // {r mylabel} should parse as label=mylabel
        let info = InfoString::parse("{r mylabel}");
        assert!(matches!(
            info.block_type,
            CodeBlockType::Executable { ref language } if language == "r"
        ));
        assert_eq!(info.attributes.len(), 1);
        assert_eq!(
            info.attributes[0],
            ("label".to_string(), Some("mylabel".to_string()))
        );
    }

    #[test]
    fn test_info_string_executable_implicit_label_with_options() {
        // {r mylabel, echo=FALSE} should parse as label=mylabel, echo=FALSE
        let info = InfoString::parse("{r mylabel, echo=FALSE}");
        assert!(matches!(
            info.block_type,
            CodeBlockType::Executable { ref language } if language == "r"
        ));
        assert_eq!(info.attributes.len(), 2);
        assert_eq!(
            info.attributes[0],
            ("label".to_string(), Some("mylabel".to_string()))
        );
        assert_eq!(
            info.attributes[1],
            ("echo".to_string(), Some("FALSE".to_string()))
        );
    }
}
