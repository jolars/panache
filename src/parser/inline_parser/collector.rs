//! Collection stage - parse inline elements into intermediate representation

use super::elements::{EscapeType, InlineElement};
use super::*;
use crate::config::Config;

/// Collect inline elements from text without resolving emphasis.
///
/// This function scans through text and identifies all inline elements (code spans,
/// links, math, etc.) and delimiter runs (*, _). It does NOT resolve which delimiters
/// are actual emphasis - that happens in stage 2.
///
/// Returns a flat list of inline elements in document order.
pub fn collect_inline_elements(
    text: &str,
    config: &Config,
    allow_reference_links: bool,
) -> Vec<InlineElement> {
    log::trace!(
        "Collecting inline elements: {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len()
    );

    let mut elements = Vec::new();
    let mut pos = 0;
    let bytes = text.as_bytes();

    while pos < text.len() {
        // Try to parse backslash math FIRST (when enabled)
        if bytes[pos] == b'\\'
            && pos + 1 < text.len()
            && config.extensions.tex_math_single_backslash
        {
            // Try display math first: \[...\]
            if bytes[pos + 1] == b'['
                && let Some((len, content)) =
                    math::try_parse_single_backslash_display_math(&text[pos..])
            {
                elements.push(InlineElement::SingleBackslashMath {
                    content: content.to_string(),
                    is_display: true,
                    start: pos,
                    end: pos + len,
                });
                pos += len;
                continue;
            }

            // Try inline math: \(...\)
            if bytes[pos + 1] == b'('
                && let Some((len, content)) =
                    math::try_parse_single_backslash_inline_math(&text[pos..])
            {
                elements.push(InlineElement::SingleBackslashMath {
                    content: content.to_string(),
                    is_display: false,
                    start: pos,
                    end: pos + len,
                });
                pos += len;
                continue;
            }
        }

        // Double backslash math: \\(...\\) and \\[...\\]
        if bytes[pos] == b'\\'
            && pos + 2 < text.len()
            && bytes[pos + 1] == b'\\'
            && config.extensions.tex_math_double_backslash
        {
            // Try display math first: \\[...\\]
            if bytes[pos + 2] == b'['
                && let Some((len, content)) =
                    math::try_parse_double_backslash_display_math(&text[pos..])
            {
                elements.push(InlineElement::DoubleBackslashMath {
                    content: content.to_string(),
                    is_display: true,
                    start: pos,
                    end: pos + len,
                });
                pos += len;
                continue;
            }

            // Try inline math: \\(...\\)
            if bytes[pos + 2] == b'('
                && let Some((len, content)) =
                    math::try_parse_double_backslash_inline_math(&text[pos..])
            {
                elements.push(InlineElement::DoubleBackslashMath {
                    content: content.to_string(),
                    is_display: false,
                    start: pos,
                    end: pos + len,
                });
                pos += len;
                continue;
            }
        }

        // Try to parse backslash escape
        if bytes[pos] == b'\\'
            && let Some((len, ch, escape_type)) = escapes::try_parse_escape(&text[pos..])
        {
            elements.push(InlineElement::Escape {
                char: ch,
                escape_type: match escape_type {
                    escapes::EscapeType::Literal => EscapeType::Literal,
                    escapes::EscapeType::NonbreakingSpace => EscapeType::NonbreakingSpace,
                    escapes::EscapeType::HardLineBreak => EscapeType::HardLineBreak,
                },
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse LaTeX command
        if bytes[pos] == b'\\'
            && let Some(len) = latex::try_parse_latex_command(&text[pos..])
        {
            elements.push(InlineElement::LaTeXCommand {
                full_text: text[pos..pos + len].to_string(),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse Quarto shortcodes
        if bytes[pos] == b'{'
            && config.extensions.quarto_shortcodes
            && let Some((len, content, is_escaped)) = shortcodes::try_parse_shortcode(&text[pos..])
        {
            elements.push(InlineElement::Shortcode {
                content: content.to_string(),
                is_escaped,
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse code span or raw inline span
        if bytes[pos] == b'`'
            && let Some((len, content, backtick_count, attributes)) =
                code_spans::try_parse_code_span(&text[pos..])
        {
            // Check if this is a raw inline span
            if let Some(ref attrs) = attributes
                && config.extensions.raw_attribute
                && let Some(format) = raw_inline::is_raw_inline(attrs)
            {
                elements.push(InlineElement::RawInline {
                    content: content.to_string(),
                    format: format.to_string(),
                    backtick_count,
                    start: pos,
                    end: pos + len,
                });
            } else {
                elements.push(InlineElement::CodeSpan {
                    content: content.to_string(),
                    backtick_count,
                    attributes,
                    start: pos,
                    end: pos + len,
                });
            }
            pos += len;
            continue;
        }

        // Try to parse inline footnote (^[...])
        if bytes[pos] == b'^'
            && pos + 1 < text.len()
            && bytes[pos + 1] == b'['
            && let Some((len, content)) = inline_footnotes::try_parse_inline_footnote(&text[pos..])
        {
            elements.push(InlineElement::InlineFootnote {
                content: content.to_string(),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse superscript (^text^)
        if bytes[pos] == b'^'
            && let Some((len, content)) = superscript::try_parse_superscript(&text[pos..])
        {
            elements.push(InlineElement::Superscript {
                content: content.to_string(),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse subscript (~text~)
        if bytes[pos] == b'~'
            && let Some((len, content)) = subscript::try_parse_subscript(&text[pos..])
        {
            elements.push(InlineElement::Subscript {
                content: content.to_string(),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse strikeout (~~text~~)
        if bytes[pos] == b'~'
            && pos + 1 < text.len()
            && bytes[pos + 1] == b'~'
            && let Some((len, content)) = strikeout::try_parse_strikeout(&text[pos..])
        {
            elements.push(InlineElement::Strikeout {
                content: content.to_string(),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse inline math (must check for $$ first for display math)
        if bytes[pos] == b'$' {
            // Try display math first ($$...$$)
            if let Some((len, content)) = math::try_parse_display_math(&text[pos..]) {
                let dollar_count = text[pos..].chars().take_while(|&c| c == '$').count();

                // Check for trailing attributes (Quarto cross-reference support)
                let after_math = &text[pos + len..];
                let (attr_len, attributes) = if config.extensions.quarto_crossrefs {
                    use crate::parser::block_parser::attributes::try_parse_trailing_attributes;
                    if let Some((_attr_block, _)) = try_parse_trailing_attributes(after_math) {
                        // We have attributes - now find the raw text span
                        let trimmed_after = after_math.trim_start();
                        if let Some(open_brace_pos) = trimmed_after.find('{') {
                            let ws_before_brace = after_math.len() - trimmed_after.len();
                            let attr_text_len = trimmed_after[open_brace_pos..]
                                .find('}')
                                .map(|close| close + 1)
                                .unwrap_or(0);
                            let total_len = ws_before_brace + open_brace_pos + attr_text_len;
                            // Store the raw text, not the AttributeBlock structure
                            (total_len, Some(after_math[..total_len].to_string()))
                        } else {
                            (0, None)
                        }
                    } else {
                        (0, None)
                    }
                } else {
                    (0, None)
                };

                elements.push(InlineElement::DisplayMath {
                    content: content.to_string(),
                    dollar_count: Some(dollar_count),
                    attributes,
                    start: pos,
                    end: pos + len + attr_len,
                });
                pos += len + attr_len;
                continue;
            }

            // Try inline math ($...$)
            if let Some((len, content)) = math::try_parse_inline_math(&text[pos..]) {
                elements.push(InlineElement::InlineMath {
                    content: content.to_string(),
                    start: pos,
                    end: pos + len,
                });
                pos += len;
                continue;
            }
        }

        // Try to parse automatic link
        if bytes[pos] == b'<'
            && let Some((len, url)) = links::try_parse_autolink(&text[pos..])
        {
            elements.push(InlineElement::Autolink {
                full_text: text[pos..pos + len].to_string(),
                url: url.to_string(),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse native span
        if bytes[pos] == b'<'
            && let Some((len, content, attributes)) =
                native_spans::try_parse_native_span(&text[pos..])
        {
            elements.push(InlineElement::NativeSpan {
                content: content.to_string(),
                attributes: attributes.to_string(),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse inline image (must come before inline link since it starts with ![)
        if pos + 1 < text.len()
            && bytes[pos] == b'!'
            && bytes[pos + 1] == b'['
            && let Some((len, alt_text, dest, attributes)) =
                links::try_parse_inline_image(&text[pos..])
        {
            elements.push(InlineElement::InlineImage {
                full_text: text[pos..pos + len].to_string(),
                alt_text: alt_text.to_string(),
                dest: dest.to_string(),
                attributes: attributes.map(|s| s.to_string()),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse reference image
        if pos + 1 < text.len()
            && bytes[pos] == b'!'
            && bytes[pos + 1] == b'['
            && config.extensions.reference_links
            && allow_reference_links
        {
            let allow_shortcut = config.extensions.shortcut_reference_links;
            if let Some((len, alt_text, label, is_shortcut)) =
                links::try_parse_reference_image(&text[pos..], allow_shortcut)
            {
                elements.push(InlineElement::ReferenceImage {
                    alt_text: alt_text.to_string(),
                    label: label.to_string(),
                    is_shortcut,
                    start: pos,
                    end: pos + len,
                });
                pos += len;
                continue;
            }
        }

        // Try to parse footnote reference [^id]
        if bytes[pos] == b'['
            && pos + 1 < text.len()
            && bytes[pos + 1] == b'^'
            && config.extensions.footnotes
            && let Some((len, id)) = inline_footnotes::try_parse_footnote_reference(&text[pos..])
        {
            elements.push(InlineElement::FootnoteReference {
                id: id.to_string(),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse inline link
        if bytes[pos] == b'['
            && let Some((len, link_text, dest, attributes)) =
                links::try_parse_inline_link(&text[pos..])
        {
            elements.push(InlineElement::InlineLink {
                full_text: text[pos..pos + len].to_string(),
                link_text: link_text.to_string(),
                dest: dest.to_string(),
                attributes: attributes.map(|s| s.to_string()),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse reference link
        if bytes[pos] == b'[' && config.extensions.reference_links && allow_reference_links {
            let allow_shortcut = config.extensions.shortcut_reference_links;
            if let Some((len, link_text, label, is_shortcut)) =
                links::try_parse_reference_link(&text[pos..], allow_shortcut)
            {
                elements.push(InlineElement::ReferenceLink {
                    link_text: link_text.to_string(),
                    label: label.to_string(),
                    is_shortcut,
                    start: pos,
                    end: pos + len,
                });
                pos += len;
                continue;
            }
        }

        // Try to parse bracketed citation
        if bytes[pos] == b'['
            && let Some((len, content)) = citations::try_parse_bracketed_citation(&text[pos..])
        {
            elements.push(InlineElement::BracketedCitation {
                content: content.to_string(),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse bracketed span
        if bytes[pos] == b'['
            && let Some((len, content, attributes)) =
                bracketed_spans::try_parse_bracketed_span(&text[pos..])
        {
            elements.push(InlineElement::BracketedSpan {
                content: content.to_string(),
                attributes: attributes.to_string(),
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Try to parse bare citation
        if (bytes[pos] == b'@'
            || (bytes[pos] == b'-' && pos + 1 < text.len() && bytes[pos + 1] == b'@'))
            && let Some((len, key, has_suppress)) = citations::try_parse_bare_citation(&text[pos..])
        {
            elements.push(InlineElement::BareCitation {
                key: key.to_string(),
                has_suppress,
                start: pos,
                end: pos + len,
            });
            pos += len;
            continue;
        }

        // Check for delimiter runs (* or _)
        if bytes[pos] == b'*' || bytes[pos] == b'_' {
            let delim_char = bytes[pos] as char;
            let run_start = pos;
            let mut run_count = 0;
            while pos < bytes.len() && bytes[pos] == delim_char as u8 {
                run_count += 1;
                pos += 1;
            }

            // Analyze flanking to determine if can open/close
            let (can_open, can_close) =
                emphasis::analyze_delimiter_run(text, run_start, delim_char, run_count);

            elements.push(InlineElement::DelimiterRun {
                char: delim_char,
                count: run_count,
                can_open,
                can_close,
                start: run_start,
                end: pos,
            });
            continue;
        }

        // No inline element matched - collect as plain text
        // Find the next position where an inline element might start
        let next_pos = find_next_inline_start(&text[pos..]);
        let text_end = if next_pos > 0 {
            pos + next_pos
        } else {
            text.len()
        };

        if text_end > pos {
            elements.push(InlineElement::Text {
                content: text[pos..text_end].to_string(),
                start: pos,
                end: text_end,
            });
            pos = text_end;
        } else {
            break;
        }
    }

    log::trace!("Collected {} inline elements", elements.len());
    elements
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_plain_text() {
        let config = Config::default();
        let elements = collect_inline_elements("hello world", &config, true);
        assert_eq!(elements.len(), 1);
        match &elements[0] {
            InlineElement::Text {
                content,
                start,
                end,
            } => {
                assert_eq!(content, "hello world");
                assert_eq!(*start, 0);
                assert_eq!(*end, 11);
            }
            _ => panic!("Expected Text element"),
        }
    }

    #[test]
    fn test_collect_code_span() {
        let config = Config::default();
        let elements = collect_inline_elements("`code`", &config, true);
        assert_eq!(elements.len(), 1);
        match &elements[0] {
            InlineElement::CodeSpan { content, .. } => {
                assert_eq!(content, "code");
            }
            _ => panic!("Expected CodeSpan element"),
        }
    }

    #[test]
    fn test_collect_delimiter_runs() {
        let config = Config::default();
        let elements = collect_inline_elements("*foo*", &config, true);
        assert_eq!(elements.len(), 3); // *, foo, *

        match &elements[0] {
            InlineElement::DelimiterRun {
                char,
                count,
                can_open,
                ..
            } => {
                assert_eq!(*char, '*');
                assert_eq!(*count, 1);
                assert!(*can_open);
            }
            _ => panic!("Expected DelimiterRun"),
        }

        match &elements[1] {
            InlineElement::Text { content, .. } => {
                assert_eq!(content, "foo");
            }
            _ => panic!("Expected Text"),
        }

        match &elements[2] {
            InlineElement::DelimiterRun {
                char,
                count,
                can_close,
                ..
            } => {
                assert_eq!(*char, '*');
                assert_eq!(*count, 1);
                assert!(*can_close);
            }
            _ => panic!("Expected DelimiterRun"),
        }
    }

    #[test]
    fn test_collect_mixed_elements() {
        let config = Config::default();
        let elements = collect_inline_elements("foo `code` *bar*", &config, true);

        // Should have: "foo ", code span, " ", delimiter, "bar", delimiter
        assert!(elements.len() >= 4);

        // Check for code span
        let has_code = elements
            .iter()
            .any(|e| matches!(e, InlineElement::CodeSpan { .. }));
        assert!(has_code, "Should have code span");

        // Check for delimiter runs
        let delim_count = elements
            .iter()
            .filter(|e| matches!(e, InlineElement::DelimiterRun { .. }))
            .count();
        assert_eq!(delim_count, 2, "Should have 2 delimiter runs");
    }
}
