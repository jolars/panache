use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use crate::parser::utils::container_stack::{Container, ContainerStack, leading_indent};
use crate::parser::utils::helpers::strip_newline;
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
}

#[derive(Debug, Clone, Copy)]
pub(in crate::parser) struct ListItemEmissionInput<'a> {
    pub content: &'a str,
    pub marker_len: usize,
    pub spaces_after_cols: usize,
    pub spaces_after_bytes: usize,
    pub indent_cols: usize,
    pub indent_bytes: usize,
}

/// Parse a Roman numeral (lower or upper case).
/// Returns (numeral_string, length) if valid, None otherwise.
fn try_parse_roman_numeral(text: &str, uppercase: bool) -> Option<(String, usize)> {
    let valid_chars = if uppercase { "IVXLCDM" } else { "ivxlcdm" };

    let count = text
        .chars()
        .take_while(|c| valid_chars.contains(*c))
        .count();

    if count == 0 {
        return None;
    }

    let numeral = &text[..count];
    let numeral_upper = numeral.to_uppercase();

    // Only consider chars that are valid Roman numeral symbols
    // Reject if it contains only non-Roman letters (a-z except i, v, x, l, c, d, m)
    let has_only_roman_chars = numeral_upper.chars().all(|c| "IVXLCDM".contains(c));
    if !has_only_roman_chars {
        return None;
    }

    // For single-character numerals, only accept the most common ones to avoid
    // ambiguity with alphabetic list markers (a-z, A-Z).
    // Single L, C, D, M are valid Roman numerals but unlikely in list contexts.
    if count == 1 {
        let ch = numeral_upper.chars().next().unwrap();
        if !matches!(ch, 'I' | 'V' | 'X') {
            return None;
        }
    }

    // Validate it's a proper Roman numeral (basic validation)
    // Must not have more than 3 consecutive same characters (except M)
    if numeral_upper.contains("IIII")
        || numeral_upper.contains("XXXX")
        || numeral_upper.contains("CCCC")
        || numeral_upper.contains("VV")
        || numeral_upper.contains("LL")
        || numeral_upper.contains("DD")
    {
        return None;
    }

    // Must have valid subtractive notation (I before V/X, X before L/C, C before D/M)
    // V, L, D can never appear before a larger numeral (no subtractive use)
    let chars: Vec<char> = numeral_upper.chars().collect();
    for i in 0..chars.len().saturating_sub(1) {
        let curr = chars[i];
        let next = chars[i + 1];

        // Get Roman numeral values for comparison
        let curr_val = match curr {
            'I' => 1,
            'V' => 5,
            'X' => 10,
            'L' => 50,
            'C' => 100,
            'D' => 500,
            'M' => 1000,
            _ => return None,
        };
        let next_val = match next {
            'I' => 1,
            'V' => 5,
            'X' => 10,
            'L' => 50,
            'C' => 100,
            'D' => 500,
            'M' => 1000,
            _ => return None,
        };

        // Check for invalid subtractive notation
        if curr_val < next_val {
            // Subtractive notation - check if it's valid
            match (curr, next) {
                ('I', 'V') | ('I', 'X') => {} // Valid: IV=4, IX=9
                ('X', 'L') | ('X', 'C') => {} // Valid: XL=40, XC=90
                ('C', 'D') | ('C', 'M') => {} // Valid: CD=400, CM=900
                _ => return None,             // Invalid subtractive notation
            }
        }
    }

    Some((numeral.to_string(), count))
}

