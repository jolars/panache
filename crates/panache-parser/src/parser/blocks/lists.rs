use crate::options::ParserOptions;
use crate::syntax::{ExampleListMarker, SyntaxKind};
use rowan::GreenNodeBuilder;
use smallvec::SmallVec;

use crate::parser::utils::container_stack::{
    Container, ContainerStack, leading_indent, leading_indent_from,
};
use crate::parser::utils::helpers::{strip_newline, trim_end_newlines};
use crate::parser::utils::list_item_buffer::ListItemBuffer;

/// Signal returned by `add_list_item` / `finish_list_item_with_optional_nested`
/// so the caller can decide how to handle leftover first-line content.
///
/// `BqDispatch` fires when the list item opens an inner BLOCK_QUOTE on the same
/// line (`- > <content>`) and the post-`> ` content is non-empty and not itself
/// a list marker. The caller is responsible for dispatching `content` through
/// the block parser (typically `Parser::parse_inner_content`) so block-level
/// constructs like HTML blocks or headings are recognized rather than wrapped
/// in a stray paragraph.
pub(in crate::parser) enum ListItemFinish {
    Done,
    BqDispatch { content: String },
}

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
        start_number: Option<usize>,
        label: Option<String>,
        style: ListDelimiter,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListDelimiter {
    Period,
    RightParen,
    Parens,
}

/// Context hint for marker detection: the kind of open alphabetic list (if
/// any) at the candidate line's indent column. Used to disambiguate
/// single-letter Roman candidates {i,v,x,I,V,X} from their letter
/// interpretation in Pandoc-dialect input. Pandoc parses `a. … h. … i. … j.`
/// as a single LowerAlpha list (the `i.` after the blank line continues as
/// the letter `i`, not as Roman numeral 1). Marker detection needs this
/// signal to make that classification in a single pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum OpenListHint {
    #[default]
    None,
    LowerAlpha,
    UpperAlpha,
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

pub(in crate::parser) struct EmittedListItem {
    content_col: usize,
    text_to_buffer: String,
    has_task_checkbox: bool,
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

    if count == 1 {
        let upper = bytes[0] & !0x20;
        if !matches!(upper, b'I' | b'V' | b'X') {
            return None;
        }
    }

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

/// Pandoc-dialect single-pass disambiguation: when a single-letter Roman
/// candidate `{i,v,x}` / `{I,V,X}` would shadow an open same-case alpha
/// list, reject the Roman classification so detection falls through to the
/// alpha branch. `numeral_bytes` is the buffer the Roman parser just
/// validated; `len` is its byte-length. The check fires only for `len == 1`
/// (multi-character romans like `ii.` are unambiguously Roman) and only in
/// Pandoc dialect.
fn single_char_roman_shadowed_by_alpha(
    numeral_bytes: &[u8],
    len: usize,
    uppercase: bool,
    hint: OpenListHint,
    dialect: crate::Dialect,
) -> bool {
    if dialect != crate::Dialect::Pandoc || len != 1 {
        return false;
    }
    match (uppercase, hint) {
        (false, OpenListHint::LowerAlpha) => {
            matches!(numeral_bytes[0], b'i' | b'v' | b'x')
        }
        (true, OpenListHint::UpperAlpha) => {
            matches!(numeral_bytes[0], b'I' | b'V' | b'X')
        }
        _ => false,
    }
}

/// The `ParserOptions` bits [`try_parse_list_marker_with`] reads,
/// separated out so consumers without config access at call time (the
/// container-prefix line-run terminator) can capture them up front.
/// Never reconstruct a synthetic `ParserOptions` for this: a future
/// config read inside marker detection would silently see defaults.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct ListMarkerDetect {
    pub(crate) fancy_lists: bool,
    pub(crate) example_lists: bool,
    pub(crate) example_list_resets: bool,
    pub(crate) dialect: crate::Dialect,
}

impl ListMarkerDetect {
    pub(crate) fn from_options(config: &ParserOptions) -> Self {
        Self {
            fancy_lists: config.extensions.fancy_lists,
            example_lists: config.extensions.example_lists,
            example_list_resets: config.pandoc_compat.supports_example_list_resets(),
            dialect: config.dialect,
        }
    }
}

pub(crate) fn try_parse_list_marker(
    line: &str,
    config: &ParserOptions,
    open_alpha_hint: OpenListHint,
) -> Option<ListMarkerMatch> {
    try_parse_list_marker_with(
        line,
        ListMarkerDetect::from_options(config),
        open_alpha_hint,
    )
}

