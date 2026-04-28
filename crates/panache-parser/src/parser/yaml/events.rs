//! YAML event projection: walk a shadow-parser CST and produce a
//! yaml-test-suite style event stream (`+STR`, `+DOC`, `+MAP`, `=VAL :foo`,
//! ...).
//!
//! This module is parser-crate scoped and used only by the test harness in
//! `crates/panache-parser/tests/yaml.rs` for fixture parity. It reads the
//! green tree built by [`crate::parser::yaml::parse_yaml_tree`] and re-derives
//! event-stream semantics (tag resolution, anchor stripping, flow-seq
//! splitting). The intent is to keep the projection adjacent to the parser so
//! CST shape is the single source of truth for events.

use std::collections::HashMap;

use crate::syntax::{SyntaxKind, SyntaxNode};

use super::parser::parse_yaml_tree;

/// Per-document tag handle map: handle (`!!`, `!yaml!`, `!e!`) → URI prefix.
/// The secondary handle `!!` always defaults to `tag:yaml.org,2002:` per the
/// YAML 1.2 spec. Per-document `%TAG` directives override and add to this map.
type TagHandles = HashMap<String, String>;

fn default_tag_handles() -> TagHandles {
    let mut handles = HashMap::new();
    handles.insert("!!".to_string(), "tag:yaml.org,2002:".to_string());
    handles
}

/// Scan a `YAML_DOCUMENT` for `%TAG` directive lines and merge them into
/// the default handle map.
fn collect_tag_handles(doc: &SyntaxNode) -> TagHandles {
    let mut handles = default_tag_handles();
    for tok in doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
    {
        if tok.kind() != SyntaxKind::YAML_SCALAR {
            continue;
        }
        let line = tok.text().trim_start();
        let Some(rest) = line.strip_prefix("%TAG") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(handle) = parts.next() else { continue };
        let Some(prefix) = parts.next() else { continue };
        handles.insert(handle.to_string(), prefix.to_string());
    }
    handles
}

/// Resolve a tag shorthand (e.g. `!!str`, `!yaml!str`, `!e!foo`, `!local`) to
/// the long-form `<tag:...>` event token, consulting the per-document handle
/// map. Falls back to the built-in handling for unknown handles.
fn resolve_long_tag(tag: &str, handles: &TagHandles) -> Option<String> {
    if let Some(s) = long_tag_builtin(tag) {
        return Some(s);
    }
    let mut best: Option<(&str, &String)> = None;
    for (h, p) in handles {
        if tag.starts_with(h)
            && best.is_none_or(|(b_handle, _): (&str, _)| h.len() > b_handle.len())
        {
            best = Some((h.as_str(), p));
        }
    }
    if let Some((handle, prefix)) = best {
        let suffix = &tag[handle.len()..];
        return Some(format!("<{prefix}{suffix}>"));
    }
    None
}

/// Walk the shadow CST for `input` and return the projected yaml-test-suite
/// event stream. Returns an empty vector if the input fails to parse.
pub fn project_events(input: &str) -> Vec<String> {
    let Some(tree) = parse_yaml_tree(input) else {
        return Vec::new();
    };

    let mut events = vec!["+STR".to_string()];
    let stream = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_STREAM);
    if let Some(stream) = stream {
        for doc in stream
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
        {
            project_document(&doc, &mut events);
        }
    }
    events.push("-STR".to_string());
    events
}

