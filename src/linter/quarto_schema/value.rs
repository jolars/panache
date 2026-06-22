//! Bridge from panache's YAML AST to the interpreter's [`SchemaValue`] tree.
//!
//! The interpreter is deliberately decoupled from the CST: it works on a small
//! value abstraction (type-plus-span, plus nested maps/sequences). This module
//! produces that abstraction from a parsed YAML region.
//!
//! Spans are kept in **host-document coordinates**. We reparse the region's
//! content string with [`parse_yaml_document`]—which yields content-local
//! offsets—and shift every range by the region's content start, so diagnostics
//! land on the right bytes of the original document.
//!
//! Scalar types follow the **YAML 1.2 core schema** (js-yaml semantics), since
//! Quarto reads frontmatter and cell options with js-yaml. Quoted scalars are
//! always strings; only plain scalars are type-resolved.

use rowan::{TextRange, TextSize};

use crate::syntax::{
    AstNode, SyntaxNode, YamlBlockMapEntry, YamlDocument, YamlFlowMapEntry, YamlNode, YamlScalar,
    YamlScalarStyle,
};

use super::interp::{MapEntry, ScalarType, SchemaValue, ValueKind};

/// Bridge the embedded YAML of a content node (`YAML_METADATA_CONTENT` or
/// `HASHPIPE_YAML_CONTENT`) to a [`SchemaValue`].
///
/// Spans come straight from the embedded CST, which is already in
/// host-document coordinates — including hashpipe, where the `#|` prefix is
/// carried as `YAML_LINE_PREFIX` trivia, so per-line offsets stay correct (a
/// reparse-and-shift of the prefix-stripped text would not). Returns `None`
/// when the region has no parsed YAML document (malformed YAML the parser left
/// as opaque tokens — already reported as a `yaml-*` diagnostic).
pub fn bridge_yaml_content(content_node: &SyntaxNode) -> Option<SchemaValue> {
    let doc = content_node.descendants().find_map(YamlDocument::cast)?;
    Some(bridge_node_or_empty(
        doc.as_node(),
        doc.syntax().text_range(),
        TextSize::new(0),
    ))
}

fn shift(range: TextRange, base: TextSize) -> TextRange {
    range + base
}

/// Bridge an optional content node, falling back to an empty (null) scalar at
/// `span` when the value is empty (`key:` with nothing after it).
fn bridge_node_or_empty(node: Option<YamlNode>, span: TextRange, base: TextSize) -> SchemaValue {
    match node {
        Some(n) => bridge_node(n, base),
        None => SchemaValue {
            span,
            kind: ValueKind::Scalar {
                ty: ScalarType::Null,
                literal: String::new(),
            },
        },
    }
}

fn bridge_node(node: YamlNode, base: TextSize) -> SchemaValue {
    match node {
        YamlNode::Scalar(s) => bridge_scalar(&s, base),
        YamlNode::BlockMap(m) => SchemaValue {
            span: shift(m.syntax().text_range(), base),
            kind: ValueKind::Map(bridge_block_entries(m.entries(), base)),
        },
        YamlNode::FlowMap(m) => SchemaValue {
            span: shift(m.syntax().text_range(), base),
            kind: ValueKind::Map(bridge_flow_entries(m.entries(), base)),
        },
        YamlNode::BlockSequence(s) => SchemaValue {
            span: shift(s.syntax().text_range(), base),
            kind: ValueKind::Seq(
                s.items()
                    .map(|item| bridge_node_or_empty(item.as_node(), item_span(&item, base), base))
                    .collect(),
            ),
        },
        YamlNode::FlowSequence(s) => SchemaValue {
            span: shift(s.syntax().text_range(), base),
            kind: ValueKind::Seq(
                s.items()
                    .map(|item| bridge_node_or_empty(item.as_node(), item_span(&item, base), base))
                    .collect(),
            ),
        },
    }
}

fn item_span<N: AstNode<Language = crate::syntax::PanacheLanguage>>(
    item: &N,
    base: TextSize,
) -> TextRange {
    shift(item.syntax().text_range(), base)
}

