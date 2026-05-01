use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use crate::parser::utils::container_stack::{
    Container, ContainerStack, leading_indent, leading_indent_from,
};
use crate::parser::utils::helpers::{strip_newline, trim_end_newlines};
use crate::parser::utils::list_item_buffer::ListItemBuffer;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ListMarker {
    Bullet(char),
    Ordered(OrderedMarker),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OrderedMarker {
    Decimal {
        number: String,
        style: ListDelimiter,
    },
    Hash,
    LowerAlpha {
        letter: char,
        style: ListDelimiter,
    },
    UpperAlpha {
        letter: char,
        style: ListDelimiter,
    },
    LowerRoman {
        numeral: String,
        style: ListDelimiter,
    },
    UpperRoman {
        numeral: String,
        style: ListDelimiter,
    },
    Example {
        label: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListDelimiter {
    Period,
    RightParen,
    Parens,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ListMarkerMatch {
    pub(crate) marker: ListMarker,
    pub(crate) marker_len: usize,
    pub(crate) spaces_after_cols: usize,
    pub(crate) spaces_after_bytes: usize,
    /// True when CommonMark's "≥ 5 cols of post-marker whitespace → marker + 1
    /// virtual space; rest belongs to content" rule fired during marker
    /// detection. The marker's required 1 col of trailing space was virtually
    /// absorbed (typically from a tab) rather than consumed as a literal byte;
    /// the surplus whitespace is left in the post-marker text so block-level
    /// detection can recognize it as an indented code block.
    pub(crate) virtual_marker_space: bool,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::parser) struct ListItemEmissionInput<'a> {
    pub content: &'a str,
    pub marker_len: usize,
    pub spaces_after_cols: usize,
    pub spaces_after_bytes: usize,
    pub indent_cols: usize,
    pub indent_bytes: usize,
    pub virtual_marker_space: bool,
}

/// Parse a Roman numeral (lower or upper case).
/// Returns the byte-length of the numeral if valid, None otherwise.
///
/// Byte-level and allocation-free. Callers (`try_parse_list_marker` for
/// fancy-list ordering) hit this on every line, so the prior path —
/// `to_uppercase` String + repeated `Vec<char>::collect` + an always-
/// allocated `String` return — was a profile hotspot. All Roman numeral
/// chars are ASCII; map to canonical-upper byte via `b & !0x20` and
/// validate without heap traffic. Callers slice the original input
/// only on a confirmed full match (when the trailing `.` / `)` is
/// also present), so the `String` cost is moved off the no-match path.
fn try_parse_roman_numeral(text: &str, uppercase: bool) -> Option<usize> {
    let bytes = text.as_bytes();
    // Take while ASCII char is one of `IVXLCDM` (case-folded).
    let mut count = 0usize;
    while count < bytes.len() {
        let b = bytes[count];
        let valid = if uppercase {
            matches!(b, b'I' | b'V' | b'X' | b'L' | b'C' | b'D' | b'M')
        } else {
            matches!(b, b'i' | b'v' | b'x' | b'l' | b'c' | b'd' | b'm')
        };
        if !valid {
            break;
        }
        count += 1;
    }

    if count == 0 {
        return None;
    }

    // For single-character numerals, only accept the most common ones to avoid
    // ambiguity with alphabetic list markers (a-z, A-Z).
    if count == 1 {
        let upper = bytes[0] & !0x20;
        if !matches!(upper, b'I' | b'V' | b'X') {
            return None;
        }
    }

    // Reject sequences of >= 4 consecutive same chars (case-insensitive).
    // Also reject doubled V/L/D (only ever appear once in valid Romans).
    let mut run_byte = 0u8;
    let mut run_len = 0usize;
    for &b in &bytes[..count] {
        let upper = b & !0x20;
        if upper == run_byte {
            run_len += 1;
        } else {
            run_byte = upper;
            run_len = 1;
        }
        if (run_len > 3 && matches!(upper, b'I' | b'X' | b'C'))
            || (run_len > 1 && matches!(upper, b'V' | b'L' | b'D'))
        {
            return None;
        }
    }

    // Validate subtractive notation: V/L/D can never precede a larger
    // numeral; I, X, C only precede the next two larger units.
    fn val(upper: u8) -> u32 {
        match upper {
            b'I' => 1,
            b'V' => 5,
            b'X' => 10,
            b'L' => 50,
            b'C' => 100,
            b'D' => 500,
            b'M' => 1000,
            _ => 0,
        }
    }
    for i in 0..count.saturating_sub(1) {
        let curr = bytes[i] & !0x20;
        let next = bytes[i + 1] & !0x20;
        let cv = val(curr);
        let nv = val(next);
        if cv < nv {
            match (curr, next) {
                (b'I', b'V') | (b'I', b'X') => {}
                (b'X', b'L') | (b'X', b'C') => {}
                (b'C', b'D') | (b'C', b'M') => {}
                _ => return None,
            }
        }
    }
    Some(count)
}

/// Compute (spaces_after_cols, spaces_after_bytes, virtual_marker_space) for a
/// post-marker string starting at column `marker_end_col` of the source line.
///
/// Implements CommonMark §5.2 rule #2: when the effective column-width of the
/// post-marker whitespace (counted with tabs expanding from `marker_end_col`)
/// is ≥ 5 and there is non-empty content after it, the list item's content
/// column is `marker_end_col + 1` (the marker plus exactly one — possibly
/// virtual — space). The surplus whitespace is left in the post-marker text
/// so block-level dispatch can recognize it as an indented code block.
///
/// In the rule case, when the first byte is a tab whose source-column span
/// exceeds 1, no bytes are consumed (the tab stays in content) and
/// `virtual_marker_space` is true. Otherwise the byte count describes the
/// literal whitespace consumed as marker space.
fn marker_spaces_after(after_marker: &str, marker_end_col: usize) -> (usize, usize, bool) {
    let (effective_cols, n_bytes) = leading_indent_from(after_marker, marker_end_col);
    let after_ws = &after_marker[n_bytes..];
    let has_content = !trim_end_newlines(after_ws).is_empty();
    if has_content && effective_cols >= 5 {
        let bytes = match after_marker.as_bytes().first() {
            Some(b' ') => 1,
            Some(b'\t') => {
                let span = 4 - (marker_end_col % 4);
                if span == 1 { 1 } else { 0 }
            }
            _ => 0,
        };
        (1, bytes, bytes == 0)
    } else {
        (effective_cols, n_bytes, false)
    }
}

pub(crate) fn try_parse_list_marker(line: &str, config: &ParserOptions) -> Option<ListMarkerMatch> {
    // Trailing newlines should not block bare-marker detection; the line `*\n`
    // is a bare bullet marker and the post-marker text is logically empty.
    let line = trim_end_newlines(line);
    let (_indent_cols, indent_bytes) = leading_indent(line);
    let trimmed = &line[indent_bytes..];

    // Try bullet markers (including task lists)
    if let Some(ch) = trimmed.chars().next()
        && matches!(ch, '*' | '+' | '-')
    {
        let after_marker = &trimmed[1..];

        // Check for task list: [ ] or [x] or [X]
        let trimmed_after = after_marker.trim_start();
        let is_task = trimmed_after.starts_with('[')
            && trimmed_after.len() >= 3
            && matches!(
                trimmed_after.chars().nth(1),
                Some(' ') | Some('x') | Some('X')
            )
            && trimmed_after.chars().nth(2) == Some(']');

        // Must be followed by whitespace (or be task list)
        if after_marker.starts_with(' ')
            || after_marker.starts_with('\t')
            || after_marker.is_empty()
            || is_task
        {
            let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                marker_spaces_after(after_marker, _indent_cols + 1);
            return Some(ListMarkerMatch {
                marker: ListMarker::Bullet(ch),
                marker_len: 1,
                spaces_after_cols,
                spaces_after_bytes,
                virtual_marker_space,
            });
        }
    }

    // Try ordered markers
    if config.extensions.fancy_lists
        && let Some(after_marker) = trimmed.strip_prefix("#.")
        && (after_marker.starts_with(' ')
            || after_marker.starts_with('\t')
            || after_marker.is_empty())
    {
        let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
            marker_spaces_after(after_marker, _indent_cols + 2);
        return Some(ListMarkerMatch {
            marker: ListMarker::Ordered(OrderedMarker::Hash),
            marker_len: 2,
            spaces_after_cols,
            spaces_after_bytes,
            virtual_marker_space,
        });
    }

    // Try example lists: (@) or (@label)
    if config.extensions.example_lists
        && let Some(rest) = trimmed.strip_prefix("(@")
    {
        // Check if it has a label or is just (@)
        let label_end = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .count();

        // Must be followed by ')'
        if rest.len() > label_end && rest.chars().nth(label_end) == Some(')') {
            let label = if label_end > 0 {
                Some(rest[..label_end].to_string())
            } else {
                None
            };

            let after_marker = &rest[label_end + 1..];
            if after_marker.starts_with(' ')
                || after_marker.starts_with('\t')
                || after_marker.is_empty()
            {
                let marker_len = 2 + label_end + 1; // "(@" + label + ")"
                let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                    marker_spaces_after(after_marker, _indent_cols + marker_len);
                return Some(ListMarkerMatch {
                    marker: ListMarker::Ordered(OrderedMarker::Example { label }),
                    marker_len,
                    spaces_after_cols,
                    spaces_after_bytes,
                    virtual_marker_space,
                });
            }
        }
    }

    // Try parenthesized markers: (2), (a), (ii)
    if let Some(rest) = trimmed.strip_prefix('(') {
        if config.extensions.fancy_lists {
            // Try decimal: (2)
            let digit_count = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            if digit_count > 0
                && rest.len() > digit_count
                && rest.chars().nth(digit_count) == Some(')')
            {
                let number = &rest[..digit_count];
                let after_marker = &rest[digit_count + 1..];
                if after_marker.starts_with(' ')
                    || after_marker.starts_with('\t')
                    || after_marker.is_empty()
                {
                    let marker_len = 2 + digit_count;
                    let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                        marker_spaces_after(after_marker, _indent_cols + marker_len);
                    return Some(ListMarkerMatch {
                        marker: ListMarker::Ordered(OrderedMarker::Decimal {
                            number: number.to_string(),
                            style: ListDelimiter::Parens,
                        }),
                        marker_len,
                        spaces_after_cols,
                        spaces_after_bytes,
                        virtual_marker_space,
                    });
                }
            }
        }

        // Try fancy lists if enabled (parenthesized markers)
        if config.extensions.fancy_lists {
            // Try Roman numerals first (to avoid ambiguity with letters i, v, x, etc.)

            // Try lowercase Roman: (ii)
            if let Some(len) = try_parse_roman_numeral(rest, false)
                && rest.len() > len
                && rest.as_bytes()[len] == b')'
            {
                let after_marker = &rest[len + 1..];
                if after_marker.starts_with(' ')
                    || after_marker.starts_with('\t')
                    || after_marker.is_empty()
                {
                    let marker_len = len + 2;
                    let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                        marker_spaces_after(after_marker, _indent_cols + marker_len);
                    return Some(ListMarkerMatch {
                        marker: ListMarker::Ordered(OrderedMarker::LowerRoman {
                            numeral: rest[..len].to_string(),
                            style: ListDelimiter::Parens,
                        }),
                        marker_len,
                        spaces_after_cols,
                        spaces_after_bytes,
                        virtual_marker_space,
                    });
                }
            }

            // Try uppercase Roman: (II)
            if let Some(len) = try_parse_roman_numeral(rest, true)
                && rest.len() > len
                && rest.as_bytes()[len] == b')'
            {
                let after_marker = &rest[len + 1..];
                if after_marker.starts_with(' ')
                    || after_marker.starts_with('\t')
                    || after_marker.is_empty()
                {
                    let marker_len = len + 2;
                    let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                        marker_spaces_after(after_marker, _indent_cols + marker_len);
                    return Some(ListMarkerMatch {
                        marker: ListMarker::Ordered(OrderedMarker::UpperRoman {
                            numeral: rest[..len].to_string(),
                            style: ListDelimiter::Parens,
                        }),
                        marker_len,
                        spaces_after_cols,
                        spaces_after_bytes,
                        virtual_marker_space,
                    });
                }
            }

            // Try lowercase letter: (a)
            if let Some(ch) = rest.chars().next()
                && ch.is_ascii_lowercase()
                && rest.len() > 1
                && rest.chars().nth(1) == Some(')')
            {
                let after_marker = &rest[2..];
                if after_marker.starts_with(' ')
                    || after_marker.starts_with('\t')
                    || after_marker.is_empty()
                {
                    let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                        marker_spaces_after(after_marker, _indent_cols + 3);
                    return Some(ListMarkerMatch {
                        marker: ListMarker::Ordered(OrderedMarker::LowerAlpha {
                            letter: ch,
                            style: ListDelimiter::Parens,
                        }),
                        marker_len: 3,
                        spaces_after_cols,
                        spaces_after_bytes,
                        virtual_marker_space,
                    });
                }
            }

            // Try uppercase letter: (A)
            if let Some(ch) = rest.chars().next()
                && ch.is_ascii_uppercase()
                && rest.len() > 1
                && rest.chars().nth(1) == Some(')')
            {
                let after_marker = &rest[2..];
                if after_marker.starts_with(' ')
                    || after_marker.starts_with('\t')
                    || after_marker.is_empty()
                {
                    let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                        marker_spaces_after(after_marker, _indent_cols + 3);
                    return Some(ListMarkerMatch {
                        marker: ListMarker::Ordered(OrderedMarker::UpperAlpha {
                            letter: ch,
                            style: ListDelimiter::Parens,
                        }),
                        marker_len: 3,
                        spaces_after_cols,
                        spaces_after_bytes,
                        virtual_marker_space,
                    });
                }
            }
        }
    }

    // Try decimal numbers: 1. or 1)
    let digit_count = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_count > 0 && trimmed.len() > digit_count {
        // CommonMark restricts ordered list markers to 1-9 digits (spec §5.2).
        // Pandoc-markdown accepts arbitrary digit counts.
        if config.dialect == crate::Dialect::CommonMark && digit_count > 9 {
            return None;
        }

        let number = &trimmed[..digit_count];
        let delim = trimmed.chars().nth(digit_count);

        let (style, marker_len) = match delim {
            Some('.') => (ListDelimiter::Period, digit_count + 1),
            Some(')') => (ListDelimiter::RightParen, digit_count + 1),
            _ => return None,
        };
        // CommonMark §5.2: decimal `1)` markers are part of the core grammar.
        // Pandoc-markdown gates `)`-style ordered markers behind `fancy_lists`.
        if style == ListDelimiter::RightParen
            && !config.extensions.fancy_lists
            && config.dialect != crate::Dialect::CommonMark
        {
            return None;
        }

        let after_marker = &trimmed[marker_len..];
        if after_marker.starts_with(' ')
            || after_marker.starts_with('\t')
            || after_marker.is_empty()
        {
            let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                marker_spaces_after(after_marker, _indent_cols + marker_len);
            return Some(ListMarkerMatch {
                marker: ListMarker::Ordered(OrderedMarker::Decimal {
                    number: number.to_string(),
                    style,
                }),
                marker_len,
                spaces_after_cols,
                spaces_after_bytes,
                virtual_marker_space,
            });
        }
    }

    // Try fancy lists if enabled (non-parenthesized)
    if config.extensions.fancy_lists {
        // Try Roman numerals first, as they may overlap with letters

        // Try lowercase Roman: i. or ii)
        if let Some(len) = try_parse_roman_numeral(trimmed, false)
            && trimmed.len() > len
            && let delim = trimmed.as_bytes()[len]
            && (delim == b'.' || delim == b')')
        {
            let style = if delim == b'.' {
                ListDelimiter::Period
            } else {
                ListDelimiter::RightParen
            };
            let marker_len = len + 1;

            let after_marker = &trimmed[marker_len..];
            if after_marker.starts_with(' ')
                || after_marker.starts_with('\t')
                || after_marker.is_empty()
            {
                let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                    marker_spaces_after(after_marker, _indent_cols + marker_len);
                return Some(ListMarkerMatch {
                    marker: ListMarker::Ordered(OrderedMarker::LowerRoman {
                        numeral: trimmed[..len].to_string(),
                        style,
                    }),
                    marker_len,
                    spaces_after_cols,
                    spaces_after_bytes,
                    virtual_marker_space,
                });
            }
        }

        // Try uppercase Roman: I. or II)
        if let Some(len) = try_parse_roman_numeral(trimmed, true)
            && trimmed.len() > len
            && let delim = trimmed.as_bytes()[len]
            && (delim == b'.' || delim == b')')
        {
            let style = if delim == b'.' {
                ListDelimiter::Period
            } else {
                ListDelimiter::RightParen
            };
            let marker_len = len + 1;

            let after_marker = &trimmed[marker_len..];
            if after_marker.starts_with(' ')
                || after_marker.starts_with('\t')
                || after_marker.is_empty()
            {
                let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                    marker_spaces_after(after_marker, _indent_cols + marker_len);
                return Some(ListMarkerMatch {
                    marker: ListMarker::Ordered(OrderedMarker::UpperRoman {
                        numeral: trimmed[..len].to_string(),
                        style,
                    }),
                    marker_len,
                    spaces_after_cols,
                    spaces_after_bytes,
                    virtual_marker_space,
                });
            }
        }

        // Try lowercase letter: a. or a)
        if let Some(ch) = trimmed.chars().next()
            && ch.is_ascii_lowercase()
            && trimmed.len() > 1
            && let Some(delim) = trimmed.chars().nth(1)
            && (delim == '.' || delim == ')')
        {
            let style = if delim == '.' {
                ListDelimiter::Period
            } else {
                ListDelimiter::RightParen
            };
            let marker_len = 2;

            let after_marker = &trimmed[marker_len..];
            if after_marker.starts_with(' ')
                || after_marker.starts_with('\t')
                || after_marker.is_empty()
            {
                let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                    marker_spaces_after(after_marker, _indent_cols + marker_len);
                return Some(ListMarkerMatch {
                    marker: ListMarker::Ordered(OrderedMarker::LowerAlpha { letter: ch, style }),
                    marker_len,
                    spaces_after_cols,
                    spaces_after_bytes,
                    virtual_marker_space,
                });
            }
        }

        // Try uppercase letter: A. or A)
        if let Some(ch) = trimmed.chars().next()
            && ch.is_ascii_uppercase()
            && trimmed.len() > 1
            && let Some(delim) = trimmed.chars().nth(1)
            && (delim == '.' || delim == ')')
        {
            let style = if delim == '.' {
                ListDelimiter::Period
            } else {
                ListDelimiter::RightParen
            };
            let marker_len = 2;

            let after_marker = &trimmed[marker_len..];
            // Special rule: uppercase letter with period needs 2 spaces minimum
            let min_spaces = if delim == '.' { 2 } else { 1 };
            let (effective_cols, _) = leading_indent_from(after_marker, _indent_cols + marker_len);

            if (after_marker.starts_with(' ') || after_marker.starts_with('\t'))
                && effective_cols >= min_spaces
            {
                let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                    marker_spaces_after(after_marker, _indent_cols + marker_len);
                return Some(ListMarkerMatch {
                    marker: ListMarker::Ordered(OrderedMarker::UpperAlpha { letter: ch, style }),
                    marker_len,
                    spaces_after_cols,
                    spaces_after_bytes,
                    virtual_marker_space,
                });
            }
        }
    }

    None
}

pub(crate) fn markers_match(a: &ListMarker, b: &ListMarker, dialect: crate::Dialect) -> bool {
    match (a, b) {
        // CommonMark §5.3: bullet list markers `-`, `+`, `*` are *distinct*
        // bullet types — switching from one to another starts a new list.
        // Pandoc-markdown treats them as interchangeable: any bullet
        // continues an open bullet list. Verified with pandoc against
        // `- foo\n- bar\n+ baz\n` (#301).
        (ListMarker::Bullet(ca), ListMarker::Bullet(cb)) => match dialect {
            crate::Dialect::CommonMark => ca == cb,
            _ => true,
        },
        (ListMarker::Ordered(OrderedMarker::Hash), ListMarker::Ordered(OrderedMarker::Hash)) => {
            true
        }
        (
            ListMarker::Ordered(OrderedMarker::Decimal { style: s1, .. }),
            ListMarker::Ordered(OrderedMarker::Decimal { style: s2, .. }),
        ) => s1 == s2,
        (
            ListMarker::Ordered(OrderedMarker::LowerAlpha { style: s1, .. }),
            ListMarker::Ordered(OrderedMarker::LowerAlpha { style: s2, .. }),
        ) => s1 == s2,
        (
            ListMarker::Ordered(OrderedMarker::UpperAlpha { style: s1, .. }),
            ListMarker::Ordered(OrderedMarker::UpperAlpha { style: s2, .. }),
        ) => s1 == s2,
        (
            ListMarker::Ordered(OrderedMarker::LowerRoman { style: s1, .. }),
            ListMarker::Ordered(OrderedMarker::LowerRoman { style: s2, .. }),
        ) => s1 == s2,
        (
            ListMarker::Ordered(OrderedMarker::UpperRoman { style: s1, .. }),
            ListMarker::Ordered(OrderedMarker::UpperRoman { style: s2, .. }),
        ) => s1 == s2,
        (
            ListMarker::Ordered(OrderedMarker::Example { .. }),
            ListMarker::Ordered(OrderedMarker::Example { .. }),
        ) => true, // All example list items match each other
        _ => false,
    }
}

/// Emit a list item node to the builder (marker and whitespace only).
/// Returns (content_col, text_to_buffer) where text_to_buffer is the content that should be
/// added to the list item buffer for later inline parsing.
pub(in crate::parser) fn emit_list_item(
    builder: &mut GreenNodeBuilder<'static>,
    item: &ListItemEmissionInput<'_>,
) -> (usize, String) {
    builder.start_node(SyntaxKind::LIST_ITEM.into());

    // Emit leading indentation for lossless parsing
    if item.indent_bytes > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &item.content[..item.indent_bytes],
        );
    }

    let marker_text = &item.content[item.indent_bytes..item.indent_bytes + item.marker_len];
    builder.token(SyntaxKind::LIST_MARKER.into(), marker_text);

    if item.spaces_after_bytes > 0 {
        let space_start = item.indent_bytes + item.marker_len;
        let space_end = space_start + item.spaces_after_bytes;
        if space_end <= item.content.len() {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &item.content[space_start..space_end],
            );
        }
    }

    let content_col = item.indent_cols + item.marker_len + item.spaces_after_cols;
    let content_start = item.indent_bytes + item.marker_len + item.spaces_after_bytes;

    // Extract text content to be buffered (instead of emitting it directly).
    // If the item starts with a task checkbox, emit it as a dedicated token so it
    // doesn't get parsed as a link.
    let text_to_buffer = if content_start < item.content.len() {
        let rest = &item.content[content_start..];
        if (rest.starts_with("[ ]") || rest.starts_with("[x]") || rest.starts_with("[X]"))
            && rest
                .as_bytes()
                .get(3)
                .is_some_and(|b| (*b as char).is_whitespace())
        {
            builder.token(SyntaxKind::TASK_CHECKBOX.into(), &rest[..3]);
            rest[3..].to_string()
        } else {
            rest.to_string()
        }
    } else {
        String::new()
    };

    (content_col, text_to_buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ParserOptions;

    #[test]
    fn detects_bullet_markers() {
        let config = ParserOptions::default();
        assert!(try_parse_list_marker("* item", &config).is_some());
        assert!(try_parse_list_marker("*\titem", &config).is_some());
    }

    #[test]
    fn detects_fancy_alpha_markers() {
        let mut config = ParserOptions::default();
        config.extensions.fancy_lists = true;

        // Test lowercase alpha period
        assert!(
            try_parse_list_marker("a. item", &config).is_some(),
            "a. should parse"
        );
        assert!(
            try_parse_list_marker("b. item", &config).is_some(),
            "b. should parse"
        );
        assert!(
            try_parse_list_marker("c. item", &config).is_some(),
            "c. should parse"
        );

        // Test lowercase alpha right paren
        assert!(
            try_parse_list_marker("a) item", &config).is_some(),
            "a) should parse"
        );
        assert!(
            try_parse_list_marker("b) item", &config).is_some(),
            "b) should parse"
        );
    }
}

#[test]
fn markers_match_fancy_lists() {
    use ListDelimiter::*;
    use ListMarker::*;
    use OrderedMarker::*;

    // Same type and style should match
    let a_period = Ordered(LowerAlpha {
        letter: 'a',
        style: Period,
    });
    let b_period = Ordered(LowerAlpha {
        letter: 'b',
        style: Period,
    });
    assert!(
        markers_match(&a_period, &b_period, crate::Dialect::Pandoc),
        "a. and b. should match"
    );

    let i_period = Ordered(LowerRoman {
        numeral: "i".to_string(),
        style: Period,
    });
    let ii_period = Ordered(LowerRoman {
        numeral: "ii".to_string(),
        style: Period,
    });
    assert!(
        markers_match(&i_period, &ii_period, crate::Dialect::Pandoc),
        "i. and ii. should match"
    );

    // Different styles should not match
    let a_paren = Ordered(LowerAlpha {
        letter: 'a',
        style: RightParen,
    });
    assert!(
        !markers_match(&a_period, &a_paren, crate::Dialect::Pandoc),
        "a. and a) should not match"
    );
}

#[test]
fn markers_match_bullet_dialect_split() {
    use ListMarker::*;
    // Pandoc: any bullet matches any bullet (same list).
    assert!(markers_match(
        &Bullet('-'),
        &Bullet('+'),
        crate::Dialect::Pandoc
    ));
    // CommonMark: bullets match only when the marker character is the same.
    assert!(markers_match(
        &Bullet('-'),
        &Bullet('-'),
        crate::Dialect::CommonMark
    ));
    assert!(!markers_match(
        &Bullet('-'),
        &Bullet('+'),
        crate::Dialect::CommonMark
    ));
    assert!(!markers_match(
        &Bullet('*'),
        &Bullet('-'),
        crate::Dialect::CommonMark
    ));
}

#[test]
fn detects_complex_roman_numerals() {
    let mut config = ParserOptions::default();
    config.extensions.fancy_lists = true;

    // Test various Roman numerals
    assert!(
        try_parse_list_marker("iv. item", &config).is_some(),
        "iv. should parse"
    );
    assert!(
        try_parse_list_marker("v. item", &config).is_some(),
        "v. should parse"
    );
    assert!(
        try_parse_list_marker("vi. item", &config).is_some(),
        "vi. should parse"
    );
    assert!(
        try_parse_list_marker("vii. item", &config).is_some(),
        "vii. should parse"
    );
    assert!(
        try_parse_list_marker("viii. item", &config).is_some(),
        "viii. should parse"
    );
    assert!(
        try_parse_list_marker("ix. item", &config).is_some(),
        "ix. should parse"
    );
    assert!(
        try_parse_list_marker("x. item", &config).is_some(),
        "x. should parse"
    );
}

#[test]
fn detects_example_list_markers() {
    let mut config = ParserOptions::default();
    config.extensions.example_lists = true;

    // Test unlabeled example
    assert!(
        try_parse_list_marker("(@) item", &config).is_some(),
        "(@) should parse"
    );

    // Test labeled examples
    assert!(
        try_parse_list_marker("(@foo) item", &config).is_some(),
        "(@foo) should parse"
    );
    assert!(
        try_parse_list_marker("(@my_label) item", &config).is_some(),
        "(@my_label) should parse"
    );
    assert!(
        try_parse_list_marker("(@test-123) item", &config).is_some(),
        "(@test-123) should parse"
    );

    // Test with extension disabled
    let disabled_config = ParserOptions {
        extensions: crate::options::Extensions {
            example_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(
        try_parse_list_marker("(@) item", &disabled_config).is_none(),
        "(@) should not parse when extension disabled"
    );
}

#[test]
fn deep_ordered_prefers_nearest_enclosing_indent_over_nearest_below() {
    use crate::parser::utils::container_stack::{Container, ContainerStack};

    let marker = ListMarker::Ordered(OrderedMarker::LowerRoman {
        numeral: "ii".to_string(),
        style: ListDelimiter::Period,
    });

    let mut containers = ContainerStack::new();
    containers.push(Container::List {
        marker: marker.clone(),
        base_indent_cols: 8,
        has_blank_between_items: false,
    });
    containers.push(Container::ListItem {
        content_col: 11,
        buffer: crate::parser::utils::list_item_buffer::ListItemBuffer::new(),
        marker_only: false,
        virtual_marker_space: false,
    });
    containers.push(Container::List {
        marker,
        base_indent_cols: 6,
        has_blank_between_items: false,
    });

    // With deep ordered drift (indent 7), we should keep the enclosing level
    // (base indent 8), not re-associate to the nearest lower sibling level (6).
    assert_eq!(
        find_matching_list_level(
            &containers,
            &ListMarker::Ordered(OrderedMarker::LowerRoman {
                numeral: "iii".to_string(),
                style: ListDelimiter::Period,
            }),
            7,
            crate::Dialect::Pandoc,
        ),
        Some(0)
    );
}

#[test]
fn deep_ordered_matches_exact_indent_when_available() {
    use crate::parser::utils::container_stack::{Container, ContainerStack};

    let marker = ListMarker::Ordered(OrderedMarker::LowerRoman {
        numeral: "ii".to_string(),
        style: ListDelimiter::Period,
    });

    let mut containers = ContainerStack::new();
    containers.push(Container::List {
        marker: marker.clone(),
        base_indent_cols: 8,
        has_blank_between_items: false,
    });
    containers.push(Container::List {
        marker,
        base_indent_cols: 6,
        has_blank_between_items: false,
    });

    assert_eq!(
        find_matching_list_level(
            &containers,
            &ListMarker::Ordered(OrderedMarker::LowerRoman {
                numeral: "iii".to_string(),
                style: ListDelimiter::Period,
            }),
            6,
            crate::Dialect::Pandoc,
        ),
        Some(1)
    );
}

#[test]
fn parses_nested_bullet_list_from_single_marker() {
    use crate::parse;
    use crate::syntax::SyntaxKind;

    let config = ParserOptions::default();

    // Test all three bullet marker combinations as nested lists
    for (input, desc) in [("- *\n", "- *"), ("- +\n", "- +"), ("- -\n", "- -")] {
        let tree = parse(input, Some(config.clone()));

        // tree IS the DOCUMENT node
        assert_eq!(
            tree.kind(),
            SyntaxKind::DOCUMENT,
            "{desc}: root should be DOCUMENT"
        );

        // Should have a LIST as first child of DOCUMENT
        let outer_list = tree
            .children()
            .find(|n| n.kind() == SyntaxKind::LIST)
            .unwrap_or_else(|| panic!("{desc}: should have outer LIST node"));

        // Outer list should have a LIST_ITEM
        let outer_item = outer_list
            .children()
            .find(|n| n.kind() == SyntaxKind::LIST_ITEM)
            .unwrap_or_else(|| panic!("{desc}: should have outer LIST_ITEM"));

        // Outer list item should contain a nested LIST (not PLAIN with TEXT)
        let nested_list = outer_item
            .children()
            .find(|n| n.kind() == SyntaxKind::LIST)
            .unwrap_or_else(|| {
                panic!(
                    "{desc}: outer LIST_ITEM should contain nested LIST, got: {:?}",
                    outer_item.children().map(|n| n.kind()).collect::<Vec<_>>()
                )
            });

        // Nested list should have a LIST_ITEM
        let nested_item = nested_list
            .children()
            .find(|n| n.kind() == SyntaxKind::LIST_ITEM)
            .unwrap_or_else(|| panic!("{desc}: nested LIST should have LIST_ITEM"));

        // Nested list item should be empty (no PLAIN or TEXT content)
        let has_plain = nested_item
            .children()
            .any(|n| n.kind() == SyntaxKind::PLAIN);
        assert!(
            !has_plain,
            "{desc}: nested LIST_ITEM should not have PLAIN node (should be empty)"
        );
    }
}

// Helper functions for list management in Parser

/// Check if we're in any list.
pub(in crate::parser) fn in_list(containers: &ContainerStack) -> bool {
    containers
        .stack
        .iter()
        .any(|c| matches!(c, Container::List { .. }))
}

/// Check if we're in a list inside a blockquote.
pub(in crate::parser) fn in_blockquote_list(containers: &ContainerStack) -> bool {
    let mut seen_blockquote = false;
    for c in &containers.stack {
        if matches!(c, Container::BlockQuote { .. }) {
            seen_blockquote = true;
        }
        if seen_blockquote && matches!(c, Container::List { .. }) {
            return true;
        }
    }
    false
}

/// Find matching list level for a marker with the given indent.
pub(in crate::parser) fn find_matching_list_level(
    containers: &ContainerStack,
    marker: &ListMarker,
    indent_cols: usize,
    dialect: crate::Dialect,
) -> Option<usize> {
    // Search from deepest (last) to shallowest (first)
    // But for shallow items (0-3 indent), prefer matching at the closest base indent
    let mut best_match: Option<(usize, usize, bool)> = None; // (index, distance, base_leq_indent)

    let is_deep_ordered = matches!(marker, ListMarker::Ordered(_)) && indent_cols >= 4;
    let mut best_above_match: Option<(usize, usize)> = None; // (index, delta = base - indent), ordered deep only

    for (i, c) in containers.stack.iter().enumerate().rev() {
        if let Container::List {
            marker: list_marker,
            base_indent_cols,
            ..
        } = c
            && markers_match(marker, list_marker, dialect)
        {
            let matches = if indent_cols >= 4 && *base_indent_cols >= 4 {
                // Deep indentation:
                // - bullets stay directional to preserve nesting boundaries
                // - ordered markers allow small symmetric drift to keep
                //   marker-width-aligned lists (i./ii./iii.) at one level
                match (marker, list_marker) {
                    (ListMarker::Ordered(_), ListMarker::Ordered(_)) => {
                        indent_cols.abs_diff(*base_indent_cols) <= 3
                    }
                    _ => indent_cols >= *base_indent_cols && indent_cols <= base_indent_cols + 3,
                }
            } else if indent_cols >= 4 || *base_indent_cols >= 4 {
                // One shallow, one deep:
                // - ordered markers still allow symmetric drift so aligned roman
                //   markers (e.g. 3/4/5 spaces for i./ii./iii.) stay at one level
                // - bullets remain directional to preserve nesting boundaries
                match (marker, list_marker) {
                    (ListMarker::Ordered(_), ListMarker::Ordered(_)) => {
                        indent_cols.abs_diff(*base_indent_cols) <= 3
                    }
                    _ => false,
                }
            } else {
                // Both at shallow indentation (0-3)
                // Allow items within 3 spaces
                indent_cols.abs_diff(*base_indent_cols) <= 3
            };

            if matches {
                let distance = indent_cols.abs_diff(*base_indent_cols);
                let base_leq_indent = *base_indent_cols <= indent_cols;

                // For deep ordered lists, avoid "nearest below" re-association caused by
                // formatter alignment shifts (e.g. i./ii./iii. becoming 6/7/8-space indents).
                // Prefer matching the nearest enclosing level whose base indent is >= current.
                if is_deep_ordered
                    && matches!(
                        (marker, list_marker),
                        (ListMarker::Ordered(_), ListMarker::Ordered(_))
                    )
                    && *base_indent_cols >= indent_cols
                {
                    let delta = *base_indent_cols - indent_cols;
                    if best_above_match.is_none_or(|(_, best_delta)| delta < best_delta) {
                        best_above_match = Some((i, delta));
                    }
                }

                if let Some((_, best_dist, best_base_leq)) = best_match {
                    if distance < best_dist
                        || (distance == best_dist && base_leq_indent && !best_base_leq)
                    {
                        best_match = Some((i, distance, base_leq_indent));
                    }
                } else {
                    best_match = Some((i, distance, base_leq_indent));
                }

                // If we found an exact match, return immediately
                if distance == 0 {
                    return Some(i);
                }
            }
        }
    }

    if let Some((index, _)) = best_above_match {
        return Some(index);
    }

    best_match.map(|(i, _, _)| i)
}

/// Start a nested list within an existing list item.
pub(in crate::parser) fn start_nested_list(
    containers: &mut ContainerStack,
    builder: &mut GreenNodeBuilder<'static>,
    marker: &ListMarker,
    item: &ListItemEmissionInput<'_>,
    indent_to_emit: Option<&str>,
    config: &ParserOptions,
) {
    // Emit the indent if needed
    if let Some(indent_str) = indent_to_emit {
        builder.token(SyntaxKind::WHITESPACE.into(), indent_str);
    }

    // Start nested list
    builder.start_node(SyntaxKind::LIST.into());
    containers.push(Container::List {
        marker: marker.clone(),
        base_indent_cols: item.indent_cols,
        has_blank_between_items: false,
    });

    // Add the nested list item
    let (content_col, text_to_buffer) = emit_list_item(builder, item);
    finish_list_item_with_optional_nested(
        containers,
        builder,
        content_col,
        text_to_buffer,
        item.virtual_marker_space,
        config,
    );
}

/// Checks if the content after a list marker is exactly another bullet marker.
/// Returns the nested bullet marker character if detected.
pub(in crate::parser) fn is_content_nested_bullet_marker(
    content: &str,
    marker_len: usize,
    spaces_after_bytes: usize,
) -> Option<char> {
    let (_, indent_bytes) = leading_indent(content);
    let content_start = indent_bytes + marker_len + spaces_after_bytes;

    if content_start >= content.len() {
        return None;
    }

    let remaining = &content[content_start..];
    let (text_part, _) = strip_newline(remaining);
    let trimmed = text_part.trim();

    // Check if it's exactly one of the bullet marker characters
    if trimmed.len() == 1 {
        let ch = trimmed.chars().next().unwrap();
        if matches!(ch, '*' | '+' | '-') {
            return Some(ch);
        }
    }

    None
}

/// Add a list item that contains a nested empty list (for cases like `- *`).
/// This creates: LIST_ITEM (outer) -> LIST (nested) -> LIST_ITEM (empty inner)
pub(in crate::parser) fn add_list_item_with_nested_empty_list(
    containers: &mut ContainerStack,
    builder: &mut GreenNodeBuilder<'static>,
    item: &ListItemEmissionInput<'_>,
    nested_marker: char,
) {
    // First, emit the outer list item (just marker + whitespace)
    builder.start_node(SyntaxKind::LIST_ITEM.into());

    // Emit leading indentation for lossless parsing
    if item.indent_bytes > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &item.content[..item.indent_bytes],
        );
    }

    let marker_text = &item.content[item.indent_bytes..item.indent_bytes + item.marker_len];
    builder.token(SyntaxKind::LIST_MARKER.into(), marker_text);

    if item.spaces_after_bytes > 0 {
        let space_start = item.indent_bytes + item.marker_len;
        let space_end = space_start + item.spaces_after_bytes;
        if space_end <= item.content.len() {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &item.content[space_start..space_end],
            );
        }
    }

    // Now start the nested list inside this item
    builder.start_node(SyntaxKind::LIST.into());

    // Add empty list item to the nested list
    builder.start_node(SyntaxKind::LIST_ITEM.into());
    builder.token(SyntaxKind::LIST_MARKER.into(), &nested_marker.to_string());

    // Extract and emit the newline from original content (lossless)
    let content_start = item.indent_bytes + item.marker_len + item.spaces_after_bytes;
    if content_start < item.content.len() {
        let remaining = &item.content[content_start..];
        // Skip the nested marker character (1 byte) and get the newline
        if remaining.len() > 1 {
            let (_, newline_str) = strip_newline(&remaining[1..]);
            if !newline_str.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }
        }
    }

    builder.finish_node(); // Close nested LIST_ITEM
    builder.finish_node(); // Close nested LIST

    // Push container for the outer list item
    let content_col = item.indent_cols + item.marker_len + item.spaces_after_cols;
    containers.push(Container::ListItem {
        content_col,
        buffer: ListItemBuffer::new(),
        marker_only: false, // The nested LIST counts as real content.
        virtual_marker_space: item.virtual_marker_space,
    });
}

