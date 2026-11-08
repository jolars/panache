use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use rowan::{GreenNodeBuilder, NodeOrToken};

use super::FormatContext;
use super::blocks::logical_line_block_lines;
use crate::config::{Config, Dialect, SmartPunctuation};
use crate::syntax::{
    AstNode, AutoLink, Figure, HeadingContent, ImageAlt, ImageLink, LineBlock, LineBlockLine, Link,
    LinkText, Paragraph, Plain, SyntaxKind, SyntaxNode, SyntaxToken, TableCaption, TableCell,
};
use panache_parser::parser::inlines::bookdown::{
    try_parse_bookdown_definition, try_parse_bookdown_text_reference,
};
use panache_parser::parser::inlines::core::parse_inline_text_recursive;
use panache_parser::parser::inlines::links::try_parse_bare_uri;

pub(super) fn smart_token_text<'a>(
    token: &'a SyntaxToken,
    context: &'a FormatContext,
) -> Cow<'a, str> {
    match context.smart_punctuation {
        SmartPunctuation::Preserve => Cow::Borrowed(token.text()),
        SmartPunctuation::Ascii | SmartPunctuation::Unicode => context
            .punctuation
            .get(&u32::from(token.text_range().start()))
            .map(|text| Cow::Borrowed(text.as_str()))
            .unwrap_or_else(|| Cow::Borrowed(token.text())),
    }
}

pub(super) fn smart_node_text(node: &SyntaxNode, context: &FormatContext) -> String {
    if context.smart_punctuation == SmartPunctuation::Preserve {
        return node.text().to_string();
    }
    node.descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .map(|token| smart_token_text(&token, context).into_owned())
        .collect()
}

fn contains_offset(ranges: &[Range<usize>], offset: usize) -> bool {
    ranges
        .get(ranges.partition_point(|range| range.end <= offset))
        .is_some_and(|range| range.contains(&offset))
}

fn find_bare_link_ranges(
    text: &str,
    mut offset: usize,
    bare_link_starts: &[usize],
    is_prose: impl Fn(usize) -> bool,
) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    for word in text.split_inclusive(char::is_whitespace) {
        let uri = word.match_indices(':').any(|(colon, _)| {
            let scheme = word[..colon]
                .rsplit(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '+' | '-' | '.'))
                .next()
                .unwrap_or_default();
            let start = colon - scheme.len();
            is_prose(offset + start)
                && is_prose(offset + colon)
                && try_parse_bare_uri(&word[start..]).is_some()
        });
        let www = word.match_indices("www.").any(|(start, _)| {
            is_prose(offset + start)
                && is_prose(offset + start + 4)
                && word[start + 4..]
                    .chars()
                    .next()
                    .is_some_and(char::is_alphanumeric)
        });
        let email = word.match_indices('@').any(|(at, _)| {
            at > 0
                && is_prose(offset + at)
                && word[at + 1..]
                    .match_indices('.')
                    .any(|(start, _)| is_prose(offset + at + 1 + start))
        });
        let range = offset..offset + word.trim_end().len();
        let bare_link = bare_link_starts
            .get(bare_link_starts.partition_point(|&start| start < range.start))
            .is_some_and(|start| range.contains(start));
        if uri || www || email || bare_link {
            // Preserve addresses even when bare autolinking is disabled.
            // Keep attached punctuation too: converting trailing `...`
            // to `…` would make the ellipsis part of the link target.
            ranges.push(range);
        }
        offset += word.len();
    }
    ranges
}