fn project_document(doc: &SyntaxNode, out: &mut Vec<String>) {
    let has_doc_start = doc
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::YAML_DOCUMENT_START);
    let has_doc_end = doc
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::YAML_DOCUMENT_END);
    out.push(if has_doc_start {
        "+DOC ---".to_string()
    } else {
        "+DOC".to_string()
    });
    let handles = collect_tag_handles(doc);

    if let Some(seq_node) = doc
        .descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
    {
        out.push("+SEQ".to_string());
        project_block_sequence_items(&seq_node, &handles, out);
        out.push("-SEQ".to_string());
    } else if let Some(root_map) = doc
        .descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
    {
        let mut values = Vec::new();
        project_block_map_entries(&root_map, &handles, &mut values);
        if !values.is_empty() {
            out.push("+MAP".to_string());
            out.append(&mut values);
            out.push("-MAP".to_string());
        } else if let Some(flow_map) = doc
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
        {
            let mut flow_values = Vec::new();
            project_flow_map_entries(&flow_map, &handles, &mut flow_values);
            out.push("+MAP {}".to_string());
            out.append(&mut flow_values);
            out.push("-MAP".to_string());
        } else if let Some(flow_seq) = doc
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
            && let Some(items) = simple_flow_sequence_items(&flow_seq.text().to_string())
        {
            out.push("+SEQ []".to_string());
            for item in items {
                project_flow_seq_item(&item, &handles, out);
            }
            out.push("-SEQ".to_string());
        } else if let Some(scalar) = scalar_document_value(doc, &handles) {
            out.push(scalar);
        } else {
            out.push("=VAL :".to_string());
        }
    } else if let Some(flow_map) = doc
        .descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
    {
        out.push("+MAP {}".to_string());
        project_flow_map_entries(&flow_map, &handles, out);
        out.push("-MAP".to_string());
    } else if let Some(flow_seq) = doc
        .descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
        && let Some(items) = simple_flow_sequence_items(&flow_seq.text().to_string())
    {
        out.push("+SEQ []".to_string());
        for item in items {
            project_flow_seq_item(&item, &handles, out);
        }
        out.push("-SEQ".to_string());
    } else if let Some(scalar) = scalar_document_value(doc, &handles) {
        out.push(scalar);
    } else {
        out.push("=VAL :".to_string());
    }

    out.push(if has_doc_end {
        "-DOC ...".to_string()
    } else {
        "-DOC".to_string()
    });
}

fn scalar_document_value(doc: &SyntaxNode, handles: &TagHandles) -> Option<String> {
    // Skip `%TAG`/`%YAML` directive lines: those are document-level metadata,
    // not part of the scalar body.
    let text = doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| tok.kind() == SyntaxKind::YAML_SCALAR)
        .filter(|tok| !tok.text().trim_start().starts_with('%'))
        .map(|tok| tok.text().to_string())
        .collect::<Vec<_>>()
        .join("");
    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        // Tagged-but-empty scalar document still emits a `=VAL <tag> :` event.
        let tag_only = doc
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
            .map(|tok| tok.text().to_string());
        if let Some(tag) = tag_only
            && let Some(long) = resolve_long_tag(&tag, handles)
        {
            return Some(format!("=VAL {long} :"));
        }
        return None;
    }
    let tag_text = doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
        .map(|tok| tok.text().to_string());
    let event = if let Some(tag) = tag_text
        && let Some(long) = resolve_long_tag(&tag, handles)
    {
        if trimmed_text.starts_with('"') || trimmed_text.starts_with('\'') {
            let quoted = quoted_val_event(trimmed_text);
            // quoted_val_event returns `=VAL "body` — splice the tag in.
            quoted.replacen("=VAL ", &format!("=VAL {long} "), 1)
        } else {
            format!("=VAL {long} :{trimmed_text}")
        }
    } else if trimmed_text.starts_with('"') || trimmed_text.starts_with('\'') {
        quoted_val_event(&text)
    } else {
        plain_val_event(&text)
    };
    Some(event)
}

fn plain_val_event(text: &str) -> String {
    format!("=VAL :{}", text.replace('\\', "\\\\"))
}

/// Project a flow-collection scalar token, preserving quoted-scalar
/// classification when the source uses `"..."` or `'...'`. Plain scalars are
/// folded just like outside flow context. A leading tag shorthand (`!!str`,
/// `!handle!suffix`, `!local`) is resolved through `handles`.
fn flow_scalar_event(text: &str, handles: &TagHandles) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        return quoted_val_event(trimmed);
    }
    let (anchor, long_tag, body) = decompose_scalar(trimmed, handles);
    if anchor.is_some() || long_tag.is_some() {
        return scalar_event(anchor, long_tag.as_deref(), body);
    }
    plain_val_event(&fold_plain_scalar(text))
}