pub(crate) fn try_parse_list_marker_with(
    line: &str,
    detect: ListMarkerDetect,
    open_alpha_hint: OpenListHint,
) -> Option<ListMarkerMatch> {
    let line = trim_end_newlines(line);
    let (_indent_cols, indent_bytes) = leading_indent(line);
    let trimmed = &line[indent_bytes..];

    if crate::parser::blocks::horizontal_rules::try_parse_horizontal_rule(trimmed).is_some() {
        return None;
    }

    if let Some(ch) = trimmed.chars().next()
        && matches!(ch, '*' | '+' | '-')
    {
        let after_marker = &trimmed[1..];

        let trimmed_after = after_marker.trim_start();
        let is_task = trimmed_after.starts_with('[')
            && trimmed_after.len() >= 3
            && matches!(
                trimmed_after.chars().nth(1),
                Some(' ') | Some('x') | Some('X')
            )
            && trimmed_after.chars().nth(2) == Some(']');

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

    if detect.fancy_lists
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

    if detect.example_lists
        && (trimmed.starts_with(['(', '@'])
            || (detect.example_list_resets
                && trimmed.as_bytes().first().is_some_and(u8::is_ascii_digit)))
        && let Some(marker) = trimmed.split_ascii_whitespace().next()
        && let Some(example) = ExampleListMarker::parse(marker)
        && (detect.example_list_resets || marker.trim_start_matches('(').starts_with('@'))
    {
        let marker_len = marker.len();
        let style = if marker.starts_with('(') {
            ListDelimiter::Parens
        } else if marker.ends_with('.') {
            ListDelimiter::Period
        } else {
            ListDelimiter::RightParen
        };
        let after_marker = &trimmed[marker_len..];
        if after_marker.starts_with(' ')
            || after_marker.starts_with('\t')
            || after_marker.is_empty()
        {
            let (spaces_after_cols, spaces_after_bytes, virtual_marker_space) =
                marker_spaces_after(after_marker, _indent_cols + marker_len);
            return Some(ListMarkerMatch {
                marker: ListMarker::Ordered(OrderedMarker::Example {
                    start_number: example.start_number,
                    label: (!example.label.is_empty()).then(|| example.label.to_string()),
                    style,
                }),
                marker_len,
                spaces_after_cols,
                spaces_after_bytes,
                virtual_marker_space,
            });
        }
    }

    if let Some(rest) = trimmed.strip_prefix('(') {
        if detect.fancy_lists {
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

        if detect.fancy_lists {
            if let Some(len) = try_parse_roman_numeral(rest, false)
                && rest.len() > len
                && rest.as_bytes()[len] == b')'
                && !single_char_roman_shadowed_by_alpha(
                    rest.as_bytes(),
                    len,
                    false,
                    open_alpha_hint,
                    detect.dialect,
                )
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

            if let Some(len) = try_parse_roman_numeral(rest, true)
                && rest.len() > len
                && rest.as_bytes()[len] == b')'
                && !single_char_roman_shadowed_by_alpha(
                    rest.as_bytes(),
                    len,
                    true,
                    open_alpha_hint,
                    detect.dialect,
                )
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

    let digit_count = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_count > 0 && trimmed.len() > digit_count {
        if detect.dialect == crate::Dialect::CommonMark && digit_count > 9 {
            return None;
        }

        let number = &trimmed[..digit_count];
        let delim = trimmed.chars().nth(digit_count);

        let (style, marker_len) = match delim {
            Some('.') => (ListDelimiter::Period, digit_count + 1),
            Some(')') => (ListDelimiter::RightParen, digit_count + 1),
            _ => return None,
        };
        if style == ListDelimiter::RightParen
            && !detect.fancy_lists
            && detect.dialect != crate::Dialect::CommonMark
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

    if detect.fancy_lists {
        if let Some(len) = try_parse_roman_numeral(trimmed, false)
            && trimmed.len() > len
            && let delim = trimmed.as_bytes()[len]
            && (delim == b'.' || delim == b')')
            && !single_char_roman_shadowed_by_alpha(
                trimmed.as_bytes(),
                len,
                false,
                open_alpha_hint,
                detect.dialect,
            )
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

        if let Some(len) = try_parse_roman_numeral(trimmed, true)
            && trimmed.len() > len
            && let delim = trimmed.as_bytes()[len]
            && (delim == b'.' || delim == b')')
            && !single_char_roman_shadowed_by_alpha(
                trimmed.as_bytes(),
                len,
                true,
                open_alpha_hint,
                detect.dialect,
            )
        {
            let style = if delim == b'.' {
                ListDelimiter::Period
            } else {
                ListDelimiter::RightParen
            };
            let marker_len = len + 1;

            let after_marker = &trimmed[marker_len..];
            let min_spaces = if delim == b'.' && len == 1 { 2 } else { 1 };
            let (effective_cols, _) = leading_indent_from(after_marker, _indent_cols + marker_len);

            if (after_marker.starts_with(' ')
                || after_marker.starts_with('\t')
                || after_marker.is_empty())
                && (after_marker.is_empty() || effective_cols >= min_spaces)
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
            ListMarker::Ordered(OrderedMarker::Example { style: s1, .. }),
            ListMarker::Ordered(OrderedMarker::Example { style: s2, .. }),
        ) => s1 == s2,
        _ => false,
    }
}

/// One tab stop: the indentation (in columns) required for list continuation
/// paragraphs and nested lists under the `four_space_rule` extension
/// (pandoc <= 2.0 list semantics).
const FOUR_SPACE_RULE_COLS: usize = 4;

/// Column at which a list item's content logically begins. This is the
/// threshold used downstream for continuation/nesting classification and for
/// stripping the leading indent off continuation lines.
///
/// By default it lines up with the first non-space character after the marker
/// (CommonMark / pandoc default). Under the `four_space_rule` extension it is a
/// flat one-tab-width per nesting level, independent of marker width — so a
/// `100.` marker still requires four-space continuation, not six.
pub(in crate::parser) fn list_item_content_col(
    indent_cols: usize,
    marker_len: usize,
    spaces_after_cols: usize,
    config: &ParserOptions,
) -> usize {
    if config.extensions.four_space_rule {
        indent_cols + FOUR_SPACE_RULE_COLS
    } else {
        indent_cols + marker_len + spaces_after_cols
    }
}

/// Emit a list item node to the builder (marker and whitespace only).
/// Returns (content_col, text_to_buffer) where text_to_buffer is the content that should be
/// added to the list item buffer for later inline parsing.
pub(in crate::parser) fn emit_list_item(
    builder: &mut GreenNodeBuilder<'static>,
    item: &ListItemEmissionInput<'_>,
    config: &ParserOptions,
) -> EmittedListItem {
    builder.start_node(SyntaxKind::LIST_ITEM.into());

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

    let content_start = item.indent_bytes + item.marker_len + item.spaces_after_bytes;
    // CommonMark gives an empty marker one logical space of padding,
    // regardless of the whitespace actually present on its line.
    let spaces_after_cols = if config.dialect == crate::Dialect::CommonMark
        && item.content[content_start..].trim().is_empty()
    {
        1
    } else {
        item.spaces_after_cols
    };
    let content_col =
        list_item_content_col(item.indent_cols, item.marker_len, spaces_after_cols, config);

    let mut has_task_checkbox = false;
    let text_to_buffer = if content_start < item.content.len() {
        let rest = &item.content[content_start..];
        if is_task_checkbox(rest) {
            has_task_checkbox = true;
            builder.token(SyntaxKind::TASK_CHECKBOX.into(), &rest[..3]);
            rest[3..].to_string()
        } else {
            rest.to_string()
        }
    } else {
        String::new()
    };

    EmittedListItem {
        content_col,
        text_to_buffer,
        has_task_checkbox,
    }
}

/// Whether the item content starts with a task-list checkbox.
///
/// Pandoc converts task markers *after* inline parsing
/// (`taskListItemFromAscii` in `Text.Pandoc.Shared`), matching only the inline
/// sequence `Str "[x]" : Space : rest`. So the marker needs a literal space or
/// tab on the same line, followed by something. `- [x]` on its own is the
/// bracket-shape pattern `[x]`, not a checkbox, and `- [x]\n  foo` yields a
/// `SoftBreak` rather than a `Space`, so it isn't one either. (GFM's spec is
/// laxer about the continuation-line form; we follow pandoc.)
fn is_task_checkbox(rest: &str) -> bool {
    if !(rest.starts_with("[ ]") || rest.starts_with("[x]") || rest.starts_with("[X]")) {
        return false;
    }
    let after = &rest[3..];
    after.starts_with([' ', '\t'])
        && after
            .split('\n')
            .next()
            .is_some_and(|line| !line.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ParserOptions;

    #[test]
    fn detects_bullet_markers() {
        let config = ParserOptions::default();
        assert!(try_parse_list_marker("* item", &config, OpenListHint::None).is_some());
        assert!(try_parse_list_marker("*\titem", &config, OpenListHint::None).is_some());
    }

    #[test]
    fn detects_fancy_alpha_markers() {
        let mut config = ParserOptions::default();
        config.extensions.fancy_lists = true;

        assert!(
            try_parse_list_marker("a. item", &config, OpenListHint::None).is_some(),
            "a. should parse"
        );
        assert!(
            try_parse_list_marker("b. item", &config, OpenListHint::None).is_some(),
            "b. should parse"
        );
        assert!(
            try_parse_list_marker("c. item", &config, OpenListHint::None).is_some(),
            "c. should parse"
        );

        assert!(
            try_parse_list_marker("a) item", &config, OpenListHint::None).is_some(),
            "a) should parse"
        );
        assert!(
            try_parse_list_marker("b) item", &config, OpenListHint::None).is_some(),
            "b) should parse"
        );
    }

    #[test]
    fn single_letter_i_classified_as_alpha_with_lower_alpha_hint() {
        let config = ParserOptions::default(); // Pandoc + fancy_lists
        let m = try_parse_list_marker("i. foo", &config, OpenListHint::LowerAlpha).unwrap();
        assert!(
            matches!(
                m.marker,
                ListMarker::Ordered(OrderedMarker::LowerAlpha { letter: 'i', .. })
            ),
            "i. should classify as LowerAlpha when a LowerAlpha list is open: got {:?}",
            m.marker
        );
    }

    #[test]
    fn single_letter_i_classified_as_roman_with_no_hint() {
        let config = ParserOptions::default();
        let m = try_parse_list_marker("i. foo", &config, OpenListHint::None).unwrap();
        assert!(
            matches!(
                m.marker,
                ListMarker::Ordered(OrderedMarker::LowerRoman { .. })
            ),
            "i. should classify as LowerRoman with no hint: got {:?}",
            m.marker
        );
    }

    #[test]
    fn multichar_roman_ignores_hint() {
        let config = ParserOptions::default();
        let m = try_parse_list_marker("ii. foo", &config, OpenListHint::LowerAlpha).unwrap();
        assert!(
            matches!(
                m.marker,
                ListMarker::Ordered(OrderedMarker::LowerRoman { .. })
            ),
            "ii. must stay LowerRoman regardless of hint: got {:?}",
            m.marker
        );
    }

    #[test]
    fn hint_ignored_in_commonmark_dialect() {
        let config = ParserOptions {
            dialect: crate::Dialect::CommonMark,
            extensions: crate::options::Extensions {
                fancy_lists: false,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(
            try_parse_list_marker("i. foo", &config, OpenListHint::LowerAlpha).is_none(),
            "i. should not parse as a list marker under CommonMark"
        );
    }

    #[test]
    fn uppercase_i_classified_as_alpha_with_upper_alpha_hint() {
        let config = ParserOptions::default();
        let m = try_parse_list_marker("I.  foo", &config, OpenListHint::UpperAlpha).unwrap();
        assert!(
            matches!(
                m.marker,
                ListMarker::Ordered(OrderedMarker::UpperAlpha { letter: 'I', .. })
            ),
            "I. should classify as UpperAlpha when an UpperAlpha list is open: got {:?}",
            m.marker
        );
    }

    #[test]
    fn lowercase_hint_does_not_shadow_uppercase_candidate() {
        let config = ParserOptions::default();
        let m = try_parse_list_marker("I.  foo", &config, OpenListHint::LowerAlpha).unwrap();
        assert!(
            matches!(
                m.marker,
                ListMarker::Ordered(OrderedMarker::UpperRoman { .. })
            ),
            "I. + LowerAlpha hint must stay UpperRoman (case mismatch): got {:?}",
            m.marker
        );
    }

    #[test]
    fn parenthesized_single_letter_i_obeys_hint() {
        let config = ParserOptions::default();
        let m = try_parse_list_marker("(i) foo", &config, OpenListHint::LowerAlpha).unwrap();
        assert!(
            matches!(
                m.marker,
                ListMarker::Ordered(OrderedMarker::LowerAlpha { letter: 'i', .. })
            ),
            "(i) should classify as LowerAlpha when a LowerAlpha list is open: got {:?}",
            m.marker
        );
    }

    #[test]
    fn open_list_hint_at_indent_lower_alpha_at_same_indent() {
        use crate::parser::utils::container_stack::{Container, ContainerStack};
        let mut stack = ContainerStack::new();
        stack.stack.push(Container::List {
            marker: ListMarker::Ordered(OrderedMarker::LowerAlpha {
                letter: 'a',
                style: ListDelimiter::Period,
            }),
            base_indent_cols: 0,
            has_blank_between_items: false,
        });
        assert_eq!(
            open_list_hint_at_indent(&stack, 0),
            OpenListHint::LowerAlpha
        );
    }

    #[test]
    fn open_list_hint_at_indent_returns_none_when_indent_differs() {
        use crate::parser::utils::container_stack::{Container, ContainerStack};
        let mut stack = ContainerStack::new();
        stack.stack.push(Container::List {
            marker: ListMarker::Ordered(OrderedMarker::LowerAlpha {
                letter: 'a',
                style: ListDelimiter::Period,
            }),
            base_indent_cols: 0,
            has_blank_between_items: false,
        });
        assert_eq!(open_list_hint_at_indent(&stack, 3), OpenListHint::None);
    }

    #[test]
    fn open_list_hint_at_indent_returns_none_for_decimal_or_roman() {
        use crate::parser::utils::container_stack::{Container, ContainerStack};
        let mut stack = ContainerStack::new();
        stack.stack.push(Container::List {
            marker: ListMarker::Ordered(OrderedMarker::Decimal {
                number: "1".to_string(),
                style: ListDelimiter::Period,
            }),
            base_indent_cols: 0,
            has_blank_between_items: false,
        });
        assert_eq!(open_list_hint_at_indent(&stack, 0), OpenListHint::None);

        let mut stack = ContainerStack::new();
        stack.stack.push(Container::List {
            marker: ListMarker::Ordered(OrderedMarker::LowerRoman {
                numeral: "i".to_string(),
                style: ListDelimiter::Period,
            }),
            base_indent_cols: 0,
            has_blank_between_items: false,
        });
        assert_eq!(open_list_hint_at_indent(&stack, 0), OpenListHint::None);
    }

    #[test]
    fn open_list_hint_at_indent_stops_at_blockquote_barrier() {
        use crate::parser::utils::container_stack::{Container, ContainerStack};
        let mut stack = ContainerStack::new();
        stack.stack.push(Container::List {
            marker: ListMarker::Ordered(OrderedMarker::LowerAlpha {
                letter: 'a',
                style: ListDelimiter::Period,
            }),
            base_indent_cols: 0,
            has_blank_between_items: false,
        });
        stack.stack.push(Container::BlockQuote { start_line: None });
        assert_eq!(open_list_hint_at_indent(&stack, 0), OpenListHint::None);
    }
}

#[test]
fn markers_match_fancy_lists() {
    use ListDelimiter::*;
    use ListMarker::*;
    use OrderedMarker::*;

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
    assert!(markers_match(
        &Bullet('-'),
        &Bullet('+'),
        crate::Dialect::Pandoc
    ));
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

    assert!(
        try_parse_list_marker("iv. item", &config, OpenListHint::None).is_some(),
        "iv. should parse"
    );
    assert!(
        try_parse_list_marker("v. item", &config, OpenListHint::None).is_some(),
        "v. should parse"
    );
    assert!(
        try_parse_list_marker("vi. item", &config, OpenListHint::None).is_some(),
        "vi. should parse"
    );
    assert!(
        try_parse_list_marker("vii. item", &config, OpenListHint::None).is_some(),
        "vii. should parse"
    );
    assert!(
        try_parse_list_marker("viii. item", &config, OpenListHint::None).is_some(),
        "viii. should parse"
    );
    assert!(
        try_parse_list_marker("ix. item", &config, OpenListHint::None).is_some(),
        "ix. should parse"
    );
    assert!(
        try_parse_list_marker("x. item", &config, OpenListHint::None).is_some(),
        "x. should parse"
    );
}

#[test]
fn detects_example_list_markers() {
    let mut config = ParserOptions::default();
    config.extensions.example_lists = true;

    assert!(
        try_parse_list_marker("(@) item", &config, OpenListHint::None).is_some(),
        "(@) should parse"
    );

    assert!(
        try_parse_list_marker("(@foo) item", &config, OpenListHint::None).is_some(),
        "(@foo) should parse"
    );
    assert!(
        try_parse_list_marker("(@my_label) item", &config, OpenListHint::None).is_some(),
        "(@my_label) should parse"
    );
    assert!(
        try_parse_list_marker("(@test-123) item", &config, OpenListHint::None).is_some(),
        "(@test-123) should parse"
    );

    let disabled_config = ParserOptions {
        extensions: crate::options::Extensions {
            example_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(
        try_parse_list_marker("(@) item", &disabled_config, OpenListHint::None).is_none(),
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

    for (input, desc) in [("- *\n", "- *"), ("- +\n", "- +"), ("- -\n", "- -")] {
        let tree = parse(input, Some(config.clone()));

        assert_eq!(
            tree.kind(),
            SyntaxKind::DOCUMENT,
            "{desc}: root should be DOCUMENT"
        );

        let outer_list = tree
            .children()
            .find(|n| n.kind() == SyntaxKind::LIST)
            .unwrap_or_else(|| panic!("{desc}: should have outer LIST node"));

        let outer_item = outer_list
            .children()
            .find(|n| n.kind() == SyntaxKind::LIST_ITEM)
            .unwrap_or_else(|| panic!("{desc}: should have outer LIST_ITEM"));

        let nested_list = outer_item
            .children()
            .find(|n| n.kind() == SyntaxKind::LIST)
            .unwrap_or_else(|| {
                panic!(
                    "{desc}: outer LIST_ITEM should contain nested LIST, got: {:?}",
                    outer_item.children().map(|n| n.kind()).collect::<Vec<_>>()
                )
            });

        let nested_item = nested_list
            .children()
            .find(|n| n.kind() == SyntaxKind::LIST_ITEM)
            .unwrap_or_else(|| panic!("{desc}: nested LIST should have LIST_ITEM"));

        let has_plain = nested_item
            .children()
            .any(|n| n.kind() == SyntaxKind::PLAIN);
        assert!(
            !has_plain,
            "{desc}: nested LIST_ITEM should not have PLAIN node (should be empty)"
        );
    }
}

/// Check if we're in any list.
pub(in crate::parser) fn in_list(containers: &ContainerStack) -> bool {
    containers
        .stack
        .iter()
        .any(|c| matches!(c, Container::List { .. }))
}

/// Whether any enclosing container is a list item or a definition body.
///
/// This is the scope of pandoc's "ordered sublists must start at 1" rule
/// (jgm/pandoc#11735, pandoc 3.10.1). Pandoc reparses list-item and
/// definition-body content through `parseFromString`, and the restriction
/// rides on that nested parse — so it applies transitively through any
/// blockquotes or divs opened *inside* such an item, while a blockquote or
/// div at document level is unaffected. Verified against
/// `pandoc -f markdown -t native` 3.10.2 for each of those shapes.
pub(in crate::parser) fn in_list_item_or_definition_body(containers: &ContainerStack) -> bool {
    containers
        .stack
        .iter()
        .any(|c| matches!(c, Container::ListItem { .. } | Container::Definition { .. }))
}

/// Content column of the list item open directly inside the list at
/// `list_level`, if that item is still open.
///
/// Used to tell a sibling item from a marker nested in the current item's
/// content: `find_matching_list_level` cannot, because it tolerates indent
/// drift and so hands back the enclosing list for both. A marker reaching
/// this column is inside the open item, hence a new sublist.
///
/// `None` when the item has already been closed — which is what a blank line
/// between loose items does, and why the answer must come from the matched
/// list rather than from the innermost item on the stack.
pub(in crate::parser) fn open_item_content_col_in_list(
    containers: &ContainerStack,
    list_level: usize,
) -> Option<usize> {
    match containers.stack.get(list_level + 1) {
        Some(Container::ListItem { content_col, .. }) => Some(*content_col),
        _ => None,
    }
}

/// Content column of the innermost open list item or definition body, if any.
pub(in crate::parser) fn innermost_content_col(containers: &ContainerStack) -> Option<usize> {
    containers
        .stack
        .iter()
        .rev()
        .find_map(|c| match c {
            Container::ListItem { content_col, .. } | Container::Definition { content_col, .. } => {
                Some(*content_col)
            }
            Container::BlockQuote { .. } => Some(usize::MAX),
            _ => None,
        })
        .filter(|col| *col != usize::MAX)
}

/// The start number a marker declares, or `None` for auto-numbered markers
/// (`#.` and example lists without a reset).
///
/// Bullets have no start number and return `None` as well — the sublist
/// restriction only constrains ordered lists.
pub(in crate::parser) fn marker_start_number(marker: &ListMarker) -> Option<usize> {
    let ordered = match marker {
        ListMarker::Ordered(o) => o,
        ListMarker::Bullet(_) => return None,
    };
    match ordered {
        OrderedMarker::Hash => None,
        OrderedMarker::Example { start_number, .. } => *start_number,
        OrderedMarker::Decimal { number, .. } => number.parse().ok(),
        OrderedMarker::LowerAlpha { letter, .. } => Some(*letter as usize - 'a' as usize + 1),
        OrderedMarker::UpperAlpha { letter, .. } => Some(*letter as usize - 'A' as usize + 1),
        OrderedMarker::LowerRoman { numeral, .. } | OrderedMarker::UpperRoman { numeral, .. } => {
            roman_to_number(numeral).map(|number| number as usize)
        }
    }
}

fn roman_to_number(numeral: &str) -> Option<u32> {
    let value = |c: char| match c.to_ascii_lowercase() {
        'i' => Some(1),
        'v' => Some(5),
        'x' => Some(10),
        'l' => Some(50),
        'c' => Some(100),
        'd' => Some(500),
        'm' => Some(1000),
        _ => None,
    };
    let digits: Option<Vec<u32>> = numeral.chars().map(value).collect();
    let digits = digits?;
    if digits.is_empty() {
        return None;
    }
    let total = digits
        .iter()
        .enumerate()
        .map(|(i, d)| match digits.get(i + 1) {
            Some(next) if d < next => -(*d as i64),
            _ => *d as i64,
        })
        .sum::<i64>();
    u32::try_from(total).ok()
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

/// Return the kind of open alphabetic list at exactly `indent_cols`, if any.
///
/// Walks the container stack from deepest to shallowest, stopping at a
/// `Container::BlockQuote` barrier (mirrors `find_matching_list_level`'s
/// barrier behavior so a list outside a blockquote can't influence
/// classification inside one). Returns `OpenListHint::None` for any
/// non-alpha marker or when no list is open at the queried indent.
///
/// Used by `try_parse_list_marker` to disambiguate single-letter Roman
/// candidates {i,v,x,I,V,X} against an open alpha list in Pandoc dialect.
/// The exact-indent gate is what protects nested Roman-inside-alpha
/// sublists like `a.\n   i.` — there the inner `i.` lives at a deeper
/// indent than the outer alpha base, so this returns `None` and Roman
/// classification wins.
pub(in crate::parser) fn open_list_hint_at_indent(
    containers: &ContainerStack,
    indent_cols: usize,
) -> OpenListHint {
    for c in containers.stack.iter().rev() {
        if matches!(c, Container::BlockQuote { .. }) {
            return OpenListHint::None;
        }
        if let Container::List {
            marker,
            base_indent_cols,
            ..
        } = c
            && *base_indent_cols == indent_cols
        {
            return match marker {
                ListMarker::Ordered(OrderedMarker::LowerAlpha { .. }) => OpenListHint::LowerAlpha,
                ListMarker::Ordered(OrderedMarker::UpperAlpha { .. }) => OpenListHint::UpperAlpha,
                _ => OpenListHint::None,
            };
        }
    }
    OpenListHint::None
}

/// A marker line caught by pandoc's `listStart` fence in the frame of
/// one of the open nested lists: the stack level of that list, and
/// whether the marker continues it as a sibling item (matching marker
/// kind) or replaces it with a new list at the same position.
pub(in crate::parser) struct BandFence {
    pub level: usize,
    pub marker_matches: bool,
}

/// Locate the list level where a dedented marker can start an item.
/// CommonMark checks for a list marker relative to the enclosing item
/// after matching its indentation. Pandoc applies its per-level `listStart`
/// rule in the same indentation bands (see `tests/frame_pinning.rs`).
///
/// Pandoc parses each nested list inside the enclosing item's content
/// reparse, so within the innermost run of nested items — content
/// columns `c_1 < ... < c_n`, cumulative in the section frame — a
/// marker's `listStart` tolerance is 3 columns past the start of
/// whichever band `[c_{j-1}, c_j)` (`c_0 = 0`) its indent falls in.
/// Within the tolerance it terminates every list above band `j`:
/// level `j`'s own continuation gobble can't reach it, and `listStart`
/// fires in level `j`'s frame. Past the tolerance it is a lazy
/// continuation, and at or past `c_n` it is nested content; both
/// return `None`.
///
/// The walk stops at the containers that break a list section
/// (mirroring `ContainerPrefix::from_stack`'s ladder): their content
/// reparse restarts the frame, so bands never cross them.
pub(in crate::parser) fn band_fence_level(
    containers: &ContainerStack,
    marker: &ListMarker,
    indent_cols: usize,
    dialect: crate::Dialect,
) -> Option<BandFence> {
    let mut levels: SmallVec<[(usize, usize); 4]> = SmallVec::new();
    let mut open_item_col: Option<usize> = None;
    for (i, c) in containers.stack.iter().enumerate().rev() {
        match c {
            Container::BlockQuote { .. }
            | Container::FootnoteDefinition { .. }
            | Container::Definition { .. }
            | Container::Admonition { .. } => break,
            Container::ListItem { content_col, .. } => open_item_col = Some(*content_col),
            Container::List { .. } => {
                if let Some(cc) = open_item_col.take() {
                    levels.push((i, cc));
                }
            }
            _ => {}
        }
    }
    if levels.first().is_none_or(|(_, cc)| indent_cols >= *cc) {
        return None;
    }
    let mut band_start = 0;
    let mut band = None;
    for &(level, cc) in levels.iter().rev() {
        if indent_cols < cc {
            band = Some(level);
            break;
        }
        band_start = cc;
    }
    let level = band?;
    if indent_cols > band_start + 3 {
        return None;
    }
    let marker_matches = match &containers.stack[level] {
        Container::List {
            marker: list_marker,
            ..
        } => markers_match(marker, list_marker, dialect),
        _ => false,
    };
    Some(BandFence {
        level,
        marker_matches,
    })
}

/// Find matching list level for a marker with the given indent.
pub(in crate::parser) fn find_matching_list_level(
    containers: &ContainerStack,
    marker: &ListMarker,
    indent_cols: usize,
    dialect: crate::Dialect,
) -> Option<usize> {
    let mut best_match: Option<(usize, usize, bool)> = None; // (index, distance, base_leq_indent)

    let is_deep_ordered = matches!(marker, ListMarker::Ordered(_)) && indent_cols >= 4;
    let mut best_above_match: Option<(usize, usize)> = None; // (index, delta = base - indent), ordered deep only

    for (i, c) in containers.stack.iter().enumerate().rev() {
        if matches!(c, Container::BlockQuote { .. }) {
            break;
        }
        if let Container::List {
            marker: list_marker,
            base_indent_cols,
            ..
        } = c
            && markers_match(marker, list_marker, dialect)
        {
            let matches = if indent_cols >= 4 && *base_indent_cols >= 4 {
                match (marker, list_marker) {
                    (ListMarker::Ordered(_), ListMarker::Ordered(_)) => {
                        indent_cols.abs_diff(*base_indent_cols) <= 3
                    }
                    _ => indent_cols >= *base_indent_cols && indent_cols <= base_indent_cols + 3,
                }
            } else if indent_cols >= 4 || *base_indent_cols >= 4 {
                match (marker, list_marker) {
                    (ListMarker::Ordered(_), ListMarker::Ordered(_)) => {
                        indent_cols.abs_diff(*base_indent_cols) <= 3
                    }
                    _ => false,
                }
            } else {
                indent_cols.abs_diff(*base_indent_cols) <= 3
            };

            if matches {
                let distance = indent_cols.abs_diff(*base_indent_cols);
                let base_leq_indent = *base_indent_cols <= indent_cols;

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
) -> ListItemFinish {
    if let Some(indent_str) = indent_to_emit {
        builder.token(SyntaxKind::WHITESPACE.into(), indent_str);
    }

    builder.start_node(SyntaxKind::LIST.into());
    containers.push(Container::List {
        marker: marker.clone(),
        base_indent_cols: item.indent_cols,
        has_blank_between_items: false,
    });

    let emitted = emit_list_item(builder, item, config);
    finish_list_item_with_optional_nested(
        containers,
        builder,
        emitted,
        item.virtual_marker_space,
        config,
    )
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
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::LIST_ITEM.into());

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

    builder.start_node(SyntaxKind::LIST.into());

    builder.start_node(SyntaxKind::LIST_ITEM.into());
    builder.token(SyntaxKind::LIST_MARKER.into(), &nested_marker.to_string());

    let content_start = item.indent_bytes + item.marker_len + item.spaces_after_bytes;
    if content_start < item.content.len() {
        let remaining = &item.content[content_start..];
        if remaining.len() > 1 {
            let (_, newline_str) = strip_newline(&remaining[1..]);
            if !newline_str.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }
        }
    }

    builder.finish_node(); // Close nested LIST_ITEM
    builder.finish_node(); // Close nested LIST

    let content_col = list_item_content_col(
        item.indent_cols,
        item.marker_len,
        item.spaces_after_cols,
        config,
    );
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
) -> ListItemFinish {
    let emitted = emit_list_item(builder, item, config);

    log::trace!(
        "add_list_item: content={:?}, text_to_buffer={:?}",
        item.content,
        emitted.text_to_buffer
    );

    finish_list_item_with_optional_nested(
        containers,
        builder,
        emitted,
        item.virtual_marker_space,
        config,
    )
}

fn finish_list_item_with_optional_nested(
    containers: &mut ContainerStack,
    builder: &mut GreenNodeBuilder<'static>,
    emitted: EmittedListItem,
    virtual_marker_space: bool,
    config: &ParserOptions,
) -> ListItemFinish {
    let EmittedListItem {
        content_col,
        text_to_buffer,
        has_task_checkbox,
    } = emitted;
    let buffered_is_thematic_break =
        super::horizontal_rules::try_parse_horizontal_rule(trim_end_newlines(&text_to_buffer))
            .is_some();

    if !has_task_checkbox
        && !buffered_is_thematic_break
        && let Some(inner_match) =
            try_parse_list_marker(&text_to_buffer, config, OpenListHint::None)
        && !(config.dialect != crate::Dialect::CommonMark
            && config
                .effective_pandoc_compat()
                .restricts_ordered_sublist_start()
            && marker_start_number(&inner_match.marker).is_some_and(|start| start != 1))
    {
        let inner_content_start = inner_match.marker_len + inner_match.spaces_after_bytes;
        let after_inner =
            trim_end_newlines(text_to_buffer.get(inner_content_start..).unwrap_or(""));
        if !after_inner.is_empty() {
            containers.push(Container::ListItem {
                content_col,
                buffer: ListItemBuffer::new(),
                marker_only: false, // The nested LIST counts as real content.
                virtual_marker_space,
            });
            builder.start_node(SyntaxKind::LIST.into());
            containers.push(Container::List {
                marker: inner_match.marker.clone(),
                base_indent_cols: content_col,
                has_blank_between_items: false,
            });
            let inner_item = ListItemEmissionInput {
                content: text_to_buffer.as_str(),
                marker_len: inner_match.marker_len,
                spaces_after_cols: inner_match.spaces_after_cols,
                spaces_after_bytes: inner_match.spaces_after_bytes,
                indent_cols: content_col,
                indent_bytes: 0,
                virtual_marker_space: inner_match.virtual_marker_space,
            };
            let inner_emitted = emit_list_item(builder, &inner_item, config);
            let _ = finish_list_item_with_optional_nested(
                containers,
                builder,
                inner_emitted,
                inner_match.virtual_marker_space,
                config,
            );
            return ListItemFinish::Done;
        }
    }

    if !has_task_checkbox && !buffered_is_thematic_break && text_to_buffer.starts_with('>') {
        containers.push(Container::ListItem {
            content_col,
            buffer: ListItemBuffer::new(),
            marker_only: false,
            virtual_marker_space,
        });

        let mut remaining = text_to_buffer.as_str();
        let mut content_offset = 0;
        while let Some(after_marker) = remaining.strip_prefix('>') {
            builder.start_node(SyntaxKind::BLOCK_QUOTE.into());
            builder.token(SyntaxKind::BLOCK_QUOTE_MARKER.into(), ">");
            content_offset += 1;
            remaining = match after_marker.strip_prefix(' ') {
                Some(after_space) => {
                    builder.token(SyntaxKind::WHITESPACE.into(), " ");
                    content_offset += 1;
                    after_space
                }
                None => after_marker,
            };
            containers.push(Container::BlockQuote { start_line: None });
        }

        let trimmed = trim_end_newlines(remaining);

        let inner_is_thematic_break =
            super::horizontal_rules::try_parse_horizontal_rule(trimmed).is_some();
        if !inner_is_thematic_break
            && let Some(inner_match) = try_parse_list_marker(remaining, config, OpenListHint::None)
        {
            let inner_content_start = inner_match.marker_len + inner_match.spaces_after_bytes;
            let after_inner = trim_end_newlines(remaining.get(inner_content_start..).unwrap_or(""));
            if !after_inner.is_empty() {
                let bq_content_col = content_col + content_offset;
                builder.start_node(SyntaxKind::LIST.into());
                containers.push(Container::List {
                    marker: inner_match.marker.clone(),
                    base_indent_cols: bq_content_col,
                    has_blank_between_items: false,
                });
                let inner_item = ListItemEmissionInput {
                    content: remaining,
                    marker_len: inner_match.marker_len,
                    spaces_after_cols: inner_match.spaces_after_cols,
                    spaces_after_bytes: inner_match.spaces_after_bytes,
                    indent_cols: 0,
                    indent_bytes: 0,
                    virtual_marker_space: inner_match.virtual_marker_space,
                };
                let inner_emitted = emit_list_item(builder, &inner_item, config);
                let _ = finish_list_item_with_optional_nested(
                    containers,
                    builder,
                    inner_emitted,
                    inner_match.virtual_marker_space,
                    config,
                );
                return ListItemFinish::Done;
            }
        }

        if !trimmed.is_empty() {
            return ListItemFinish::BqDispatch {
                content: remaining.to_string(),
            };
        }
        if !remaining.is_empty() {
            builder.start_node(SyntaxKind::BLANK_LINE.into());
            builder.token(SyntaxKind::BLANK_LINE.into(), remaining);
            builder.finish_node();
        }
        return ListItemFinish::Done;
    }

    let marker_only = text_to_buffer.trim().is_empty();
    let mut buffer = ListItemBuffer::new();
    if has_task_checkbox {
        buffer.mark_task_paragraph();
    }
    if !text_to_buffer.is_empty() {
        buffer.push_text(text_to_buffer, config);
    }
    containers.push(Container::ListItem {
        content_col,
        buffer,
        marker_only,
        virtual_marker_space,
    });
    ListItemFinish::Done
}
