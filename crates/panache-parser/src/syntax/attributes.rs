use crate::parser::utils::attributes::{
    AttributeBlock, decode_html_attr_entities, parse_html_attribute_list,
    try_parse_trailing_attributes,
};
use crate::syntax::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode, SyntaxToken};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeEntry {
    Identifier(AttributeValue),
    Class(AttributeValue),
    KeyValue {
        key: AttributeValue,
        value: AttributeValue,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeValue {
    raw: String,
    value: String,
    range: rowan::TextRange,
}

impl AttributeValue {
    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn text_range(&self) -> rowan::TextRange {
        self.range
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AttributeNode(SyntaxNode);

impl AstNode for AttributeNode {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::ATTRIBUTE | SyntaxKind::DIV_INFO | SyntaxKind::HTML_ATTRS
        )
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        Self::can_cast(node.kind()).then(|| AttributeNode(node))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl AttributeNode {
    /// Whether this node carries structured `ATTR_*` children. Pandoc `{...}`
    /// attributes (`emit_attribute_node`), `DIV_INFO`, and `HTML_ATTRS`
    /// (`emit_html_attrs_node`) all do. Only opaque fallbacks (MMD `[#id]`
    /// headers, raw-inline `{=format}`, malformed bodies) keep a single inner
    /// text token and are read via the reparse helpers below.
    fn structured_id_token(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .find(|el| el.kind() == SyntaxKind::ATTR_ID)
            .and_then(|el| el.into_token())
    }

    fn has_structured_children(&self) -> bool {
        self.0.children_with_tokens().any(|el| {
            matches!(
                el.kind(),
                SyntaxKind::ATTR_ID
                    | SyntaxKind::ATTR_CLASS
                    | SyntaxKind::ATTR_UNNUMBERED
                    | SyntaxKind::ATTR_KEY_VALUE
            )
        })
    }

    /// Decode HTML character references in a value read straight off a
    /// structured `ATTR_*` token. Only `HTML_ATTRS` carries HTML attribute
    /// syntax; Pandoc `{...}` values are returned verbatim, since pandoc does
    /// not accept an `&`-bearing body as an attribute block at all. The
    /// [`reparse`](Self::reparse) path already decodes via
    /// [`parse_html_attribute_list`], so it must not be routed through here.
    fn decode_structured_value(&self, raw: &str) -> String {
        if self.0.kind() == SyntaxKind::HTML_ATTRS {
            decode_html_attr_entities(raw).into_owned()
        } else {
            raw.to_string()
        }
    }

    fn reparse(&self) -> Option<AttributeBlock> {
        let text = self.0.text().to_string();
        match self.0.kind() {
            SyntaxKind::HTML_ATTRS => parse_html_attribute_list(&text),
            _ => try_parse_trailing_attributes(&text).map(|(attrs, _)| attrs),
        }
    }

    pub fn id(&self) -> Option<String> {
        if self.has_structured_children() {
            return self
                .structured_id_token()
                .map(|token| attribute_token_value(self, &token, Some('#')).value)
                .filter(|id| !id.is_empty());
        }
        self.reparse()
            .and_then(|attrs| attrs.identifier)
            .filter(|id| !id.is_empty())
    }

    /// markdown-it-attrs accepts either shorthand or a named id. The first
    /// declaration wins, matching the rendered element's attribute order.
    pub fn vitepress_id(&self) -> Option<AttributeValue> {
        self.entries().into_iter().find_map(|entry| match entry {
            AttributeEntry::Identifier(value) => Some(value),
            AttributeEntry::KeyValue { key, value } if key.value() == "id" => Some(value),
            _ => None,
        })
    }

    /// Source-ordered attributes with cooked values and payload ranges.
    pub fn entries(&self) -> Vec<AttributeEntry> {
        if !self.has_structured_children() {
            let range = self.0.text_range();
            let mut entries = Vec::new();
            if let Some(id) = self.id() {
                entries.push(AttributeEntry::Identifier(AttributeValue {
                    raw: id.clone(),
                    value: id,
                    range: self.id_value_range().unwrap_or(range),
                }));
            }
            entries.extend(self.classes().into_iter().map(|class| {
                AttributeEntry::Class(AttributeValue {
                    raw: class.clone(),
                    value: class,
                    range,
                })
            }));
            entries.extend(self.key_values().into_iter().map(|(key, value)| {
                AttributeEntry::KeyValue {
                    key: AttributeValue {
                        raw: key.clone(),
                        value: key,
                        range,
                    },
                    value: AttributeValue {
                        raw: value.clone(),
                        value,
                        range,
                    },
                }
            }));
            return entries;
        }

        self.0
            .children_with_tokens()
            .filter_map(|element| match element {
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::ATTR_ID => Some(
                    AttributeEntry::Identifier(attribute_token_value(self, &token, Some('#'))),
                ),
                rowan::NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::ATTR_CLASS | SyntaxKind::ATTR_UNNUMBERED
                    ) =>
                {
                    let mut value = attribute_token_value(
                        self,
                        &token,
                        (token.kind() == SyntaxKind::ATTR_CLASS).then_some('.'),
                    );
                    if token.kind() == SyntaxKind::ATTR_UNNUMBERED {
                        value.value = "unnumbered".to_string();
                    }
                    Some(AttributeEntry::Class(value))
                }
                rowan::NodeOrToken::Node(node) if node.kind() == SyntaxKind::ATTR_KEY_VALUE => {
                    let key = node
                        .children_with_tokens()
                        .filter_map(|element| element.into_token())
                        .find(|token| token.kind() == SyntaxKind::ATTR_KEY)?;
                    let value = node
                        .children_with_tokens()
                        .filter_map(|element| element.into_token())
                        .find(|token| token.kind() == SyntaxKind::ATTR_VALUE)?;
                    Some(AttributeEntry::KeyValue {
                        key: attribute_token_value(self, &key, None),
                        value: attribute_quoted_value(self, &value),
                    })
                }
                _ => None,
            })
            .collect()
    }

    pub fn classes(&self) -> Vec<String> {
        if self.has_structured_children() {
            return self
                .0
                .children_with_tokens()
                .filter(|el| {
                    matches!(
                        el.kind(),
                        SyntaxKind::ATTR_CLASS | SyntaxKind::ATTR_UNNUMBERED
                    )
                })
                .filter_map(|el| el.into_token())
                .map(|t| {
                    if t.kind() == SyntaxKind::ATTR_UNNUMBERED {
                        return "unnumbered".to_string();
                    }
                    attribute_token_value(self, &t, Some('.')).value
                })
                .collect();
        }
        self.reparse().map(|a| a.classes).unwrap_or_default()
    }

    pub fn key_values(&self) -> Vec<(String, String)> {
        if self.has_structured_children() {
            return self
                .0
                .children()
                .filter(|n| n.kind() == SyntaxKind::ATTR_KEY_VALUE)
                .map(|kv| {
                    let key = child_token_text(&kv, SyntaxKind::ATTR_KEY).unwrap_or_default();
                    let value = child_token_text(&kv, SyntaxKind::ATTR_VALUE)
                        .map(|v| self.decode_structured_value(&strip_value_quotes(&v)))
                        .unwrap_or_default();
                    (key, value)
                })
                .collect();
        }
        self.reparse().map(|a| a.key_values).unwrap_or_default()
    }

    pub fn id_value_range(&self) -> Option<rowan::TextRange> {
        if self.has_structured_children() {
            let tok = self.structured_id_token()?;
            let r = tok.text_range();
            let lead = if self.0.kind() != SyntaxKind::HTML_ATTRS && tok.text().starts_with('#') {
                rowan::TextSize::from(1)
            } else {
                rowan::TextSize::from(0)
            };
            return Some(rowan::TextRange::new(r.start() + lead, r.end()));
        }

        let id = self.id()?;
        let text = self.0.text().to_string();
        let node_start: usize = self.0.text_range().start().into();
        match self.0.kind() {
            SyntaxKind::HTML_ATTRS => {
                let marker = text.find("id")?;
                let after_id = &text[marker + 2..];
                let eq_off = after_id.bytes().position(|b| b == b'=')?;
                let after_eq = &after_id[eq_off + 1..];
                let (val_offset_in_after_eq, val_len) = match after_eq.bytes().next() {
                    Some(q @ (b'"' | b'\'')) => {
                        let inner = &after_eq[1..];
                        let len = inner.bytes().position(|b| b == q).unwrap_or(inner.len());
                        (1, len)
                    }
                    _ => {
                        let len = after_eq
                            .bytes()
                            .position(|b| b.is_ascii_whitespace())
                            .unwrap_or(after_eq.len());
                        (0, len)
                    }
                };
                let value_start_in_text = marker + 2 + eq_off + 1 + val_offset_in_after_eq;
                let start = rowan::TextSize::from((node_start + value_start_in_text) as u32);
                let end =
                    rowan::TextSize::from((node_start + value_start_in_text + val_len) as u32);
                Some(rowan::TextRange::new(start, end))
            }
            _ => {
                let marker = text.find(&format!("#{}", id))?;
                let start = rowan::TextSize::from((node_start + marker + 1) as u32);
                let end = rowan::TextSize::from((node_start + marker + 1 + id.len()) as u32);
                Some(rowan::TextRange::new(start, end))
            }
        }
    }
}

fn attribute_token_value(
    attributes: &AttributeNode,
    token: &SyntaxToken,
    prefix: Option<char>,
) -> AttributeValue {
    let raw = token.text().to_string();
    let prefix_len = prefix
        .filter(|_| attributes.0.kind() != SyntaxKind::HTML_ATTRS)
        .filter(|prefix| raw.starts_with(*prefix))
        .map_or(0, char::len_utf8);
    let source_value = &raw[prefix_len..];
    let range = token.text_range();
    let value = attributes.decode_structured_value(source_value);
    AttributeValue {
        raw,
        value,
        range: rowan::TextRange::new(
            range.start() + rowan::TextSize::from(prefix_len as u32),
            range.end(),
        ),
    }
}

fn attribute_quoted_value(attributes: &AttributeNode, token: &SyntaxToken) -> AttributeValue {
    let raw = token.text().to_string();
    let quoted = raw.len() >= 2
        && matches!(raw.as_bytes().first(), Some(b'"' | b'\''))
        && raw.as_bytes().first() == raw.as_bytes().last();
    let (source_value, trim) = if quoted {
        (&raw[1..raw.len() - 1], 1u32)
    } else {
        (raw.as_str(), 0u32)
    };
    let range = token.text_range();
    let value = attributes.decode_structured_value(source_value);
    AttributeValue {
        raw,
        value,
        range: rowan::TextRange::new(
            range.start() + rowan::TextSize::from(trim),
            range.end() - rowan::TextSize::from(trim),
        ),
    }
}

fn child_token_text(node: &SyntaxNode, kind: SyntaxKind) -> Option<String> {
    node.children_with_tokens()
        .find(|el| el.kind() == kind)
        .and_then(|el| el.into_token())
        .map(|t| t.text().to_string())
}

fn strip_value_quotes(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 {
        let q = bytes[0];
        if (q == b'"' || q == b'\'') && bytes[bytes.len() - 1] == q {
            return raw[1..raw.len() - 1].to_string();
        }
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_node_extracts_div_info_id_and_range() {
        let config = crate::ParserOptions {
            flavor: crate::options::Flavor::RMarkdown,
            ..Default::default()
        };
        let tree = crate::parse("::: {#mu .exercise k=v}\ntext\n:::\n", Some(config));
        let node = tree
            .descendants()
            .find_map(AttributeNode::cast)
            .expect("attribute node");
        assert_eq!(node.syntax().kind(), SyntaxKind::DIV_INFO);
        assert!(node.has_structured_children());
        assert_eq!(node.id().as_deref(), Some("mu"));
        assert_eq!(node.classes(), vec!["exercise".to_string()]);
        assert_eq!(node.key_values(), vec![("k".to_string(), "v".to_string())]);

        let range = node.id_value_range().expect("id range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&tree.text().to_string()[start..end], "mu");
    }

    #[test]
    fn attribute_node_reads_structured_children() {
        let tree = crate::parse("# H {#x .a .b k=\"v w\"}\n", None);
        let node = tree
            .descendants()
            .find_map(AttributeNode::cast)
            .expect("attribute node");

        assert_eq!(node.id().as_deref(), Some("x"));
        assert_eq!(node.classes(), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            node.key_values(),
            vec![("k".to_string(), "v w".to_string())]
        );

        let range = node.id_value_range().expect("id range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&tree.text().to_string()[start..end], "x");
    }

    /// HTML attribute values carry decoded character references, matching
    /// pandoc's `Div`/`Span` `Attr`. The range stays in SOURCE bytes, so it
    /// still covers the encoded `a&amp;b` even though `id()` is 3 bytes.
    #[test]
    fn html_attrs_decode_entities_but_keep_source_range() {
        let config = crate::ParserOptions {
            flavor: crate::options::Flavor::Pandoc,
            ..Default::default()
        };
        let src = "<div id=\"a&amp;b\" class=\"c&lt;d\" k=\"e&gt;f\">\n\ntext\n\n</div>\n";
        let tree = crate::parse(src, Some(config));
        let node = tree
            .descendants()
            .filter_map(AttributeNode::cast)
            .find(|n| n.syntax().kind() == SyntaxKind::HTML_ATTRS)
            .expect("html attrs node");

        assert_eq!(node.id().as_deref(), Some("a&b"));
        assert_eq!(node.classes(), vec!["c<d".to_string()]);
        assert_eq!(
            node.key_values(),
            vec![("k".to_string(), "e>f".to_string())]
        );

        let range = node.id_value_range().expect("id range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&tree.text().to_string()[start..end], "a&amp;b");
    }

    /// Pandoc `{...}` attribute values are NOT HTML, so they are never
    /// entity-decoded.
    #[test]
    fn brace_attrs_do_not_decode_entities() {
        let tree = crate::parse("# H {#x k=\"a&amp;b\"}\n", None);
        let node = tree
            .descendants()
            .find_map(AttributeNode::cast)
            .expect("attribute node");
        assert_eq!(
            node.key_values(),
            vec![("k".to_string(), "a&amp;b".to_string())]
        );
    }
}
