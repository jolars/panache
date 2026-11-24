//! Heading AST node wrappers.

use std::collections::HashSet;
use std::sync::LazyLock;
use unicode_general_category::{GeneralCategory, get_general_category};

use super::ast::support;
use super::{AstNode, AttributeValue, CodeSpan, PanacheLanguage, SyntaxKind, SyntaxNode};
use crate::parser::utils::attributes::decode_html_attr_entities;

static VITEPRESS_EMOJI_ALIASES: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    include_str!("vitepress-emoji-aliases.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .flat_map(str::split_whitespace)
        .collect()
});

// Shortcuts from markdown-it-emoji 3.0.0 lib/data/shortcuts.mjs.
// The alias data and shortcuts share the same license.
const VITEPRESS_EMOJI_SHORTCUTS: &[&str] = &[
    ">:-(", ">:(", ":-\"", ":\"", "</3", "<\\3", ":-/", ":/", ":'-(", ":'(", ":,-(", ":,(", ":-(",
    ":(", "<3", "]:-(", "]:(", "o:-)", "o:)", "O:-)", "O:)", "0:-)", "0:)", ":'-)", ":')", ":,-)",
    ":,)", ":'-D", ":'D", ":,-D", ":,D", ":-*", ":*", "x-)", "X-)", ":-|", ":|", ":-o", ":o",
    ":-O", ":O", ":-@", ":@", ":-D", ":D", ":-)", ":)", "]:-)", "]:)", ":,'-(", ":,'(", ";-(",
    ";(", ":-P", ":P", "8-)", "B-)", ",:-(", ",:(", ",:-)", ",:)", ":-S", ":s", ":-Z", ":z", ":-$",
    ":$", ";-)", ";)",
];

fn vitepress_shortcut_boundary(ch: char) -> bool {
    use GeneralCategory::*;
    matches!(
        get_general_category(ch),
        SpaceSeparator
            | LineSeparator
            | ParagraphSeparator
            | Control
            | ConnectorPunctuation
            | DashPunctuation
            | OpenPunctuation
            | ClosePunctuation
            | InitialPunctuation
            | FinalPunctuation
            | OtherPunctuation
    )
}

fn vitepress_emoji_len(text: &str, offset: usize, ch: char) -> Option<usize> {
    if !matches!(
        ch,
        ':' | '>' | '<' | ']' | 'o' | 'O' | '0' | 'x' | 'X' | ';' | '8' | 'B' | ','
    ) {
        return None;
    }
    let rest = &text[offset..];
    if ch == ':'
        && let Some((alias, _)) = rest[1..].split_once(':')
        && VITEPRESS_EMOJI_ALIASES.contains(alias)
    {
        return Some(alias.len() + 2);
    }
    VITEPRESS_EMOJI_SHORTCUTS
        .iter()
        .filter(|shortcut| shortcut.starts_with(ch) && rest.starts_with(**shortcut))
        .map(|shortcut| shortcut.len())
        .max()
        .filter(|len| {
            text[..offset]
                .chars()
                .next_back()
                .is_none_or(vitepress_shortcut_boundary)
                && rest[*len..]
                    .chars()
                    .next()
                    .is_none_or(vitepress_shortcut_boundary)
        })
}

fn append_vitepress_text(out: &mut String, raw: &str, standalone: bool) {
    // VitePress restores the source spelling of entities when it joins
    // adjacent text tokens. A single entity retains its decoded value.
    if standalone && raw.starts_with('&') && raw.find(';') == raw.len().checked_sub(1) {
        out.push_str(&decode_html_attr_entities(raw));
    } else {
        out.push_str(raw);
    }
}
pub struct Heading(SyntaxNode);

impl AstNode for Heading {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::HEADING
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

// VitePress linkifies an explicit web URL before applying attributes.
// When the URL ends at its authority, an adjacent attribute block is left
// outside that URL and attaches to the generated link.
fn ends_in_vitepress_linkified_authority(text: &str) -> bool {
    let Some(url) = text
        .rsplit(|ch: char| ch.is_whitespace() || matches!(ch, '(' | '[' | '<' | '"' | '\''))
        .next()
    else {
        return false;
    };
    let Some((scheme, authority)) = url.split_once("://") else {
        return false;
    };
    if !["http", "https", "ftp"]
        .iter()
        .any(|name| scheme.eq_ignore_ascii_case(name))
    {
        return false;
    }
    let authority = if let Some((auth, host)) = authority.split_once('@') {
        if auth.is_empty() || auth.contains(['/', '[', ']', '(', ')']) {
            return false;
        }
        host
    } else {
        authority
    };
    let host = if let Some((host, port)) = authority.rsplit_once(':') {
        if port.parse::<u16>().is_err() {
            return false;
        }
        host
    } else {
        authority
    };
    host.split('.').all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|ch| ch.is_alphanumeric() || ch == '-')
    })
}