/// Split a leading tag shorthand (`!handle!suffix` or `!local`) off `text`,
/// returning `(tag, remainder)`. The tag must be terminated by whitespace or
/// end of input; otherwise `text` is returned as-is.
fn split_leading_tag(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix('!')?;
    let mut i = 0usize;
    let mut bangs = 0usize;
    for (idx, ch) in rest.char_indices() {
        if ch == '!' {
            bangs += 1;
            if bangs > 1 {
                return None;
            }
            i = idx + 1;
            continue;
        }
        if matches!(ch, ' ' | '\t' | '\n' | ',' | '}' | ']') {
            i = idx;
            break;
        }
        i = idx + ch.len_utf8();
    }
    let tag_len = 1 + i;
    let (tag, remainder) = text.split_at(tag_len);
    Some((tag, remainder))
}

/// Locate a flow-context key/value `:` indicator within a flow-sequence item.
/// Per YAML 1.2 a `:` is the mapping-key indicator only when followed by
/// whitespace or by end of the item; otherwise it's part of a plain scalar
/// (e.g. `http://foo.com`). Quoted regions are skipped.
fn flow_kv_split(item: &str) -> Option<(usize, usize)> {
    let bytes = item.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped_double = false;
    for (idx, ch) in item.char_indices() {
        if in_double {
            if escaped_double {
                escaped_double = false;
                continue;
            }
            match ch {
                '\\' => escaped_double = true,
                '"' => in_double = false,
                _ => {}
            }
            continue;
        }
        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            continue;
        }
        match ch {
            '\'' => in_single = true,
            '"' => in_double = true,
            ':' => {
                let next_off = idx + ch.len_utf8();
                let after_is_break = next_off >= bytes.len()
                    || matches!(bytes[next_off], b' ' | b'\t' | b'\n' | b'\r');
                if after_is_break {
                    return Some((idx, next_off));
                }
            }
            _ => {}
        }
    }
    None
}

/// Emit events for a single flow-sequence item: either `+MAP {} key val -MAP`
/// when the item is a flow-map entry (`key: value`, possibly with empty key
/// or value), or a single `=VAL` for a bare scalar.
fn project_flow_seq_item(item: &str, handles: &TagHandles, out: &mut Vec<String>) {
    if let Some((colon, after)) = flow_kv_split(item) {
        let raw_key_full = item[..colon].trim();
        // Strip the explicit-key `?` indicator (followed by whitespace or
        // end-of-key) when present.
        let raw_key = strip_explicit_key_indicator(raw_key_full);
        let raw_value = item[after..].trim();
        out.push("+MAP {}".to_string());
        if raw_key.is_empty() {
            out.push("=VAL :".to_string());
        } else {
            out.push(flow_scalar_event(raw_key, handles));
        }
        if raw_value.is_empty() {
            out.push("=VAL :".to_string());
        } else {
            out.push(flow_scalar_event(raw_value, handles));
        }
        out.push("-MAP".to_string());
    } else if item.trim_start().starts_with('"') || item.trim_start().starts_with('\'') {
        out.push(quoted_val_event(item.trim()));
    } else {
        out.push(plain_val_event(&fold_plain_scalar(item)));
    }
}

fn strip_explicit_key_indicator(key: &str) -> &str {
    let trimmed = key.trim_start();
    if let Some(rest) = trimmed.strip_prefix('?')
        && (rest.is_empty() || rest.starts_with([' ', '\t', '\n']))
    {
        return rest.trim_start();
    }
    key
}

