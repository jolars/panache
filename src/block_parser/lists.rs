use crate::config::Config;
use crate::syntax::SyntaxKind;
use log;
use rowan::GreenNodeBuilder;

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
        (ListMarker::Bullet(ch1), ListMarker::Bullet(ch2)) => ch1 == ch2,
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

        // Check if this is a task list item (starts with [ ] or [x] or [X])
        let trimmed = remaining.trim_start();
        if trimmed.starts_with('[')
            && trimmed.len() >= 3
            && matches!(trimmed.chars().nth(1), Some(' ') | Some('x') | Some('X'))
            && trimmed.chars().nth(2) == Some(']')
        {
            // Emit leading whitespace before checkbox if any
            let leading_ws_len = remaining.len() - trimmed.len();
            if leading_ws_len > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &remaining[..leading_ws_len]);
            }

            // Emit the checkbox as a token
            builder.token(SyntaxKind::TaskCheckbox.into(), &trimmed[..3]);

            // Emit the rest as TEXT
            if trimmed.len() > 3 {
                builder.token(SyntaxKind::TEXT.into(), &trimmed[3..]);
            }
        } else {
            // Not a task list, emit as normal TEXT
            builder.token(SyntaxKind::TEXT.into(), remaining);
        }
    }
    builder.token(SyntaxKind::NEWLINE.into(), "\n");

    content_col
}