fn prose_bare_link_ranges(node: &SyntaxNode) -> Vec<Range<usize>> {
    let mut prose_ranges = Vec::new();
    let mut bare_link_starts = Vec::new();
    for element in node.descendants_with_tokens() {
        match element {
            NodeOrToken::Token(token) if is_prose_token(&token) => {
                prose_ranges.push(token.text_range().into());
            }
            NodeOrToken::Node(node) => {
                if let Some(link) = AutoLink::cast(node)
                    && link
                        .syntax()
                        .first_token()
                        .is_some_and(|token| token.kind() == SyntaxKind::TEXT)
                {
                    bare_link_starts.push(usize::from(link.syntax().text_range().start()));
                }
            }
            _ => {}
        }
    }
    // Inspect the complete source word so inline delimiters cannot split a
    // protected URL. Recognize unparsed URLs only in prose, so destinations and
    // attributes cannot suppress conversion in adjacent link text.
    find_bare_link_ranges(
        &node.text().to_string(),
        usize::from(node.text_range().start()),
        &bare_link_starts,
        |offset| contains_offset(&prose_ranges, offset),
    )
}

fn relative_bare_link_ranges(token: &SyntaxToken, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let start = usize::from(token.text_range().start());
    let end = usize::from(token.text_range().end());
    ranges[ranges.partition_point(|range| range.end <= start)..]
        .iter()
        .take_while(|range| range.start < end)
        .map(|range| range.start.max(start) - start..range.end.min(end) - start)
        .collect()
}

fn convert_outside_bare_links<'a>(
    token: &'a SyntaxToken,
    ranges: &[Range<usize>],
    mut convert: impl FnMut(&'a str, usize) -> Cow<'a, str>,
) -> Cow<'a, str> {
    let text = token.text();
    if ranges.is_empty() {
        return convert(text, 0);
    }
    let mut out = String::with_capacity(text.len());
    let mut start = 0;
    for range in ranges {
        out.push_str(&convert(&text[start..range.start], start));
        out.push_str(&text[range.clone()]);
        start = range.end;
    }
    out.push_str(&convert(&text[start..], start));
    Cow::Owned(out)
}

fn ascii_punctuation(text: &str, smart: bool, smart_quotes: bool) -> Cow<'_, str> {
    let quotes = smart || smart_quotes;
    if !(quotes && text.contains(['‘', '’', '“', '”']) || smart && text.contains(['–', '—', '…']))
    {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '‘' | '’' if quotes => out.push('\''),
            '“' | '”' if quotes => out.push('"'),
            '–' if smart => out.push_str("--"),
            '—' if smart => out.push_str("---"),
            '…' if smart => out.push_str("..."),
            _ => out.push(ch),
        }
    }
    Cow::Owned(out)
}

fn is_prose_root(kind: SyntaxKind) -> bool {
    Paragraph::can_cast(kind)
        || Plain::can_cast(kind)
        || Figure::can_cast(kind)
        || HeadingContent::can_cast(kind)
        || TableCell::can_cast(kind)
        || TableCaption::can_cast(kind)
        || LineBlockLine::can_cast(kind)
        || matches!(kind, SyntaxKind::TERM | SyntaxKind::INLINE_FOOTNOTE)
}

fn is_prose_container(kind: SyntaxKind) -> bool {
    is_prose_root(kind)
        || matches!(
            kind,
            SyntaxKind::EMPHASIS
                | SyntaxKind::STRONG
                | SyntaxKind::STRIKEOUT
                | SyntaxKind::MARK
                | SyntaxKind::SUPERSCRIPT
                | SyntaxKind::SUBSCRIPT
        )
        || LinkText::can_cast(kind)
        || ImageAlt::can_cast(kind)
        || kind == SyntaxKind::SPAN_CONTENT
}

fn is_prose_token(token: &SyntaxToken) -> bool {
    token.kind() == SyntaxKind::TEXT && token.parent().is_some_and(|p| is_prose_container(p.kind()))
}

fn is_implicit_reference(node: &SyntaxNode) -> bool {
    let reference = if let Some(link) = Link::cast(node.clone()) {
        if link.dest().is_some() {
            return false;
        }
        link.reference()
    } else if let Some(image) = ImageLink::cast(node.clone()) {
        if image.dest().is_some() {
            return false;
        }
        image.reference()
    } else {
        return false;
    };
    reference.is_none_or(|label| label.label().is_empty())
}