fn quoted_val_event(text: &str) -> String {
    if text.starts_with('\'') {
        let trimmed = text.trim_end_matches('\'');
        let normalized = trimmed.replace("''", "'").replace('\\', "\\\\");
        format!("=VAL {normalized}")
    } else {
        let trimmed = text.trim_end_matches('"');
        let mut normalized = String::with_capacity(trimmed.len());
        let mut chars = trimmed.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                normalized.push(ch);
                continue;
            }

            let Some(next) = chars.next() else {
                normalized.push('\\');
                break;
            };

            match next {
                '/' => normalized.push('/'),
                '"' => normalized.push('"'),
                other => {
                    normalized.push('\\');
                    normalized.push(other);
                }
            }
        }
        format!("=VAL {normalized}")
    }
}

fn long_tag_builtin(tag: &str) -> Option<String> {
    if tag == "!" {
        return Some("<!>".to_string());
    }
    // Bare local tag: `!local` (single leading `!`, no second `!`).
    if let Some(rest) = tag.strip_prefix('!')
        && !rest.contains('!')
    {
        return Some(format!("<!{rest}>"));
    }
    None
}

fn simple_flow_sequence_items(text: &str) -> Option<Vec<String>> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    let inner = inner.trim();
    if inner.is_empty() {
        return Some(Vec::new());
    }

    let mut items = Vec::new();
    let mut start = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped_double = false;

    for (idx, ch) in inner.char_indices() {
        if in_double {
            if escaped_double {
                escaped_double = false;
                continue;
            }
            match ch {
                '\\' => escaped_double = true,
                '"' => in_double = false,
                _ => {}
            }
            continue;
        }

        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            continue;
        }

        match ch {
            '\'' => in_single = true,
            '"' => in_double = true,
            ',' => {
                let item = inner[start..idx].trim();
                if item.is_empty() {
                    return None;
                }
                items.push(item.to_string());
                start = idx + 1;
            }
            _ => {}
        }
    }

    let last = inner[start..].trim();
    if !last.is_empty() {
        items.push(last.to_string());
    }
    Some(items)
}

fn escape_block_scalar_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// If `value_node` encodes a literal (`|`) or folded (`>`) block scalar,
/// return the folded scalar body (no escaping applied yet). Scope: default
/// clip chomping, auto-detected content indent, no explicit indicators.
fn extract_block_scalar_body(value_node: &SyntaxNode) -> Option<(char, String)> {
    let tokens: Vec<_> = value_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| matches!(tok.kind(), SyntaxKind::YAML_SCALAR | SyntaxKind::NEWLINE))
        .collect();
    let first = tokens.first()?;
    if first.kind() != SyntaxKind::YAML_SCALAR {
        return None;
    }
    let indicator = match first.text() {
        "|" => '|',
        ">" => '>',
        _ => return None,
    };

    let mut raw = String::new();
    let mut seen_header = false;
    let mut skipped_header_newline = false;
    for tok in tokens.iter().skip(1) {
        if !seen_header && !skipped_header_newline && tok.kind() == SyntaxKind::NEWLINE {
            skipped_header_newline = true;
            seen_header = true;
            continue;
        }
        raw.push_str(tok.text());
    }

    let mut lines: Vec<&str> = raw.split('\n').collect();
    if lines.last().is_some_and(|s| s.is_empty()) {
        lines.pop();
    }

    let content_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().take_while(|c| *c == ' ').count())
        .min()
        .unwrap_or(0);

    let stripped: Vec<String> = lines
        .iter()
        .map(|l| {
            if l.len() >= content_indent {
                l[content_indent..].to_string()
            } else {
                String::new()
            }
        })
        .collect();

    let folded = match indicator {
        '|' => stripped.join("\n"),
        '>' => {
            let mut result = String::new();
            let mut last_blank = false;
            for (idx, line) in stripped.iter().enumerate() {
                if line.is_empty() {
                    result.push('\n');
                    last_blank = true;
                } else {
                    if idx > 0 && !last_blank {
                        result.push(' ');
                    }
                    result.push_str(line);
                    last_blank = false;
                }
            }
            result
        }
        _ => unreachable!(),
    };

    let trimmed = folded.trim_end_matches('\n');
    let body = if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    };
    Some((indicator, body))
}