fn bridge_block_entries(
    entries: impl Iterator<Item = YamlBlockMapEntry>,
    base: TextSize,
) -> Vec<MapEntry> {
    let mut out = Vec::new();
    for entry in entries {
        let Some(key) = entry.key_text() else {
            continue;
        };
        let key_span = entry
            .key()
            .and_then(|k| k.scalar())
            .map(|s| shift(s.text_range(), base))
            .or_else(|| entry.key().map(|k| shift(k.syntax().text_range(), base)))
            .unwrap_or_else(|| shift(entry.syntax().text_range(), base));
        let value = match entry.value() {
            Some(v) => {
                bridge_node_or_empty(v.as_node(), shift(v.syntax().text_range(), base), base)
            }
            None => SchemaValue {
                span: key_span,
                kind: ValueKind::Scalar {
                    ty: ScalarType::Null,
                    literal: String::new(),
                },
            },
        };
        out.push(MapEntry {
            key,
            key_span,
            value,
        });
    }
    out
}

fn bridge_flow_entries(
    entries: impl Iterator<Item = YamlFlowMapEntry>,
    base: TextSize,
) -> Vec<MapEntry> {
    let mut out = Vec::new();
    for entry in entries {
        let Some(key) = entry.key_text() else {
            continue;
        };
        let key_span = entry
            .key()
            .and_then(|k| k.scalar())
            .map(|s| shift(s.text_range(), base))
            .unwrap_or_else(|| shift(entry.syntax().text_range(), base));
        let value = match entry.value() {
            Some(v) => {
                bridge_node_or_empty(v.as_node(), shift(v.syntax().text_range(), base), base)
            }
            None => SchemaValue {
                span: key_span,
                kind: ValueKind::Scalar {
                    ty: ScalarType::Null,
                    literal: String::new(),
                },
            },
        };
        out.push(MapEntry {
            key,
            key_span,
            value,
        });
    }
    out
}

fn bridge_scalar(scalar: &YamlScalar, base: TextSize) -> SchemaValue {
    let literal = scalar.value();
    let ty = if scalar.style() == YamlScalarStyle::Plain {
        classify_plain(&literal)
    } else {
        // Single/double-quoted, literal, and folded scalars are always strings.
        ScalarType::String
    };
    SchemaValue {
        span: shift(scalar.text_range(), base),
        kind: ValueKind::Scalar { ty, literal },
    }
}

/// Resolve a plain scalar's type per the YAML 1.2 core schema (what js-yaml
/// infers). Crucially, `yes`/`no`/`on`/`off` are **strings** in 1.2—Quarto does
/// not treat them as booleans—so they are not matched here.
fn classify_plain(s: &str) -> ScalarType {
    match s {
        "null" | "Null" | "NULL" | "~" | "" => return ScalarType::Null,
        "true" | "True" | "TRUE" | "false" | "False" | "FALSE" => return ScalarType::Bool,
        _ => {}
    }
    if is_core_int(s) {
        ScalarType::Int
    } else if is_core_float(s) {
        ScalarType::Float
    } else {
        ScalarType::String
    }
}

