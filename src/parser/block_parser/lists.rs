use crate::config::Config;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::utils::strip_newline;

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

pub(crate) fn try_parse_list_marker(
    line: &str,
    config: &Config,
) -> Option<(ListMarker, usize, usize)> {
    let trimmed = line.trim_start_matches([' ', '\t']);

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
            let spaces_after = after_marker
                .chars()
                .take_while(|c| c.is_whitespace())
                .count();
            return Some((ListMarker::Bullet(ch), 1, spaces_after));
        }
    }

    // Try ordered markers
    if let Some(after_marker) = trimmed.strip_prefix("#.")
        && (after_marker.starts_with(' ')
            || after_marker.starts_with('\t')
            || after_marker.is_empty())
    {
        let spaces_after = after_marker
            .chars()
            .take_while(|c| c.is_whitespace())
            .count();
        return Some((ListMarker::Ordered(OrderedMarker::Hash), 2, spaces_after));
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
                let spaces_after = after_marker
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .count();
                let marker_len = 2 + label_end + 1; // "(@" + label + ")"
                return Some((
                    ListMarker::Ordered(OrderedMarker::Example { label }),
                    marker_len,
                    spaces_after,
                ));
            }
        }
    }

    // Try parenthesized markers: (2), (a), (ii)
    if let Some(rest) = trimmed.strip_prefix('(') {
        // Try decimal: (2)
        let digit_count = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digit_count > 0 && rest.len() > digit_count && rest.chars().nth(digit_count) == Some(')')
        {
            let number = &rest[..digit_count];
            let after_marker = &rest[digit_count + 1..];
            if after_marker.starts_with(' ')
                || after_marker.starts_with('\t')
                || after_marker.is_empty()
            {
                let spaces_after = after_marker
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .count();
                let marker_len = 2 + digit_count;
                return Some((
                    ListMarker::Ordered(OrderedMarker::Decimal {
                        number: number.to_string(),
                        style: ListDelimiter::Parens,
                    }),
                    marker_len,
                    spaces_after,
                ));
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
                    let spaces_after = after_marker
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .count();
                    return Some((
                        ListMarker::Ordered(OrderedMarker::LowerRoman {
                            numeral,
                            style: ListDelimiter::Parens,
                        }),
                        len + 2,
                        spaces_after,
                    ));
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
                    let spaces_after = after_marker
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .count();
                    return Some((
                        ListMarker::Ordered(OrderedMarker::UpperRoman {
                            numeral,
                            style: ListDelimiter::Parens,
                        }),
                        len + 2,
                        spaces_after,
                    ));
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
                    let spaces_after = after_marker
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .count();
                    return Some((
                        ListMarker::Ordered(OrderedMarker::LowerAlpha {
                            letter: ch,
                            style: ListDelimiter::Parens,
                        }),
                        3,
                        spaces_after,
                    ));
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
                    let spaces_after = after_marker
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .count();
                    return Some((
                        ListMarker::Ordered(OrderedMarker::UpperAlpha {
                            letter: ch,
                            style: ListDelimiter::Parens,
                        }),
                        3,
                        spaces_after,
                    ));
                }
            }
        }
    }

    // Try decimal numbers: 1. or 1)
    let digit_count = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_count > 0 && trimmed.len() > digit_count {
        let number = &trimmed[..digit_count];
        let delim = trimmed.chars().nth(digit_count);

        let (style, marker_len) = match delim {
            Some('.') => (ListDelimiter::Period, digit_count + 1),
            Some(')') => (ListDelimiter::RightParen, digit_count + 1),
            _ => return None,
        };

        let after_marker = &trimmed[marker_len..];
        if after_marker.starts_with(' ')
            || after_marker.starts_with('\t')
            || after_marker.is_empty()
        {
            let spaces_after = after_marker
                .chars()
                .take_while(|c| c.is_whitespace())
                .count();
            return Some((
                ListMarker::Ordered(OrderedMarker::Decimal {
                    number: number.to_string(),
                    style,
                }),
                marker_len,
                spaces_after,
            ));
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
                let spaces_after = after_marker
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .count();
                return Some((
                    ListMarker::Ordered(OrderedMarker::LowerRoman { numeral, style }),
                    marker_len,
                    spaces_after,
                ));
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
                let spaces_after = after_marker
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .count();
                return Some((
                    ListMarker::Ordered(OrderedMarker::UpperRoman { numeral, style }),
                    marker_len,
                    spaces_after,
                ));
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
                let spaces_after = after_marker
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .count();
                return Some((
                    ListMarker::Ordered(OrderedMarker::LowerAlpha { letter: ch, style }),
                    marker_len,
                    spaces_after,
                ));
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
            let spaces_after = after_marker
                .chars()
                .take_while(|c| c.is_whitespace())
                .count();

            if (after_marker.starts_with(' ') || after_marker.starts_with('\t'))
                && spaces_after >= min_spaces
            {
                return Some((
                    ListMarker::Ordered(OrderedMarker::UpperAlpha { letter: ch, style }),
                    marker_len,
                    spaces_after,
                ));
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

/// Emit a list item node to the builder.
/// Returns the content column for the list item.
pub(crate) fn emit_list_item(
    builder: &mut GreenNodeBuilder<'static>,
    content: &str,
    marker_len: usize,
    spaces_after: usize,
    indent_cols: usize,
    indent_bytes: usize,
) -> usize {
    builder.start_node(SyntaxKind::ListItem.into());

    // Emit leading indentation for lossless parsing
    if indent_bytes > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &content[..indent_bytes]);
    }

    let marker_text = &content[indent_bytes..indent_bytes + marker_len];
    builder.token(SyntaxKind::ListMarker.into(), marker_text);

    if spaces_after > 0 {
        let space_start = indent_bytes + marker_len;
        let space_end = space_start + spaces_after;
        if space_end <= content.len() {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &content[space_start..space_end],
            );
        }
    }

    let content_col = indent_cols + marker_len + spaces_after;
    let content_start = indent_bytes + marker_len + spaces_after;

    if content_start < content.len() {
        let remaining = &content[content_start..];

        // Strip trailing newline from remaining text (it will be emitted separately)
        let (text_part, newline_str) = strip_newline(remaining);

        if !text_part.is_empty() {
            // Check if this is a task list item (starts with [ ] or [x] or [X])
            let trimmed = text_part.trim_start();
            if trimmed.starts_with('[')
                && trimmed.len() >= 3
                && matches!(trimmed.chars().nth(1), Some(' ') | Some('x') | Some('X'))
                && trimmed.chars().nth(2) == Some(']')
            {
                // Emit leading whitespace before checkbox if any
                let leading_ws_len = text_part.len() - trimmed.len();
                if leading_ws_len > 0 {
                    builder.token(SyntaxKind::WHITESPACE.into(), &text_part[..leading_ws_len]);
                }

                // Emit the checkbox as a token
                builder.token(SyntaxKind::TaskCheckbox.into(), &trimmed[..3]);

                // Emit the rest as TEXT
                if trimmed.len() > 3 {
                    builder.token(SyntaxKind::TEXT.into(), &trimmed[3..]);
                }
            } else {
                // Not a task list, emit as normal TEXT
                builder.token(SyntaxKind::TEXT.into(), text_part);
            }
        }

        // Emit newline token separately if present
        if !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
    } else {
        // Empty content line - just emit newline if present
        let (_, line_newline_str) = strip_newline(content);
        if !line_newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), line_newline_str);
        }
    }

    content_col
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn detects_bullet_markers() {
        let config = Config::default();
        assert!(try_parse_list_marker("* item", &config).is_some());
    }

    #[test]
    fn detects_fancy_alpha_markers() {
        let mut config = Config::default();
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
    let mut config = Config::default();
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
    let mut config = Config::default();
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
    let disabled_config = Config {
        extensions: crate::config::Extensions {
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