fn fold_plain_scalar(text: &str) -> String {
    let mut pieces = Vec::new();
    for line in text.split('\n') {
        let trimmed = line.trim();
        // A line whose first non-blank character is `#` is a YAML comment
        // line (the lexer currently leaves these embedded in scalar token
        // text inside multi-line flow continuations); skip it from folding.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        pieces.push(trimmed.to_string());
    }
    if pieces.is_empty() {
        return String::new();
    }
    pieces.join(" ")
}

fn project_flow_map_entries(flow_map: &SyntaxNode, handles: &TagHandles, out: &mut Vec<String>) {
    for entry in flow_map
        .children()
        .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_ENTRY)
    {
        let key_node = entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_KEY)
            .expect("flow map key");
        let value_node = entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_VALUE)
            .expect("flow map value");

        let has_explicit_colon = key_node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .any(|tok| tok.kind() == SyntaxKind::YAML_COLON);

        let raw_key = key_node
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| matches!(tok.kind(), SyntaxKind::YAML_SCALAR | SyntaxKind::YAML_KEY))
            .map(|tok| tok.text().to_string())
            .collect::<Vec<_>>()
            .join("");

        if has_explicit_colon {
            // Strip the explicit-key `?` indicator (`{ ? foo : v }`) from
            // the projected key text. A bare `? :` entry (key reduces to
            // empty after stripping) projects to an empty `=VAL :`.
            let stripped_key = strip_explicit_key_indicator(raw_key.trim());
            if stripped_key.is_empty() {
                out.push("=VAL :".to_string());
            } else {
                out.push(flow_scalar_event(stripped_key, handles));
            }
            project_flow_map_value(&value_node, handles, out);
        } else {
            let raw_value = value_node
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
                .filter(|tok| tok.kind() == SyntaxKind::YAML_SCALAR)
                .map(|tok| tok.text().to_string())
                .collect::<Vec<_>>()
                .join("");
            let combined = format!("{raw_key}{raw_value}");
            let folded = fold_plain_scalar(&combined);
            let stripped = strip_explicit_key_indicator(&folded);
            if stripped.is_empty() {
                out.push("=VAL :".to_string());
            } else {
                out.push(plain_val_event(stripped));
            }
            out.push("=VAL :".to_string());
        }
    }
}

/// Project a `YAML_FLOW_MAP_VALUE` node, recursing into nested flow
/// collections (`+SEQ [] ... -SEQ`, `+MAP {} ... -MAP`) when present so that
/// multi-line nested flow values like `{ a: [ b, c, { d: [e, f] } ] }`
/// produce structured event streams instead of one slurped scalar.
fn project_flow_map_value(value_node: &SyntaxNode, handles: &TagHandles, out: &mut Vec<String>) {
    if let Some(flow_seq) = value_node
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
    {
        out.push("+SEQ []".to_string());
        project_flow_sequence_items_cst(&flow_seq, handles, out);
        out.push("-SEQ".to_string());
        return;
    }
    if let Some(nested_map) = value_node
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
    {
        out.push("+MAP {}".to_string());
        project_flow_map_entries(&nested_map, handles, out);
        out.push("-MAP".to_string());
        return;
    }

    let raw_value = value_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| tok.kind() == SyntaxKind::YAML_SCALAR)
        .map(|tok| tok.text().to_string())
        .collect::<Vec<_>>()
        .join("");
    out.push(flow_scalar_event(&raw_value, handles));
}