fn is_lookup_label(token: &SyntaxToken, config: &Config) -> bool {
    // Implicit reference text doubles as its lookup key, including nested markup.
    if token
        .parent_ancestors()
        .any(|node| is_implicit_reference(&node))
    {
        return true;
    }
    // Bookdown labels are isolated TEXT tokens rather than dedicated CST nodes.
    config.parser_extensions.bookdown_references
        && try_parse_bookdown_definition(token.text())
            .or_else(|| try_parse_bookdown_text_reference(token.text()))
            .is_some_and(|(len, _)| len == token.text().len())
}

fn escape_conflicting_reference_brackets(
    nodes: &[SyntaxNode],
    config: &FormatContext,
    punctuation: &mut HashMap<u32, String>,
) {
    if config.dialect() != Dialect::CommonMark
        || config
            .refdef_labels
            .as_ref()
            .is_none_or(|labels| labels.is_empty())
        || !nodes
            .iter()
            .flat_map(SyntaxNode::descendants_with_tokens)
            .filter_map(NodeOrToken::into_token)
            .any(|token| is_prose_token(&token) && token.text().contains('['))
    {
        return;
    }

    let mut text = String::new();
    let mut brackets = BTreeMap::new();
    let mut changed = false;
    for node in nodes {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        for token in node
            .descendants_with_tokens()
            .filter_map(NodeOrToken::into_token)
        {
            if token.kind() == SyntaxKind::LINE_PREFIX {
                continue;
            }
            let start = u32::from(token.text_range().start());
            let converted = punctuation.get(&start);
            changed |= converted.is_some();
            let converted = converted.map_or(token.text(), String::as_str);
            if is_prose_token(&token) && !is_lookup_label(&token, config) {
                for (offset, _) in converted.match_indices('[') {
                    brackets.insert(text.len() + offset, (token.clone(), offset));
                }
            }
            text.push_str(converted);
        }
    }
    if !changed {
        return;
    }

    // CommonMark emits unresolved brackets as TEXT. Reuse its inline resolver
    // to check whether normalization would turn those literals into links.
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::PLAIN.into());
    parse_inline_text_recursive(&mut builder, &text, &config.parser_options(), false);
    builder.finish_node();
    let tree = SyntaxNode::new_root(builder.finish());
    let creates_reference = tree.descendants().any(|node| {
        let image = if let Some(link) = Link::cast(node.clone()) {
            if link.dest().is_some() {
                return false;
            }
            false
        } else if let Some(image) = ImageLink::cast(node.clone()) {
            if image.dest().is_some() {
                return false;
            }
            true
        } else {
            return false;
        };
        let start = usize::from(node.text_range().start()) + usize::from(image);
        brackets.contains_key(&start)
    });
    if !creates_reference {
        return;
    }
    // Escape all literal openings in this prose root: escaping just the new
    // reference could expose its label or enclosing brackets as another link.
    for (token, offset) in brackets.into_values().rev() {
        let converted = punctuation
            .entry(u32::from(token.text_range().start()))
            .or_insert_with(|| token.text().to_string());
        // An entity survives both direct and wrapped emission without becoming
        // a link delimiter or a backslash-math opener on the next parse.
        converted.replace_range(offset..offset + 1, "&#91;");
    }
}

// The view omits syntax delimiters and treats verbatim inlines as opaque
// atoms, so quotes can surround markup without inspecting its payload.
#[derive(Clone, Copy)]
struct Character {
    ch: char,
    source: Option<(u32, usize)>,
}