/// YAML 1.2 core integer: `[-+]?[0-9]+`, `0o[0-7]+`, or `0x[0-9a-fA-F]+`.
fn is_core_int(s: &str) -> bool {
    if let Some(rest) = s.strip_prefix("0o") {
        return !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit() && b < b'8');
    }
    if let Some(rest) = s.strip_prefix("0x") {
        return !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let digits = s.strip_prefix(['-', '+']).unwrap_or(s);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// YAML 1.2 core float: decimal with `.`/exponent, or `.inf`/`.nan` forms.
fn is_core_float(s: &str) -> bool {
    let body = s.strip_prefix(['-', '+']).unwrap_or(s);
    if matches!(body, ".inf" | ".Inf" | ".INF") || matches!(s, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    // Must look numeric AND carry a '.' or exponent (otherwise it's an int).
    let has_dot = s.contains('.');
    let has_exp = s.contains(['e', 'E']);
    if !has_dot && !has_exp {
        return false;
    }
    // Strip an exponent, then validate the mantissa is digits with at most one dot.
    let (mantissa, exp) = match s.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (s, None),
    };
    if let Some(exp) = exp {
        let exp_digits = exp.strip_prefix(['-', '+']).unwrap_or(exp);
        if exp_digits.is_empty() || !exp_digits.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    let mantissa = mantissa.strip_prefix(['-', '+']).unwrap_or(mantissa);
    let mut parts = mantissa.splitn(3, '.');
    let int_part = parts.next().unwrap_or("");
    let frac_part = parts.next().unwrap_or("");
    if parts.next().is_some() {
        return false; // more than one dot
    }
    if int_part.is_empty() && frac_part.is_empty() {
        return false;
    }
    int_part.bytes().all(|b| b.is_ascii_digit()) && frac_part.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_core_scalars() {
        assert_eq!(classify_plain("true"), ScalarType::Bool);
        assert_eq!(classify_plain("FALSE"), ScalarType::Bool);
        assert_eq!(classify_plain("null"), ScalarType::Null);
        assert_eq!(classify_plain("~"), ScalarType::Null);
        assert_eq!(classify_plain(""), ScalarType::Null);
        assert_eq!(classify_plain("42"), ScalarType::Int);
        assert_eq!(classify_plain("-7"), ScalarType::Int);
        assert_eq!(classify_plain("0x1f"), ScalarType::Int);
        assert_eq!(classify_plain("0o17"), ScalarType::Int);
        assert_eq!(classify_plain("1.5"), ScalarType::Float);
        assert_eq!(classify_plain("1e3"), ScalarType::Float);
        assert_eq!(classify_plain(".inf"), ScalarType::Float);
        // YAML 1.2: these are strings, not booleans.
        assert_eq!(classify_plain("yes"), ScalarType::String);
        assert_eq!(classify_plain("no"), ScalarType::String);
        assert_eq!(classify_plain("html"), ScalarType::String);
        assert_eq!(classify_plain("1.2.3"), ScalarType::String);
        assert_eq!(classify_plain("0x"), ScalarType::String);
    }

    /// Parse `input` and bridge its frontmatter region from the embedded CST.
    fn bridge_frontmatter(input: &str) -> SchemaValue {
        let tree = crate::parser::parse(input, None);
        let content = tree
            .descendants()
            .find(|n| n.kind() == crate::syntax::SyntaxKind::YAML_METADATA_CONTENT)
            .expect("frontmatter content node");
        bridge_yaml_content(&content).expect("bridge")
    }

    #[test]
    fn bridges_frontmatter_map_with_host_spans() {
        let input = "---\ntitle: Hello\ntoc: true\n---\n";
        let value = bridge_frontmatter(input);
        let ValueKind::Map(entries) = &value.kind else {
            panic!("expected map");
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "title");
        // The key span points at "title" in the host document.
        let r = entries[0].key_span;
        let start: usize = r.start().into();
        let end: usize = r.end().into();
        assert_eq!(&input[start..end], "title");
        // `toc: true` resolves to a boolean.
        assert!(matches!(
            &entries[1].value.kind,
            ValueKind::Scalar {
                ty: ScalarType::Bool,
                ..
            }
        ));
    }

    #[test]
    fn quoted_numbers_are_strings() {
        let value = bridge_frontmatter("---\nversion: \"1.0\"\n---\n");
        let ValueKind::Map(entries) = &value.kind else {
            panic!("expected map");
        };
        assert!(matches!(
            &entries[0].value.kind,
            ValueKind::Scalar {
                ty: ScalarType::String,
                ..
            }
        ));
    }

    #[test]
    fn bridges_nested_sequence_and_map() {
        let value = bridge_frontmatter(
            "---\nformat:\n  html:\n    toc: true\nauthors:\n  - Alice\n  - Bob\n---\n",
        );
        let ValueKind::Map(entries) = &value.kind else {
            panic!("expected map");
        };
        let format = entries.iter().find(|e| e.key == "format").unwrap();
        assert!(matches!(format.value.kind, ValueKind::Map(_)));
        let authors = entries.iter().find(|e| e.key == "authors").unwrap();
        let ValueKind::Seq(items) = &authors.value.kind else {
            panic!("expected seq");
        };
        assert_eq!(items.len(), 2);
    }
}