/// CST-walking variant of flow-sequence projection. Each
/// `YAML_FLOW_SEQUENCE_ITEM` may contain a nested `YAML_FLOW_SEQUENCE` /
/// `YAML_FLOW_MAP`; if neither is present we fall back to the text-based
/// `project_flow_seq_item` for plain/quoted scalar items.
fn project_flow_sequence_items_cst(
    flow_seq: &SyntaxNode,
    handles: &TagHandles,
    out: &mut Vec<String>,
) {
    for item in flow_seq
        .children()
        .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE_ITEM)
    {
        if let Some(nested_seq) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
        {
            out.push("+SEQ []".to_string());
            project_flow_sequence_items_cst(&nested_seq, handles, out);
            out.push("-SEQ".to_string());
            continue;
        }
        if let Some(nested_map) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
        {
            out.push("+MAP {}".to_string());
            project_flow_map_entries(&nested_map, handles, out);
            out.push("-MAP".to_string());
            continue;
        }
        // Build the item text from scalar/key tokens only so embedded
        // `YAML_COMMENT` tokens (e.g. `[ word1\n# comment\n, word2]`) do not
        // leak into the projected scalar value.
        let item_text: String = item
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| matches!(tok.kind(), SyntaxKind::YAML_SCALAR | SyntaxKind::YAML_KEY))
            .map(|tok| tok.text().to_string())
            .collect();
        project_flow_seq_item(&item_text, handles, out);
    }
}

fn project_block_sequence_items(
    seq_node: &SyntaxNode,
    handles: &TagHandles,
    out: &mut Vec<String>,
) {
    for item in seq_node
        .children()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
    {
        if let Some(nested_seq) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
        {
            out.push("+SEQ".to_string());
            project_block_sequence_items(&nested_seq, handles, out);
            out.push("-SEQ".to_string());
            continue;
        }
        if let Some(nested_map) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
        {
            out.push("+MAP".to_string());
            project_block_map_entries(&nested_map, handles, out);
            out.push("-MAP".to_string());
            continue;
        }
        if let Some(flow_seq) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
        {
            let flow_text = flow_seq.text().to_string();
            if let Some(flow_items) = simple_flow_sequence_items(&flow_text) {
                out.push("+SEQ []".to_string());
                for value in flow_items {
                    project_flow_seq_item(&value, handles, out);
                }
                out.push("-SEQ".to_string());
                continue;
            }
        }
        if let Some(flow_map) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
        {
            out.push("+MAP {}".to_string());
            project_flow_map_entries(&flow_map, handles, out);
            out.push("-MAP".to_string());
            continue;
        }
        let item_tag = item
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
            .map(|tok| tok.text().to_string());
        let scalar_text = item
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_SCALAR)
            .map(|tok| tok.text().to_string())
            .collect::<Vec<_>>()
            .join("");
        let scalar_trimmed = scalar_text.trim();
        let event = if scalar_trimmed.starts_with('*') {
            format!("=ALI {scalar_trimmed}")
        } else {
            // Combine the optional `YAML_TAG` token (already separated from
            // the scalar text by the parser) with anchors/tags found in the
            // scalar body, and render the YAML event in canonical
            // `&anchor <tag> :body` order.
            let item_long_tag = item_tag
                .as_deref()
                .and_then(|t| resolve_long_tag(t, handles));
            let (anchor, body_tag, body) = decompose_scalar(scalar_trimmed, handles);
            let long_tag = item_long_tag.or(body_tag);
            scalar_event(anchor, long_tag.as_deref(), body)
        };
        out.push(event);
    }
}

/// Decompose a node-property + scalar string into `(anchor, long_tag, body)`,
/// peeling off any leading `&anchor` and tag shorthand in either order
/// (`&a !!str foo` or `!!str &a foo`). Returns the raw body trimmed.
fn decompose_scalar<'a>(
    text: &'a str,
    handles: &TagHandles,
) -> (Option<&'a str>, Option<String>, &'a str) {
    let mut anchor: Option<&str> = None;
    let mut long_tag: Option<String> = None;
    let mut rest = text.trim();
    loop {
        if anchor.is_none()
            && let Some(after) = rest.strip_prefix('&')
        {
            let end = after
                .find(|c: char| c.is_whitespace() || matches!(c, ',' | '}' | ']'))
                .unwrap_or(after.len());
            let (name, tail) = after.split_at(end);
            anchor = Some(name);
            rest = tail.trim_start();
            continue;
        }
        if long_tag.is_none()
            && let Some((tag, tail)) = split_leading_tag(rest)
            && let Some(long) = resolve_long_tag(tag, handles)
        {
            long_tag = Some(long);
            rest = tail.trim_start();
            continue;
        }
        break;
    }
    (anchor, long_tag, rest)
}