pub(crate) fn try_parse_list_marker(line: &str, config: &ParserOptions) -> Option<ListMarkerMatch> {
    // Trailing newlines should not block bare-marker detection; the line `*\n`
    // is a bare bullet marker and the post-marker text is logically empty.
    let line = line.trim_end_matches(['\r', '\n']);
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
            let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
            return Some(ListMarkerMatch {
                marker: ListMarker::Bullet(ch),
                marker_len: 1,
                spaces_after_cols,
                spaces_after_bytes,
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
        let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
        return Some(ListMarkerMatch {
            marker: ListMarker::Ordered(OrderedMarker::Hash),
            marker_len: 2,
            spaces_after_cols,
            spaces_after_bytes,
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
                let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
                let marker_len = 2 + label_end + 1; // "(@" + label + ")"
                return Some(ListMarkerMatch {
                    marker: ListMarker::Ordered(OrderedMarker::Example { label }),
                    marker_len,
                    spaces_after_cols,
                    spaces_after_bytes,
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
                    let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
                    let marker_len = 2 + digit_count;
                    return Some(ListMarkerMatch {
                        marker: ListMarker::Ordered(OrderedMarker::Decimal {
                            number: number.to_string(),
                            style: ListDelimiter::Parens,
                        }),
                        marker_len,
                        spaces_after_cols,
                        spaces_after_bytes,
                    });
                }
            }
        }

        // Try fancy lists if enabled (parenthesized markers)
        if config.extensions.fancy_lists {
            // Try Roman numerals first (to avoid ambiguity with letters i, v, x, etc.)

            // Try lowercase Roman: (ii)
            if let Some((numeral, len)) = try_parse_roman_numeral(rest, false)
                && rest.len() > len
                && rest.chars().nth(len) == Some(')')
            {
                let after_marker = &rest[len + 1..];
                if after_marker.starts_with(' ')
                    || after_marker.starts_with('\t')
                    || after_marker.is_empty()
                {
                    let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
                    return Some(ListMarkerMatch {
                        marker: ListMarker::Ordered(OrderedMarker::LowerRoman {
                            numeral,
                            style: ListDelimiter::Parens,
                        }),
                        marker_len: len + 2,
                        spaces_after_cols,
                        spaces_after_bytes,
                    });
                }
            }

            // Try uppercase Roman: (II)
            if let Some((numeral, len)) = try_parse_roman_numeral(rest, true)
                && rest.len() > len
                && rest.chars().nth(len) == Some(')')
            {
                let after_marker = &rest[len + 1..];
                if after_marker.starts_with(' ')
                    || after_marker.starts_with('\t')
                    || after_marker.is_empty()
                {
                    let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
                    return Some(ListMarkerMatch {
                        marker: ListMarker::Ordered(OrderedMarker::UpperRoman {
                            numeral,
                            style: ListDelimiter::Parens,
                        }),
                        marker_len: len + 2,
                        spaces_after_cols,
                        spaces_after_bytes,
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
                    let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
                    return Some(ListMarkerMatch {
                        marker: ListMarker::Ordered(OrderedMarker::LowerAlpha {
                            letter: ch,
                            style: ListDelimiter::Parens,
                        }),
                        marker_len: 3,
                        spaces_after_cols,
                        spaces_after_bytes,
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
                    let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
                    return Some(ListMarkerMatch {
                        marker: ListMarker::Ordered(OrderedMarker::UpperAlpha {
                            letter: ch,
                            style: ListDelimiter::Parens,
                        }),
                        marker_len: 3,
                        spaces_after_cols,
                        spaces_after_bytes,
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
        if style == ListDelimiter::RightParen && !config.extensions.fancy_lists {
            return None;
        }

        let after_marker = &trimmed[marker_len..];
        if after_marker.starts_with(' ')
            || after_marker.starts_with('\t')
            || after_marker.is_empty()
        {
            let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
            return Some(ListMarkerMatch {
                marker: ListMarker::Ordered(OrderedMarker::Decimal {
                    number: number.to_string(),
                    style,
                }),
                marker_len,
                spaces_after_cols,
                spaces_after_bytes,
            });
        }
    }

    // Try fancy lists if enabled (non-parenthesized)
    if config.extensions.fancy_lists {
        // Try Roman numerals first, as they may overlap with letters

        // Try lowercase Roman: i. or ii)
        if let Some((numeral, len)) = try_parse_roman_numeral(trimmed, false)
            && trimmed.len() > len
            && let Some(delim) = trimmed.chars().nth(len)
            && (delim == '.' || delim == ')')
        {
            let style = if delim == '.' {
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
                let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
                return Some(ListMarkerMatch {
                    marker: ListMarker::Ordered(OrderedMarker::LowerRoman { numeral, style }),
                    marker_len,
                    spaces_after_cols,
                    spaces_after_bytes,
                });
            }
        }

        // Try uppercase Roman: I. or II)
        if let Some((numeral, len)) = try_parse_roman_numeral(trimmed, true)
            && trimmed.len() > len
            && let Some(delim) = trimmed.chars().nth(len)
            && (delim == '.' || delim == ')')
        {
            let style = if delim == '.' {
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
                let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
                return Some(ListMarkerMatch {
                    marker: ListMarker::Ordered(OrderedMarker::UpperRoman { numeral, style }),
                    marker_len,
                    spaces_after_cols,
                    spaces_after_bytes,
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
                let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);
                return Some(ListMarkerMatch {
                    marker: ListMarker::Ordered(OrderedMarker::LowerAlpha { letter: ch, style }),
                    marker_len,
                    spaces_after_cols,
                    spaces_after_bytes,
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
            let (spaces_after_cols, spaces_after_bytes) = leading_indent(after_marker);

            if (after_marker.starts_with(' ') || after_marker.starts_with('\t'))
                && spaces_after_cols >= min_spaces
            {
                return Some(ListMarkerMatch {
                    marker: ListMarker::Ordered(OrderedMarker::UpperAlpha { letter: ch, style }),
                    marker_len,
                    spaces_after_cols,
                    spaces_after_bytes,
                });
            }
        }
    }

    None
}

pub(crate) fn markers_match(a: &ListMarker, b: &ListMarker) -> bool {
    match (a, b) {
        // All bullet list markers (-, *, +) are considered matching (Pandoc behavior)
        (ListMarker::Bullet(_), ListMarker::Bullet(_)) => true,
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
        markers_match(&a_period, &b_period),
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
        markers_match(&i_period, &ii_period),
        "i. and ii. should match"
    );

    // Different styles should not match
    let a_paren = Ordered(LowerAlpha {
        letter: 'a',
        style: RightParen,
    });
    assert!(
        !markers_match(&a_period, &a_paren),
        "a. and a) should not match"
    );
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
            7
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
            6
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
            && markers_match(marker, list_marker)
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
    let mut buffer = ListItemBuffer::new();
    if !text_to_buffer.is_empty() {
        buffer.push_text(text_to_buffer);
    }
    containers.push(Container::ListItem {
        content_col,
        buffer,
    });
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
    });
}

/// Add a list item to the current list.
pub(in crate::parser) fn add_list_item(
    containers: &mut ContainerStack,
    builder: &mut GreenNodeBuilder<'static>,
    item: &ListItemEmissionInput<'_>,
) {
    let (content_col, text_to_buffer) = emit_list_item(builder, item);

    log::trace!(
        "add_list_item: content={:?}, text_to_buffer={:?}",
        item.content,
        text_to_buffer
    );

    let mut buffer = ListItemBuffer::new();
    if !text_to_buffer.is_empty() {
        buffer.push_text(text_to_buffer);
    }
    containers.push(Container::ListItem {
        content_col,
        buffer,
    });
}