fn collect_characters(
    node: &SyntaxNode,
    chars: &mut Vec<Character>,
    tokens: &mut Vec<SyntaxToken>,
    config: &Config,
    protected: &[Range<usize>],
) {
    for child in node.children_with_tokens() {
        if matches!(
            child.kind(),
            SyntaxKind::EMPHASIS_MARKER
                | SyntaxKind::STRONG_MARKER
                | SyntaxKind::STRIKEOUT_MARKER
                | SyntaxKind::MARK_MARKER
                | SyntaxKind::SUPERSCRIPT_MARKER
                | SyntaxKind::SUBSCRIPT_MARKER
        ) {
            continue;
        }
        // Destinations and attributes are syntax, not neighbors in the
        // rendered prose used to decide quote direction.
        if matches!(node.kind(), SyntaxKind::LINK | SyntaxKind::IMAGE_LINK)
            && !LinkText::can_cast(child.kind())
            && !ImageAlt::can_cast(child.kind())
        {
            continue;
        }
        if matches!(
            node.kind(),
            SyntaxKind::BRACKETED_SPAN | SyntaxKind::INLINE_HTML_SPAN
        ) && child.kind() != SyntaxKind::SPAN_CONTENT
        {
            continue;
        }
        match child {
            NodeOrToken::Token(token) if is_prose_token(&token) => {
                if is_lookup_label(&token, config) {
                    chars.push(Character {
                        ch: '\u{fffc}',
                        source: None,
                    });
                    continue;
                }
                let start = u32::from(token.text_range().start());
                let mut protected = relative_bare_link_ranges(&token, protected)
                    .into_iter()
                    .peekable();
                chars.extend(token.text().char_indices().map(|(offset, ch)| {
                    if protected.peek().is_some_and(|range| range.end <= offset) {
                        protected.next();
                    }
                    if protected
                        .peek()
                        .is_some_and(|range| range.contains(&offset))
                    {
                        Character {
                            ch: '\u{fffc}',
                            source: None,
                        }
                    } else {
                        Character {
                            ch,
                            source: Some((start, offset)),
                        }
                    }
                }));
                tokens.push(token);
            }
            NodeOrToken::Token(token) => {
                let ch = match token.kind() {
                    SyntaxKind::WHITESPACE
                    | SyntaxKind::NEWLINE
                    | SyntaxKind::HARD_LINE_BREAK
                    | SyntaxKind::NONBREAKING_SPACE => ' ',
                    SyntaxKind::ESCAPED_CHAR => token.text().chars().nth(1).unwrap_or('\u{fffc}'),
                    _ => continue,
                };
                chars.push(Character { ch, source: None });
            }
            NodeOrToken::Node(child)
                if !is_prose_root(child.kind())
                    && (is_prose_container(child.kind())
                        || matches!(
                            child.kind(),
                            SyntaxKind::LINK
                                | SyntaxKind::IMAGE_LINK
                                | SyntaxKind::BRACKETED_SPAN
                                | SyntaxKind::INLINE_HTML_SPAN
                        )) =>
            {
                collect_characters(&child, chars, tokens, config, protected);
            }
            // Tags separate quote boundaries without acting like a word or
            // whitespace, so quotes can also surround the complete element.
            NodeOrToken::Node(child) if child.kind() == SyntaxKind::INLINE_HTML => {
                chars.push(Character {
                    ch: '<',
                    source: None,
                });
            }
            NodeOrToken::Node(_) => chars.push(Character {
                ch: '\u{fffc}',
                source: None,
            }),
        }
    }
}