/// Render a scalar event from its decomposed parts: optional anchor,
/// optional long-form tag (already in `<...>` form), and the scalar body.
/// Handles plain, double-quoted, and single-quoted bodies; quoted bodies
/// share the same escape normalization as [`quoted_val_event`].
fn scalar_event(anchor: Option<&str>, long_tag: Option<&str>, body: &str) -> String {
    let mut prefix = String::new();
    if let Some(a) = anchor {
        prefix.push_str(&format!("&{a} "));
    }
    if let Some(t) = long_tag {
        prefix.push_str(t);
        prefix.push(' ');
    }
    let body = body.trim();
    if body.is_empty() {
        return format!("=VAL {prefix}:");
    }
    if body.starts_with('"') || body.starts_with('\'') {
        // Reuse the shared escape/normalization rules; splice the prefix in
        // place of the leading `=VAL ` token.
        let quoted = quoted_val_event(body);
        return quoted.replacen("=VAL ", &format!("=VAL {prefix}"), 1);
    }
    format!("=VAL {prefix}:{body}")
}

fn project_block_map_entries(map_node: &SyntaxNode, handles: &TagHandles, out: &mut Vec<String>) {
    for entry in map_node
        .children()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
    {
        let key_node = entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
            .expect("key node");
        let value_node = entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
            .expect("value node");

        let key_tag = key_node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
            .map(|tok| tok.text().to_string());
        let key_text = key_node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::YAML_KEY)
            .map(|tok| tok.text().trim_end().to_string())
            .expect("key token");

        let key_event = if key_text.starts_with('*') {
            format!("=ALI {}", key_text.trim_end())
        } else {
            let key_long_tag = key_tag
                .as_deref()
                .and_then(|t| resolve_long_tag(t, handles));
            let (anchor, body_tag, body) = decompose_scalar(key_text.trim(), handles);
            let long_tag = key_long_tag.or(body_tag);
            scalar_event(anchor, long_tag.as_deref(), body)
        };
        out.push(key_event);

        if let Some(nested_map) = value_node
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
        {
            out.push("+MAP".to_string());
            project_block_map_entries(&nested_map, handles, out);
            out.push("-MAP".to_string());
            continue;
        }

        if let Some(flow_map) = value_node
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
        {
            out.push("+MAP {}".to_string());
            project_flow_map_entries(&flow_map, handles, out);
            out.push("-MAP".to_string());
            continue;
        }

        if let Some((indicator, body)) = extract_block_scalar_body(&value_node) {
            let escaped = escape_block_scalar_text(&body);
            out.push(format!("=VAL {indicator}{escaped}"));
            continue;
        }

        let value_tag = value_node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
            .map(|tok| tok.text().to_string());
        let value_text = value_node
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_SCALAR)
            .map(|tok| tok.text().to_string())
            .collect::<Vec<_>>()
            .join("");

        if value_tag.is_none()
            && let Some(items) = simple_flow_sequence_items(&value_text)
        {
            out.push("+SEQ []".to_string());
            for item in items {
                project_flow_seq_item(&item, handles, out);
            }
            out.push("-SEQ".to_string());
        } else if value_text.trim().is_empty() {
            if let Some(tag) = value_tag
                && let Some(long) = resolve_long_tag(&tag, handles)
            {
                out.push(format!("=VAL {long} :"));
            } else {
                out.push("=VAL :".to_string());
            }
        } else if value_text.trim_start().starts_with('*') {
            out.push(format!("=ALI {}", value_text.trim()));
        } else {
            let value_long_tag = value_tag
                .as_deref()
                .and_then(|t| resolve_long_tag(t, handles));
            let (anchor, body_tag, body) = decompose_scalar(value_text.trim(), handles);
            let long_tag = value_long_tag.or(body_tag);
            out.push(scalar_event(anchor, long_tag.as_deref(), body));
        }
    }
}
