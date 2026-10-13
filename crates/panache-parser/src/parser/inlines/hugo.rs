//! Lossless Hugo shortcode recognition, shared with block detection.

use super::sink::InlineSink;
use crate::syntax::SyntaxKind;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
pub(crate) struct Tag {
    pub end: usize,
    pub content_end: usize,
    pub name: String,
    pub notation: u8,
    pub closing: bool,
    pub self_closing: bool,
    pub escaped: bool,
    pub complete: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct HugoSpan {
    pub opening: Tag,
    pub closing: Option<(usize, Tag)>,
    pub end: usize,
}

/// Pair each tag once so block lookahead and inline emission share recognition.
#[derive(Debug, Default, Clone)]
pub(crate) struct HugoIndex {
    spans: BTreeMap<usize, HugoSpan>,
    fallback: RefCell<BTreeMap<usize, HugoSpan>>,
}

impl HugoIndex {
    pub(crate) fn new(text: &str) -> Self {
        Self::for_range(text, 0, text.len())
    }

    pub(crate) fn for_range(text: &str, start: usize, end: usize) -> Self {
        let text = &text[..end];
        let mut spans: BTreeMap<usize, HugoSpan> = BTreeMap::new();
        let mut open: HashMap<(String, u8), Vec<usize>> = HashMap::new();
        let mut pos = start;
        while let Some(offset) = text[pos..].find("{{") {
            pos += offset;
            let Some(tag) = scan_tag(&text[pos..]) else {
                pos += 2;
                continue;
            };
            if tag.complete && !tag.escaped {
                let stack = open.entry((tag.name.clone(), tag.notation)).or_default();
                if tag.closing {
                    if let Some(start) = stack.pop() {
                        let span = spans.get_mut(&start).unwrap();
                        span.end = pos + tag.end - start;
                        span.closing = Some((pos - start, tag.clone()));
                    }
                } else if !tag.self_closing {
                    stack.push(pos);
                }
            }
            let end = tag.end;
            spans.insert(
                pos,
                HugoSpan {
                    opening: tag,
                    closing: None,
                    end,
                },
            );
            pos += end;
        }
        Self {
            spans,
            fallback: RefCell::default(),
        }
    }

    pub(crate) fn scan_at<'a>(
        &'a self,
        text: &str,
        pos: usize,
    ) -> Option<std::borrow::Cow<'a, HugoSpan>> {
        if let Some(span) = self.spans.get(&pos) {
            Some(std::borrow::Cow::Borrowed(span))
        } else {
            let tail = &text[pos..];
            if !tail.starts_with("{{<") && !tail.starts_with("{{%") {
                return None;
            }
            // Code spans and escaped openers can hide candidates from the
            // lexical walk. Index the suffix when Markdown reaches one so
            // later calls reuse its pairings instead of rescanning the suffix.
            let mut fallback = self.fallback.borrow_mut();
            if !fallback.contains_key(&pos) {
                fallback.extend(Self::for_range(text, pos, text.len()).spans);
            }
            fallback.get(&pos).cloned().map(std::borrow::Cow::Owned)
        }
    }
}

pub(crate) fn scan_tag(text: &str) -> Option<Tag> {
    let bytes = text.as_bytes();
    let notation = *bytes.get(2)?;
    if !text.starts_with("{{") || !matches!(notation, b'<' | b'%') {
        return None;
    }
    let closing_marker = if notation == b'<' { ">}}" } else { "%}}" };
    let escaped = text[3..].starts_with("/*");
    let mut quote = None;
    let mut pos = 3;
    let mut content_end = text.len();
    let mut end = text.len();
    let mut complete = false;
    while pos < bytes.len() {
        if escaped {
            if text[pos..].starts_with("*/") && text[pos + 2..].starts_with(closing_marker) {
                content_end = pos + 2;
                end = content_end + 3;
                complete = true;
                break;
            }
        } else if let Some(delimiter) = quote {
            if bytes[pos] == b'\\' && delimiter == b'"' {
                pos += 1;
            } else if bytes[pos] == delimiter {
                quote = None;
            }
        } else if text[pos..].starts_with(closing_marker) {
            content_end = pos;
            end = pos + 3;
            complete = true;
            break;
        } else if matches!(bytes[pos], b'"' | b'`') {
            quote = Some(bytes[pos]);
        }
        pos += 1;
        // Delimiters are ASCII, but slicing must stay on UTF-8 boundaries.
        while pos < bytes.len() && !text.is_char_boundary(pos) {
            pos += 1;
        }
    }
    if !complete {
        // Keep the paragraph's final line ending outside an incomplete tag so
        // container formatting does not turn it into an extra content line.
        end = text.trim_end_matches(['\r', '\n']).len().max(3);
        content_end = end;
    }
    let content = text[3..content_end].trim();
    let closing = content.starts_with('/');
    let name = content
        .trim_start_matches('/')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_owned();
    Some(Tag {
        end,
        content_end,
        name,
        notation,
        closing,
        self_closing: content.ends_with('/'),
        escaped,
        complete,
    })
}