#[allow(dead_code)]
pub(crate) fn try_parse_list(
    lines: &[&str],
    pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
    _has_blank_line_before: bool,
    config: &Config,
) -> Option<usize> {
    #[derive(Debug, Clone)]
    struct MarkerInfo {
        marker: ListMarker,
        indent_cols: usize,
        indent_bytes: usize,
        marker_len: usize,
        spaces_after: usize,
    }

    #[derive(Debug, Clone)]
    struct ListCtx {
        marker: ListMarker,
        base_indent_cols: usize,
    }

    #[derive(Debug, Clone)]
    struct ItemCtx {
        content_col: usize,
        after_blank_line: bool,
    }

    fn tab_advance(col: usize) -> usize {
        let tab_stop = 4;
        col + (tab_stop - (col % tab_stop))
    }

    fn leading_indent(line: &str) -> (usize, usize) {
        let mut cols = 0usize;
        let mut bytes = 0usize;
        for b in line.bytes() {
            match b {
                b' ' => {
                    cols += 1;
                    bytes += 1;
                }
                b'\t' => {
                    cols = tab_advance(cols);
                    bytes += 1;
                }
                _ => break,
            }
        }
        (cols, bytes)
    }

    fn whitespace_cols(ws: &str, start_col: usize) -> usize {
        let mut col = start_col;
        for b in ws.bytes() {
            match b {
                b' ' => col += 1,
                b'\t' => col = tab_advance(col),
                _ => break,
            }
        }
        col - start_col
    }

    fn byte_index_at_column(line: &str, target_col: usize) -> usize {
        let mut col = 0usize;
        let mut idx = 0usize;
        for (i, b) in line.bytes().enumerate() {
            match b {
                b' ' => {
                    if col >= target_col {
                        return idx;
                    }
                    col += 1;
                    idx = i + 1;
                }
                b'\t' => {
                    if col >= target_col {
                        return idx;
                    }
                    col = tab_advance(col);
                    idx = i + 1;
                }
                _ => break,
            }
        }
        idx
    }

    fn parse_marker_info(line: &str, config: &Config) -> Option<MarkerInfo> {
        let (indent_cols, indent_bytes) = leading_indent(line);
        let (marker, marker_len, spaces_after) = try_parse_list_marker(line, config)?;
        Some(MarkerInfo {
            marker,
            indent_cols,
            indent_bytes,
            marker_len,
            spaces_after,
        })
    }

    fn indent_ok(base_indent_cols: usize, indent_cols: usize) -> bool {
        if base_indent_cols <= 3 {
            indent_cols <= 3
        } else {
            indent_cols >= base_indent_cols && indent_cols <= base_indent_cols + 3
        }
    }

    fn close_one_level(
        builder: &mut GreenNodeBuilder<'static>,
        list_stack: &mut Vec<ListCtx>,
        item_stack: &mut Vec<ItemCtx>,
    ) {
        log::trace!(
            "Closing one level: list_stack.len()={}, item_stack.len()={}",
            list_stack.len(),
            item_stack.len()
        );
        builder.finish_node(); // ListItem
        item_stack.pop();
        builder.finish_node(); // List
        list_stack.pop();
    }

    fn close_to_level(
        builder: &mut GreenNodeBuilder<'static>,
        list_stack: &mut Vec<ListCtx>,
        item_stack: &mut Vec<ItemCtx>,
        keep_levels: usize,
    ) {
        while list_stack.len() > keep_levels {
            close_one_level(builder, list_stack, item_stack);
        }
    }

    fn start_new_item(
        builder: &mut GreenNodeBuilder<'static>,
        line: &str,
        info: &MarkerInfo,
        item_stack: &mut Vec<ItemCtx>,
    ) {
        log::trace!(
            "Starting new item: indent={}, marker={:?}, line={:?}",
            info.indent_cols,
            info.marker,
            line.trim()
        );
        builder.start_node(SyntaxKind::ListItem.into());

        let marker_text = &line[info.indent_bytes..info.indent_bytes + info.marker_len];
        builder.token(SyntaxKind::ListMarker.into(), marker_text);

        if info.spaces_after > 0 {
            let space_start = info.indent_bytes + info.marker_len;
            let space_end = space_start + info.spaces_after;
            let space_text = &line[space_start..space_end];
            builder.token(SyntaxKind::WHITESPACE.into(), space_text);
        }

        let ws_start_col = info.indent_cols + info.marker_len;
        let ws_text = &line[info.indent_bytes + info.marker_len..];
        let ws_prefix = ws_text
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect::<String>();
        let ws_cols = whitespace_cols(&ws_prefix, ws_start_col);

        let content_col = info.indent_cols + info.marker_len + ws_cols;
        let content_start = info.indent_bytes + info.marker_len + info.spaces_after;
        if content_start < line.len() {
            builder.token(SyntaxKind::TEXT.into(), &line[content_start..]);
        }
        builder.token(SyntaxKind::NEWLINE.into(), "\n");

        item_stack.push(ItemCtx {
            content_col,
            after_blank_line: false,
        });
    }

    if pos >= lines.len() {
        return None;
    }

    let first_line = lines[pos];
    let first = parse_marker_info(first_line, config)?;

    // List markers indented >= 4 spaces are treated as code blocks at top level.
    if first.indent_cols >= 4 {
        return None;
    }

    builder.start_node(SyntaxKind::List.into());

    log::trace!("Starting list parse at line {}", pos);

    let mut list_stack = vec![ListCtx {
        marker: first.marker.clone(),
        base_indent_cols: first.indent_cols,
    }];
    let mut item_stack: Vec<ItemCtx> = Vec::new();

    start_new_item(builder, first_line, &first, &mut item_stack);

    let mut i = pos + 1;

    while i < lines.len() {
        let line = lines[i];

        // Blank line handling: only consume blank lines that are part of the list.
        if line.trim().is_empty() {
            let mut peek = i + 1;
            while peek < lines.len() && lines[peek].trim().is_empty() {
                peek += 1;
            }

            if peek >= lines.len() {
                break;
            }

            let next_line = lines[peek];
            let (next_indent_cols, _) = leading_indent(next_line);
            let next_marker = parse_marker_info(next_line, config);

            let cur_item = item_stack.last().expect("list item must exist");
            let cur_list = list_stack.last().expect("list must exist");

            let consume_blank = if let Some(ref m) = next_marker {
                let is_sibling = markers_match(&cur_list.marker, &m.marker)
                    && indent_ok(cur_list.base_indent_cols, m.indent_cols)
                    && m.indent_cols < cur_item.content_col;

                let is_nested = m.indent_cols >= cur_item.content_col;

                is_sibling || is_nested
            } else {
                next_indent_cols >= cur_item.content_col
            };

            if !consume_blank {
                break;
            }

            if let Some(item) = item_stack.last_mut() {
                item.after_blank_line = true;
            }
            i += 1;
            continue;
        }

        // Peek ahead to check if this line has a list marker before closing levels
        let line_marker_info = parse_marker_info(line, config);
        let (line_indent_cols, _) = leading_indent(line);

        log::trace!(
            "Processing line: indent={}, has_marker={}, line={:?}, list_stack.len()={}",
            line_indent_cols,
            line_marker_info.is_some(),
            line.trim(),
            list_stack.len()
        );

        // Close nested list levels if this line is outdented AND doesn't have a list marker
        // that could match at the current or a parent level.
        if line_marker_info.is_none() {
            log::trace!("No marker on line, checking if should close levels");
            while list_stack.len() > 1 {
                let cur_item = item_stack.last().expect("list item must exist");
                if line_indent_cols >= cur_item.content_col {
                    break;
                }
                close_one_level(builder, &mut list_stack, &mut item_stack);
            }
        }

        // Try list marker.
        if let Some(info) = line_marker_info {
            log::trace!(
                "Line has marker: indent={}, marker={:?}",
                info.indent_cols,
                info.marker
            );

            // First: can this marker start a new item in an existing list level?
            let mut matched_level = None;
            for level in (0..list_stack.len()).rev() {
                let list_ctx = &list_stack[level];
                let item_ctx = &item_stack[level];
                let markers_eq = markers_match(&list_ctx.marker, &info.marker);
                let indent_is_ok = indent_ok(list_ctx.base_indent_cols, info.indent_cols);
                let indent_lt_content = info.indent_cols < item_ctx.content_col;

                log::trace!(
                    "  Checking level {}: base_indent={}, item.content_col={}, markers_match={}, indent_ok={}, indent<content={}",
                    level,
                    list_ctx.base_indent_cols,
                    item_ctx.content_col,
                    markers_eq,
                    indent_is_ok,
                    indent_lt_content
                );

                if markers_eq && indent_is_ok && indent_lt_content {
                    matched_level = Some(level);
                    log::trace!("  -> Matched at level {}", level);
                    break;
                }
            }

            if let Some(level) = matched_level {
                log::trace!("Closing to level {} (keep {} levels)", level + 1, level + 1);
                close_to_level(builder, &mut list_stack, &mut item_stack, level + 1);

                // Close the current item at this level and start a new one.
                builder.finish_node(); // ListItem
                item_stack.pop();
                start_new_item(builder, line, &info, &mut item_stack);
                i += 1;
                continue;
            }

            let cur_item = item_stack.last().expect("list item must exist");

            // Nested list if indented to the content column.
            if info.indent_cols >= cur_item.content_col {
                log::trace!(
                    "Creating nested list: marker_indent={} >= content_col={}",
                    info.indent_cols,
                    cur_item.content_col
                );
                builder.start_node(SyntaxKind::List.into());
                list_stack.push(ListCtx {
                    marker: info.marker.clone(),
                    base_indent_cols: info.indent_cols,
                });
                start_new_item(builder, line, &info, &mut item_stack);
                i += 1;
                continue;
            }

            log::trace!("Different marker at same/outer level, ending list");

            // Different marker at the same/outer level: end this list (let the outer parser handle).
            break;
        }

        // Regular continuation line.
        {
            let cur_item = item_stack.last().expect("list item must exist");

            if cur_item.after_blank_line && line_indent_cols < cur_item.content_col {
                break;
            }

            let text = if line_indent_cols >= cur_item.content_col {
                let idx = byte_index_at_column(line, cur_item.content_col);
                &line[idx..]
            } else {
                line.trim_start()
            };

            builder.token(SyntaxKind::TEXT.into(), text);
            builder.token(SyntaxKind::NEWLINE.into(), "\n");

            if let Some(item) = item_stack.last_mut() {
                item.after_blank_line = false;
            }

            i += 1;
        }
    }

    // Close all remaining open list levels/items.
    while !list_stack.is_empty() {
        close_one_level(builder, &mut list_stack, &mut item_stack);
    }

    Some(i)
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
