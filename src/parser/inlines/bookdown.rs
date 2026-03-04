//! Bookdown cross-reference parsing.
//!
//! Supports:
//! - \@ref(label)
//! - (\#label)
//! - (ref:label)

use crate::parser::inlines::citations::has_bookdown_prefix;

fn is_section_label(label: &str) -> bool {
    !label.contains(':')
}

fn label_allows_prefix(label: &str) -> bool {
    is_section_label(label) || has_bookdown_prefix(label)
}

pub(crate) fn try_parse_bookdown_reference(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();
    if !bytes.starts_with(b"\\@ref(") {
        return None;
    }
    let start = "\\@ref(".len();
    let rest = &text[start..];
    let close = rest.find(')')?;
    if close == 0 {
        return None;
    }
    let label = &rest[..close];
    if !label_allows_prefix(label) {
        return None;
    }
    Some((start + close + 1, label))
}

pub(crate) fn try_parse_bookdown_definition(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();
    if !bytes.starts_with(b"(\\#") {
        return None;
    }
    let start = "(\\#".len();
    let rest = &text[start..];
    let close = rest.find(')')?;
    if close == 0 {
        return None;
    }
    let label = &rest[..close];
    if !label_allows_prefix(label) {
        return None;
    }
    Some((start + close + 1, label))
}

pub(crate) fn try_parse_bookdown_text_reference(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();
    if !bytes.starts_with(b"(ref:") {
        return None;
    }
    let start = "(ref:".len();
    let rest = &text[start..];
    let close = rest.find(')')?;
    if close == 0 {
        return None;
    }
    let label = &rest[..close];
    Some((start + close + 1, label))
}