pub(crate) fn scan(text: &str) -> Option<HugoSpan> {
    let opening = scan_tag(text)?;
    let mut end = opening.end;
    let mut closing = None;
    if opening.complete && !opening.escaped && !opening.closing && !opening.self_closing {
        let mut pos = end;
        let mut depth = 1;
        while let Some(offset) = text[pos..].find("{{") {
            pos += offset;
            let Some(tag) = scan_tag(&text[pos..]) else {
                pos += 2;
                continue;
            };
            if tag.complete
                && !tag.escaped
                && tag.name == opening.name
                && tag.notation == opening.notation
            {
                if tag.closing {
                    depth -= 1;
                    if depth == 0 {
                        end = pos + tag.end;
                        closing = Some((pos, tag));
                        break;
                    }
                } else if !tag.self_closing {
                    depth += 1;
                }
            }
            pos += tag.end;
        }
    }
    Some(HugoSpan {
        opening,
        closing,
        end,
    })
}

pub(crate) fn emit(builder: &mut impl InlineSink, text: &str, span: &HugoSpan) {
    builder.start_node(SyntaxKind::HUGO_SHORTCODE.into());
    emit_tag(builder, text, &span.opening);
    if let Some((start, tag)) = &span.closing {
        builder.start_node(SyntaxKind::HUGO_SHORTCODE_BODY.into());
        if *start > span.opening.end {
            builder.token(
                SyntaxKind::HUGO_SHORTCODE_TEXT.into(),
                &text[span.opening.end..*start],
            );
        }
        builder.finish_node();
        emit_tag(builder, &text[*start..], tag);
    }
    builder.finish_node();
}

fn emit_tag(builder: &mut impl InlineSink, text: &str, tag: &Tag) {
    builder.start_node(SyntaxKind::HUGO_SHORTCODE_TAG.into());
    builder.token(SyntaxKind::HUGO_SHORTCODE_MARKER_OPEN.into(), &text[..3]);
    builder.start_node(SyntaxKind::HUGO_SHORTCODE_CONTENT.into());
    if tag.content_end > 3 {
        builder.token(
            SyntaxKind::HUGO_SHORTCODE_TEXT.into(),
            &text[3..tag.content_end],
        );
    }
    builder.finish_node();
    if tag.complete {
        builder.token(
            SyntaxKind::HUGO_SHORTCODE_MARKER_CLOSE.into(),
            &text[tag.content_end..tag.end],
        );
    }
    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_delimiters_in_strings() {
        for input in [
            "{{< foo value=\">}}\" >}}",
            "{{% foo `raw\n%}} text` %}}",
            "{{< foo value=\"é\\\" >}}\" >}}",
        ] {
            assert_eq!(scan(input).unwrap().end, input.len());
        }
    }

    #[test]
    fn pairs_nested_calls_and_ignores_escaped_calls() {
        let input =
            "{{< box >}}\n{{< box />}}\n{{< box >}}x{{< /box >}}\n{{</* /box */>}}\n{{< /box >}}";
        let span = scan(input).unwrap();
        assert!(span.closing.is_some());
        assert_eq!(span.end, input.len());
    }

    #[test]
    fn keeps_unpaired_calls_standalone() {
        assert_eq!(scan("{{< foo >}}\nNormal text").unwrap().end, 11);
        assert!(scan("{{% foo %}}\n{{< /foo >}}").unwrap().closing.is_none());
    }

    #[test]
    fn fallback_pairing_is_shared_by_later_candidates() {
        let call = "{{< foo >}}";
        let text = format!(
            "`{{{{< unfinished`\n\n{}",
            format!("{call}\n\n").repeat(100)
        );
        let index = HugoIndex::new(&text);
        let positions: Vec<_> = text.match_indices(call).map(|(pos, _)| pos).collect();
        assert_eq!(index.scan_at(&text, positions[0]).unwrap().end, call.len());
        let fallback = index.fallback.borrow();
        for pos in positions {
            assert_eq!(fallback.get(&pos).unwrap().end, call.len());
        }
    }

    #[test]
    fn indexed_pairing_matches_individual_scans() {
        for text in [
            "{{< foo >}}\n{{< foo >}}\n{{< /foo >}}",
            "{{< foo >}}{{% foo %}}{{< /foo >}}{{% /foo %}}",
            "{{< box >}}{{< box />}}{{< box >}}x{{< /box >}}{{</* /box */>}}{{< /box >}}",
            "{{< foo >}}{{< bar >}}{{< /foo >}}{{< /bar >}}",
            "{{< foo >}}{{< unfinished",
            "`{{< unfinished`\n\n{{< foo >}}",
            "`{{< unfinished`\n{{< foo >}}\n{{< foo >}}\n{{< /foo >}}\n{{< /foo >}}",
            "\\{{< unfinished\n{{< foo >}}{{% box %}}body{{% /box %}}{{< /foo >}}",
        ] {
            let index = HugoIndex::new(text);
            for (pos, _) in text.match_indices("{{") {
                let expected = scan(&text[pos..]).unwrap();
                let actual = index.scan_at(text, pos).unwrap();
                assert_eq!(actual.end, expected.end, "{text:?} at {pos}");
                assert_eq!(
                    actual.closing.as_ref().map(|(pos, _)| *pos),
                    expected.closing.as_ref().map(|(pos, _)| *pos)
                );
            }
        }
    }
}