impl Heading {
    /// Returns the heading level (1-6).
    pub fn level(&self) -> usize {
        for child in self.0.children() {
            if child.kind() == SyntaxKind::ATX_HEADING_MARKER {
                for token in child.children_with_tokens() {
                    if let Some(t) = token.as_token()
                        && t.kind() == SyntaxKind::ATX_HEADING_MARKER
                    {
                        return t.text().chars().filter(|&c| c == '#').count();
                    }
                }
            }
        }

        if let Some(text) = self
            .0
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|t| t.kind() == SyntaxKind::SETEXT_HEADING_UNDERLINE)
            .map(|t| t.text().to_string())
        {
            if text.starts_with('=') { 1 } else { 2 }
        } else {
            1 // Default to level 1
        }
    }

    /// Returns the heading content node if present.
    pub fn content(&self) -> Option<HeadingContent> {
        support::child(&self.0)
    }

    /// VitePress attaches an adjacent attribute block to the preceding inline
    /// element. A whitespace-separated block belongs to the heading.
    pub fn vitepress_attributes_attach_inline(&self) -> bool {
        let Some(attrs) = self.attributes() else {
            return false;
        };
        let Some(content) = self.content() else {
            return false;
        };
        attrs.syntax().text_range().start() == content.syntax().text_range().end()
            && content.syntax().last_child_or_token().is_some_and(|last| {
                matches!(
                    last.kind(),
                    SyntaxKind::LINK
                        | SyntaxKind::IMAGE_LINK
                        | SyntaxKind::AUTO_LINK
                        | SyntaxKind::INLINE_CODE
                        | SyntaxKind::EMPHASIS
                        | SyntaxKind::STRONG
                        | SyntaxKind::STRIKEOUT
                ) || (last.kind() == SyntaxKind::TEXT
                    && ends_in_vitepress_linkified_authority(&super::ast::raw_text(
                        content.syntax(),
                    )))
            })
    }

    pub fn vitepress_explicit_id(&self) -> Option<AttributeValue> {
        if self.vitepress_attributes_attach_inline() {
            return None;
        }
        self.attributes().and_then(|attrs| attrs.vitepress_id())
    }

    /// Returns the heading text as a string.
    pub fn text(&self) -> String {
        self.content().map(|c| c.text()).unwrap_or_default()
    }

    /// Returns the heading's reference label, preserving inline markup and
    /// omitting container prefixes.
    pub fn raw_label(&self) -> String {
        self.content()
            .map(|content| super::ast::raw_text(content.syntax()))
            .unwrap_or_default()
    }

    /// Returns the heading text range.
    pub fn text_range(&self) -> rowan::TextRange {
        self.0.text_range()
    }

    /// Returns heading text, or a placeholder when empty.
    pub fn title_or(&self, placeholder: &str) -> String {
        let text = self.text();
        if text.is_empty() {
            placeholder.to_string()
        } else {
            text
        }
    }

    /// Returns the text range of the ATX marker token (e.g. `###`), if this is an ATX heading.
    pub fn atx_marker_range(&self) -> Option<rowan::TextRange> {
        self.0
            .children()
            .find(|child| child.kind() == SyntaxKind::ATX_HEADING_MARKER)
            .and_then(|marker_node| {
                marker_node
                    .children_with_tokens()
                    .find_map(|el| el.as_token().map(|token| token.text_range()))
            })
    }
}

pub struct HeadingContent(SyntaxNode);