/// Add a list item to the current list.
pub(in crate::parser) fn add_list_item(
    containers: &mut ContainerStack,
    builder: &mut GreenNodeBuilder<'static>,
    item: &ListItemEmissionInput<'_>,
    config: &ParserOptions,
) {
    let (content_col, text_to_buffer) = emit_list_item(builder, item);

    log::trace!(
        "add_list_item: content={:?}, text_to_buffer={:?}",
        item.content,
        text_to_buffer
    );

    finish_list_item_with_optional_nested(
        containers,
        builder,
        content_col,
        text_to_buffer,
        item.virtual_marker_space,
        config,
    );
}

/// Finish a list item by either buffering its content or, when the buffered
/// content begins with another list marker followed by content, recursively
/// opening a nested LIST with another LIST_ITEM. Pushes the appropriate
/// containers onto the stack so the caller doesn't need to.
fn finish_list_item_with_optional_nested(
    containers: &mut ContainerStack,
    builder: &mut GreenNodeBuilder<'static>,
    content_col: usize,
    text_to_buffer: String,
    virtual_marker_space: bool,
    config: &ParserOptions,
) {
    // A line whose content is a thematic break (e.g. `* * *`) takes precedence
    // over being parsed as a sequence of nested list markers. Both dialects
    // agree: `- * * *` is a list item containing a thematic break, not a
    // chain of bullets.
    let buffered_is_thematic_break =
        super::horizontal_rules::try_parse_horizontal_rule(trim_end_newlines(&text_to_buffer))
            .is_some();

    // Recursive same-line nested list emission is gated to CommonMark.
    // Pandoc-markdown also nests in this position (e.g. `- b. foo` is a
    // bullet wrapping an alpha-ordered list), but the formatter does not
    // yet support emitting an outer LIST_ITEM whose only child is a
    // nested LIST, so producing the nested CST under Pandoc breaks
    // formatter idempotency. Tracked as future work.
    let dialect_allows_nested = config.dialect == crate::Dialect::CommonMark;

    if dialect_allows_nested
        && !buffered_is_thematic_break
        && let Some(inner_match) = try_parse_list_marker(&text_to_buffer, config)
    {
        let inner_content_start = inner_match.marker_len + inner_match.spaces_after_bytes;
        let after_inner =
            trim_end_newlines(text_to_buffer.get(inner_content_start..).unwrap_or(""));
        // Recurse only when there is real content after the inner marker.
        // The bare-inner-marker case (e.g. `- *`) is handled by the existing
        // `add_list_item_with_nested_empty_list` path.
        if !after_inner.is_empty() {
            // Push outer ListItem with empty buffer.
            containers.push(Container::ListItem {
                content_col,
                buffer: ListItemBuffer::new(),
                marker_only: false, // The nested LIST counts as real content.
                virtual_marker_space,
            });
            // Open nested LIST inside the outer LIST_ITEM.
            builder.start_node(SyntaxKind::LIST.into());
            containers.push(Container::List {
                marker: inner_match.marker.clone(),
                base_indent_cols: content_col,
                has_blank_between_items: false,
            });
            // Emit nested LIST_ITEM via emit_list_item, then recurse on its
            // content for further-nested same-line markers.
            let inner_item = ListItemEmissionInput {
                content: text_to_buffer.as_str(),
                marker_len: inner_match.marker_len,
                spaces_after_cols: inner_match.spaces_after_cols,
                spaces_after_bytes: inner_match.spaces_after_bytes,
                indent_cols: content_col,
                indent_bytes: 0,
                virtual_marker_space: inner_match.virtual_marker_space,
            };
            let (inner_content_col, inner_text_to_buffer) = emit_list_item(builder, &inner_item);
            finish_list_item_with_optional_nested(
                containers,
                builder,
                inner_content_col,
                inner_text_to_buffer,
                inner_match.virtual_marker_space,
                config,
            );
            return;
        }
    }

    // Same-line blockquote marker inside a list item: `1. > Blockquote`
    // opens a BLOCK_QUOTE inside the LIST_ITEM, with the post-marker text
    // becoming the first line of the blockquote's paragraph. Both
    // CommonMark and Pandoc-markdown agree on this shape (verified via
    // `pandoc -f commonmark` and `pandoc -f markdown`), but emission is
    // gated to CommonMark for now: the formatter does not yet preserve a
    // LIST_ITEM whose first structural child is a BLOCK_QUOTE through a
    // round-trip (the LIST_MARKER gets dropped on re-format, breaking
    // idempotency). Gating mirrors the same-line nested LIST gate above
    // and is tracked alongside that formatter work.
    if dialect_allows_nested
        && !buffered_is_thematic_break
        && text_to_buffer.starts_with('>')
        && !text_to_buffer.starts_with(">>")
    {
        let bytes = text_to_buffer.as_bytes();
        let has_trailing_space = bytes.get(1).copied() == Some(b' ');
        let content_offset = if has_trailing_space { 2 } else { 1 };
        let remaining = &text_to_buffer[content_offset..];

        // Push outer ListItem with empty buffer; the inner BLOCK_QUOTE
        // counts as real content so `marker_only` is false.
        containers.push(Container::ListItem {
            content_col,
            buffer: ListItemBuffer::new(),
            marker_only: false,
            virtual_marker_space,
        });

        // Open BLOCK_QUOTE node inside the LIST_ITEM and emit the marker.
        builder.start_node(SyntaxKind::BLOCK_QUOTE.into());
        builder.token(SyntaxKind::BLOCK_QUOTE_MARKER.into(), ">");
        if has_trailing_space {
            builder.token(SyntaxKind::WHITESPACE.into(), " ");
        }
        containers.push(Container::BlockQuote {});

        // If there is content after `> `, start a paragraph and buffer
        // the first line; subsequent lines flow in via the parser's main
        // loop (lazy continuation handles the no-marker continuation
        // line in cases like #292).
        let trimmed = trim_end_newlines(remaining);
        if !trimmed.is_empty() {
            crate::parser::blocks::paragraphs::start_paragraph_if_needed(containers, builder);
            crate::parser::blocks::paragraphs::append_paragraph_line(
                containers, builder, remaining, config,
            );
        }
        return;
    }

    let marker_only = text_to_buffer.trim().is_empty();
    let mut buffer = ListItemBuffer::new();
    if !text_to_buffer.is_empty() {
        buffer.push_text(text_to_buffer);
    }
    containers.push(Container::ListItem {
        content_col,
        buffer,
        marker_only,
        virtual_marker_space,
    });
}