// Source offsets stay tied to the lossless CST even when emitted characters
// have different lengths. Only Unicode mode needs quote pairing across tokens.
pub(super) fn smart_text_map(tree: &SyntaxNode, config: &FormatContext) -> HashMap<u32, String> {
    let mut result = HashMap::new();
    if config.smart_punctuation == SmartPunctuation::Preserve
        || config.smart_punctuation == SmartPunctuation::Ascii
            && !(config.formatter_extensions.smart || config.formatter_extensions.smart_quotes)
    {
        return result;
    }
    let roots = tree.descendants().flat_map(|node| {
        // Heading text supplies both automatic IDs and Pandoc's implicit
        // reference labels, including punctuation inside nested markup.
        if config.smart_punctuation == SmartPunctuation::Unicode
            && (config.parser_extensions.auto_identifiers
                || config.parser_extensions.implicit_header_references)
            && node
                .ancestors()
                .any(|ancestor| HeadingContent::can_cast(ancestor.kind()))
        {
            return Vec::new();
        }
        if let Some(block) = LineBlock::cast(node.clone()) {
            logical_line_block_lines(&block)
                .into_iter()
                .map(|lines| {
                    lines
                        .into_iter()
                        .map(|line| line.syntax().clone())
                        .collect()
                })
                .collect()
        } else if is_prose_root(node.kind()) && !LineBlockLine::can_cast(node.kind()) {
            vec![vec![node]]
        } else {
            Vec::new()
        }
    });
    for nodes in roots {
        let protected: Vec<_> = nodes.iter().flat_map(prose_bare_link_ranges).collect();
        if config.smart_punctuation == SmartPunctuation::Ascii {
            for token in nodes
                .iter()
                .flat_map(SyntaxNode::descendants_with_tokens)
                .filter_map(NodeOrToken::into_token)
                .filter(|token| is_prose_token(token) && !is_lookup_label(token, config))
            {
                let ranges = relative_bare_link_ranges(&token, &protected);
                let converted = convert_outside_bare_links(&token, &ranges, |text, _| {
                    ascii_punctuation(
                        text,
                        config.formatter_extensions.smart,
                        config.formatter_extensions.smart_quotes,
                    )
                });
                if converted != token.text() {
                    result.insert(
                        u32::from(token.text_range().start()),
                        converted.into_owned(),
                    );
                }
            }
            continue;
        }
        let mut chars = Vec::new();
        let mut tokens = Vec::new();
        for node in &nodes {
            collect_characters(node, &mut chars, &mut tokens, config, &protected);
        }
        let mut replacements = HashMap::new();
        let mut double: Vec<usize> = Vec::new();
        let mut single: Vec<usize> = Vec::new();
        let is_word = |ch: char| ch.is_alphanumeric() || ch == '\u{fffc}';
        for (i, character) in chars.iter().enumerate() {
            let quote = character.ch;
            if !matches!(quote, '\'' | '"') || character.source.is_none() {
                continue;
            }
            let prev = i.checked_sub(1).map(|j| chars[j].ch);
            let next = chars.get(i + 1).map(|c| c.ch);
            if quote == '\'' {
                replacements.insert(character.source.unwrap(), '’');
                if prev.is_some_and(is_word) && next.is_some_and(is_word) {
                    continue;
                }
            }
            let stack = if quote == '"' {
                &mut double
            } else {
                &mut single
            };
            let can_open =
                prev.is_none_or(|c| !is_word(c)) && next.is_some_and(|c| !c.is_whitespace());
            let can_close =
                prev.is_some_and(|c| !c.is_whitespace()) && next.is_none_or(|c| !is_word(c));
            if can_close && let Some(open) = stack.pop() {
                replacements.insert(
                    chars[open].source.unwrap(),
                    if quote == '"' { '“' } else { '‘' },
                );
                replacements.insert(
                    character.source.unwrap(),
                    if quote == '"' { '”' } else { '’' },
                );
            } else if can_open {
                stack.push(i);
            }
        }
        for token in tokens {
            let start = u32::from(token.text_range().start());
            let ranges = relative_bare_link_ranges(&token, &protected);
            let converted = convert_outside_bare_links(&token, &ranges, |text, base| {
                let quoted: String = text
                    .char_indices()
                    .map(|(offset, ch)| {
                        replacements
                            .get(&(start, base + offset))
                            .copied()
                            .unwrap_or(ch)
                    })
                    .collect();
                Cow::Owned(unicode_dashes_and_ellipsis(&quoted))
            });
            if converted != token.text() {
                result.insert(start, converted.into_owned());
            }
        }
        escape_conflicting_reference_brackets(&nodes, config, &mut result);
    }
    result
}

fn unicode_dashes_and_ellipsis(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let (replacement, len) = if rest.starts_with("---") {
            ('—', 3)
        } else if rest.starts_with("--") {
            ('–', 2)
        } else if rest.starts_with("...") {
            ('…', 3)
        } else {
            let ch = rest.chars().next().unwrap();
            (ch, ch.len_utf8())
        };
        out.push(replacement);
        rest = &rest[len..];
    }
    out
}