impl AstNode for HeadingContent {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::HEADING_CONTENT
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl HeadingContent {
    /// Returns the text content of the heading.
    pub fn text(&self) -> String {
        super::ast::visible_text_tokens(&self.0)
            .filter(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::TEXT
                        | SyntaxKind::INLINE_CODE_CONTENT
                        | SyntaxKind::INLINE_EXEC_CONTENT
                )
            })
            .map(|token| token.text().to_string())
            .collect()
    }

    /// Text and code from which VitePress derives the heading's identifier.
    pub fn vitepress_anchor_text(&self, emoji: bool) -> String {
        let mut text = String::new();
        let mut after_break = false;
        let tokens = super::ast::visible_text_tokens_excluding(&self.0, |kind| {
            matches!(
                kind,
                SyntaxKind::IMAGE_LINK
                    | SyntaxKind::LINK_DEST
                    | SyntaxKind::LINK_REF
                    | SyntaxKind::ATTRIBUTE
            )
        });
        for token in tokens {
            match token.kind() {
                SyntaxKind::TEXT
                    if token
                        .parent()
                        .is_none_or(|node| node.kind() != SyntaxKind::LINK) =>
                {
                    let prev = token.prev_sibling_or_token();
                    let next = token.next_sibling_or_token();
                    let mut raw = token.text();
                    if after_break {
                        raw = raw.trim_start_matches([' ', '\t']);
                    }
                    if next
                        .as_ref()
                        .is_some_and(|el| el.kind() == SyntaxKind::NEWLINE)
                        || (next.is_none() && token.parent().as_ref() == Some(&self.0))
                    {
                        raw = raw.trim_end_matches([' ', '\t']);
                    }
                    after_break = false;

                    let adjacent_text = |neighbor: Option<super::SyntaxElement>| {
                        neighbor.is_some_and(|el| {
                            matches!(el.kind(), SyntaxKind::TEXT | SyntaxKind::ESCAPED_CHAR)
                        })
                    };
                    let joins_previous = adjacent_text(prev);
                    let joins_next = adjacent_text(next);
                    let mut start = 0;
                    if emoji {
                        for (offset, ch) in raw.char_indices() {
                            if offset >= start
                                && let Some(len) = vitepress_emoji_len(raw, offset, ch)
                            {
                                append_vitepress_text(
                                    &mut text,
                                    &raw[start..offset],
                                    start > 0 || !joins_previous,
                                );
                                start = offset + len;
                            }
                        }
                    }
                    append_vitepress_text(
                        &mut text,
                        &raw[start..],
                        (start > 0 || !joins_previous) && !joins_next,
                    );
                }
                SyntaxKind::INLINE_CODE_MARKER => {
                    if let Some(span) = token.parent().and_then(CodeSpan::cast)
                        && span
                            .content_range()
                            .is_some_and(|range| range.start() == token.text_range().end())
                    {
                        after_break = false;
                        let code = span
                            .content()
                            .replace("\r\n", " ")
                            .replace(['\n', '\r'], " ");
                        // CommonMark removes one space from each end unless the
                        // span consists entirely of spaces.
                        let content = if code.starts_with(' ')
                            && code.ends_with(' ')
                            && code.bytes().any(|byte| byte != b' ')
                        {
                            &code[1..code.len() - 1]
                        } else {
                            &code
                        };
                        text.push_str(content);
                    }
                }
                SyntaxKind::ESCAPED_CHAR => {
                    after_break = false;
                    let escaped = token.text().strip_prefix('\\').unwrap_or(token.text());
                    let joined_ampersand = escaped == "&"
                        && [token.prev_sibling_or_token(), token.next_sibling_or_token()]
                            .into_iter()
                            .flatten()
                            .any(|neighbor| {
                                matches!(
                                    neighbor.kind(),
                                    SyntaxKind::TEXT | SyntaxKind::ESCAPED_CHAR
                                )
                            });
                    text.push_str(if joined_ampersand { "&amp;" } else { escaped });
                }
                SyntaxKind::NEWLINE | SyntaxKind::HARD_LINE_BREAK => after_break = true,
                _ => {}
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_title_or_returns_placeholder_for_empty_heading() {
        let tree = crate::parse("# \n", None);
        let heading = tree.descendants().find_map(Heading::cast).expect("heading");
        assert_eq!(heading.title_or("(empty)"), "(empty)");
    }

    #[test]
    fn setext_heading_levels_are_one_and_two() {
        let input = "Level 1\n=======\n\nLevel 2\n-------\n";
        let tree = crate::parse(input, None);
        let levels: Vec<usize> = tree
            .descendants()
            .filter_map(Heading::cast)
            .map(|h| h.level())
            .collect();
        assert_eq!(levels, vec![1, 2]);
    }

    #[test]
    fn heading_atx_marker_range_points_to_hashes() {
        let input = "### Title\n";
        let tree = crate::parse(input, None);
        let heading = tree.descendants().find_map(Heading::cast).expect("heading");
        let range = heading.atx_marker_range().expect("marker range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "###");
    }
}
