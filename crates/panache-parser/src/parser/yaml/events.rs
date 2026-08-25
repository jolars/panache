//! YAML event projection: walk a YAML parser CST and produce a
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

use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use super::cooking;
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

fn collect_tag_handles(doc: &SyntaxNode) -> TagHandles {
    let mut handles = default_tag_handles();
    for tok in doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
    {
        if tok.kind() != SyntaxKind::YAML_DIRECTIVE {
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

fn resolve_long_tag(tag: &str, handles: &TagHandles) -> Option<String> {
    if let Some(inner) = tag.strip_prefix("!<").and_then(|t| t.strip_suffix('>')) {
        return Some(format!("<{}>", percent_decode_tag(inner)));
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
        let resolved = format!("{prefix}{suffix}");
        return Some(format!("<{}>", percent_decode_tag(&resolved)));
    }
    long_tag_builtin(tag)
}

/// Decode percent-encoded bytes (`%xx`) in a resolved tag URI. YAML 1.2 allows
/// percent-encoding in tag suffixes so callers can embed otherwise-special
/// characters (`!`, `:`, etc.); event-stream parity expects the decoded form.
fn percent_decode_tag(tag: &str) -> String {
    let bytes = tag.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) =
                (hex_digit_value(bytes[i + 1]), hex_digit_value(bytes[i + 2]))
        {
            out.push(hi * 16 + lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| tag.to_string())
}

fn hex_digit_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Walk the YAML CST for `input` and return the projected yaml-test-suite
/// event stream. Returns an empty vector if the input fails to parse.
pub fn project_events(input: &str) -> Vec<String> {
    let Some(tree) = parse_yaml_tree(input) else {
        return Vec::new();
    };
    project_events_from_tree(&tree)
}

/// Walk a YAML parser CST and return the projected yaml-test-suite event
/// stream. Decoupled from `parse_yaml_tree` so callers that already hold a
/// tree (e.g. yaml-test-suite parity checks) can reuse the same projection.
pub fn project_events_from_tree(tree: &SyntaxNode) -> Vec<String> {
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

fn doc_is_marker_only(doc: &SyntaxNode) -> bool {
    for el in doc.descendants_with_tokens() {
        if let Some(tok) = el.as_token() {
            match tok.kind() {
                SyntaxKind::WHITESPACE
                | SyntaxKind::NEWLINE
                | SyntaxKind::YAML_COMMENT
                | SyntaxKind::YAML_DOCUMENT_END
                | SyntaxKind::YAML_DOCUMENT_START => {}
                _ => return false,
            }
        }
    }
    true
}

fn flow_seq_preceding_block_map_at_doc_level(
    doc: &SyntaxNode,
    block_map: &SyntaxNode,
) -> Option<SyntaxNode> {
    let block_map_offset = block_map.text_range().start();
    doc.children()
        .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
        .find(|n| n.text_range().end() <= block_map_offset)
}

fn block_map_entry_key_is_empty(entry: &SyntaxNode) -> bool {
    let Some(key_node) = entry
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
    else {
        return false;
    };
    !key_node
        .children_with_tokens()
        .take_while(|el| el.as_token().map(|t| t.kind()) != Some(SyntaxKind::YAML_COLON))
        .any(|el| match el {
            rowan::NodeOrToken::Node(n) => {
                n.kind() == SyntaxKind::YAML_SCALAR && !n.text().to_string().trim().is_empty()
            }
            rowan::NodeOrToken::Token(t) => {
                matches!(t.kind(), SyntaxKind::YAML_KEY | SyntaxKind::YAML_TAG)
                    && !t.text().trim().is_empty()
            }
        })
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
    if !has_doc_start && doc_is_marker_only(doc) {
        return;
    }
    out.push(if has_doc_start {
        "+DOC ---".to_string()
    } else {
        "+DOC".to_string()
    });
    let handles = collect_tag_handles(doc);

    if let Some(seq_node) = doc
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
    {
        out.push(seq_open_event(&seq_node, &handles));
        project_block_sequence_items(&seq_node, &handles, out);
        out.push("-SEQ".to_string());
    } else if let Some(root_map) = doc
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
    {
        if let Some(flow_seq) = flow_seq_preceding_block_map_at_doc_level(doc, &root_map)
            && let Some(first_entry) = root_map
                .children()
                .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            && block_map_entry_key_is_empty(&first_entry)
        {
            out.push(map_open_event_for_block_map(&root_map, &handles));
            out.push("+SEQ []".to_string());
            project_flow_sequence_items_cst(&flow_seq, &handles, out);
            out.push("-SEQ".to_string());
            if let Some(value_node) = first_entry
                .children()
                .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
            {
                project_block_map_entry_value(&value_node, &handles, out);
            } else {
                out.push("=VAL :".to_string());
            }
            for entry in root_map
                .children()
                .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
                .skip(1)
            {
                project_block_map_entry(&entry, &handles, out);
            }
            out.push("-MAP".to_string());
        } else {
            let mut values = Vec::new();
            project_block_map_entries(&root_map, &handles, &mut values);
            if !values.is_empty() {
                out.push(map_open_event_for_block_map(&root_map, &handles));
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
            } else if let Some(scalar) = scalar_document_value(doc, &handles) {
                out.push(scalar);
            } else {
                out.push("=VAL :".to_string());
            }
        }
    } else if let Some(flow_collection) = doc.children().find(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_FLOW_MAP | SyntaxKind::YAML_FLOW_SEQUENCE
        )
    }) {
        let anchor = anchor_preceding_node(doc, &flow_collection);
        project_flow_collection_node_with_anchor(
            &flow_collection,
            anchor.as_deref(),
            &handles,
            out,
        );
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
    {
        out.push("+SEQ []".to_string());
        project_flow_sequence_items_cst(&flow_seq, &handles, out);
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
    if let Some((indicator, body)) = extract_scalar_doc_block_body(doc) {
        let escaped = escape_block_scalar_text(&body);
        return Some(format!("=VAL {indicator}{escaped}"));
    }
    if let Some((indicator, body)) = extract_top_level_block_body(doc) {
        let escaped = escape_block_scalar_text(&body);
        return Some(format!("=VAL {indicator}{escaped}"));
    }
    let text = doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR_TEXT
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::WHITESPACE
            )
        })
        .filter(|tok| !tok.text().trim_start().starts_with('%'))
        .map(|tok| tok.text().to_string())
        .collect::<Vec<_>>()
        .join("");
    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
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
    let multi_line_text = collect_scalar_source(doc);
    let is_multi_line_quoted = multi_line_text.contains('\n')
        && (trimmed_text.starts_with('"') || trimmed_text.starts_with('\''));
    let event = if let Some(tag) = tag_text
        && let Some(long) = resolve_long_tag(&tag, handles)
    {
        if trimmed_text.starts_with('"') || trimmed_text.starts_with('\'') {
            let quoted = if is_multi_line_quoted {
                quoted_val_event_multi_line(&multi_line_text)
            } else {
                quoted_val_event(trimmed_text)
            };
            quoted.replacen("=VAL ", &format!("=VAL {long} "), 1)
        } else {
            let folded = fold_plain_document_lines(doc);
            let (anchor, _, body) = decompose_scalar(folded.trim_start(), handles);
            scalar_event(anchor, Some(&long), &escape_block_scalar_text(body))
        }
    } else if is_multi_line_quoted {
        quoted_val_event_multi_line(&multi_line_text)
    } else if trimmed_text.starts_with('"') || trimmed_text.starts_with('\'') {
        quoted_val_event(&text)
    } else {
        let folded = fold_plain_document_lines(doc);
        let (anchor, body_tag, body) = decompose_scalar(folded.trim_start(), handles);
        if anchor.is_some() || body_tag.is_some() {
            scalar_event(anchor, body_tag.as_deref(), &escape_block_scalar_text(body))
        } else {
            format!("=VAL :{}", escape_block_scalar_text(&folded))
        }
    };
    Some(event)
}

fn collect_scalar_source(node: &SyntaxNode) -> String {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR_TEXT
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::NEWLINE
            )
        })
        .map(|tok| tok.text().to_string())
        .collect()
}

fn plain_val_event(text: &str) -> String {
    format!("=VAL :{}", text.replace('\\', "\\\\"))
}

fn fold_plain_document_lines(doc: &SyntaxNode) -> String {
    let raw: String = doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR_TEXT
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::WHITESPACE
                    | SyntaxKind::NEWLINE
            )
        })
        .map(|tok| tok.text().to_string())
        .collect();

    let mut out = String::with_capacity(raw.len());
    let mut empty_run: usize = 0;
    let mut have_content = false;
    for line in raw.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if have_content {
                empty_run += 1;
            }
            continue;
        }
        if !have_content {
            out.push_str(trimmed);
            have_content = true;
        } else if empty_run == 0 {
            out.push(' ');
            out.push_str(trimmed);
        } else {
            for _ in 0..empty_run {
                out.push('\n');
            }
            out.push_str(trimmed);
        }
        empty_run = 0;
    }
    out
}

fn flow_scalar_event(text: &str, handles: &TagHandles) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        if trimmed.contains('\n') {
            return quoted_val_event_multi_line(trimmed);
        }
        return quoted_val_event(trimmed);
    }
    if trimmed.starts_with('*') {
        return format!("=ALI {trimmed}");
    }
    let (anchor, long_tag, body) = decompose_scalar(trimmed, handles);
    if anchor.is_some() || long_tag.is_some() {
        return scalar_event(anchor, long_tag.as_deref(), body);
    }
    plain_val_event(&cooking::cook_plain(text))
}

/// Split a leading tag shorthand (`!handle!suffix` or `!local`) off `text`,
/// returning `(tag, remainder)`. The tag must be terminated by whitespace or
/// end of input; otherwise `text` is returned as-is.
fn split_leading_tag(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix('!')?;
    if let Some(uri) = rest.strip_prefix('<') {
        let close = uri.find('>')?;
        return Some(text.split_at(2 + close + 1));
    }
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
                let key_is_json_like = item[..idx].trim_end().ends_with(['"', '\'']);
                if after_is_break || key_is_json_like {
                    return Some((idx, next_off));
                }
            }
            _ => {}
        }
    }
    None
}

fn project_flow_seq_item(item: &str, handles: &TagHandles, out: &mut Vec<String>) {
    if let Some((colon, after)) = flow_kv_split(item) {
        let raw_key_full = item[..colon].trim();
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
        let trimmed = item.trim();
        if trimmed.contains('\n') {
            out.push(quoted_val_event_multi_line(trimmed));
        } else {
            out.push(quoted_val_event(trimmed));
        }
    } else {
        out.push(flow_scalar_event(&cooking::cook_plain(item), handles));
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
        let inner = cooking::cook_single_quoted_single_line(text);
        format!("=VAL '{}", escape_for_event(&inner))
    } else {
        let inner = cooking::cook_double_quoted_single_line(text);
        format!("=VAL \"{}", escape_for_event(&inner))
    }
}

fn quoted_val_event_multi_line(raw: &str) -> String {
    let trimmed = raw.trim_start_matches([' ', '\t', '\n']);
    if trimmed.starts_with('\'') {
        let decoded = cooking::cook_single_quoted_multi_line(trimmed);
        format!("=VAL '{}", escape_for_event(&decoded))
    } else {
        let decoded = cooking::cook_double_quoted_multi_line(trimmed);
        format!("=VAL \"{}", escape_for_event(&decoded))
    }
}

fn escape_for_event(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{07}' => out.push_str("\\a"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0B}' => out.push_str("\\v"),
            '\u{0C}' => out.push_str("\\f"),
            '\u{1B}' => out.push_str("\\e"),
            '\0' => out.push_str("\\0"),
            other => out.push(other),
        }
    }
    out
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

fn extract_block_scalar_body(value_node: &SyntaxNode) -> Option<(char, String)> {
    let tokens: Vec<_> = value_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR_TEXT
                    | SyntaxKind::NEWLINE
                    | SyntaxKind::WHITESPACE
                    | SyntaxKind::YAML_COMMENT,
            )
        })
        .collect();
    fold_block_scalar_tokens(&tokens, block_scalar_parent_indent(value_node))
}

fn block_scalar_parent_indent(value_node: &SyntaxNode) -> usize {
    let target = match value_node.kind() {
        SyntaxKind::YAML_BLOCK_MAP_VALUE => value_node
            .parent()
            .filter(|p| p.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            .unwrap_or_else(|| value_node.clone()),
        _ => value_node.clone(),
    };
    column_of_node_start(&target)
}

fn column_of_node_start(node: &SyntaxNode) -> usize {
    let offset: usize = node.text_range().start().into();
    let root = node.ancestors().last().unwrap_or_else(|| node.clone());
    let text = root.text().to_string();
    let cap = offset.min(text.len());
    let prefix = &text[..cap];
    match prefix.rfind('\n') {
        Some(nl) => offset.saturating_sub(nl + 1),
        None => offset,
    }
}

fn extract_scalar_doc_block_body(doc: &SyntaxNode) -> Option<(char, String)> {
    let mut started = false;
    let mut tokens = Vec::new();
    for el in doc.descendants_with_tokens() {
        let Some(tok) = el.into_token() else { continue };
        if !started {
            if tok.kind() == SyntaxKind::YAML_DOCUMENT_START {
                started = true;
            }
            continue;
        }
        match tok.kind() {
            SyntaxKind::YAML_DOCUMENT_END => break,
            SyntaxKind::YAML_SCALAR_TEXT
            | SyntaxKind::NEWLINE
            | SyntaxKind::WHITESPACE
            | SyntaxKind::YAML_COMMENT => tokens.push(tok),
            _ => {}
        }
    }
    fold_block_scalar_tokens(&tokens, 0)
}

/// Detect a top-level (no `YAML_DOCUMENT_START` marker) block-scalar document
/// of the form `>\n …` or `|\n …`. Walks the document's content tokens and
/// applies block-scalar folding when the first scalar token is a bare
/// block-scalar header. Returns `None` otherwise so plain / quoted scalar
/// handling can proceed.
fn extract_top_level_block_body(doc: &SyntaxNode) -> Option<(char, String)> {
    if doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::YAML_DOCUMENT_START)
    {
        return None;
    }
    let tokens: Vec<_> = doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR_TEXT
                    | SyntaxKind::NEWLINE
                    | SyntaxKind::WHITESPACE
                    | SyntaxKind::YAML_COMMENT,
            )
        })
        .collect();
    // Same shape tolerance as `fold_block_scalar_tokens`: v1 emits the
    // header as a standalone scalar, v2 emits the whole block scalar
    // (header + newline + body) as a single token. Detect the header by
    // inspecting up to the first newline.
    let first = tokens.iter().find(|tok| {
        if tok.kind() != SyntaxKind::YAML_SCALAR_TEXT {
            return false;
        }
        let header_part = tok.text().split('\n').next().unwrap_or("");
        parse_block_scalar_indicator(header_part).is_some()
    })?;
    let _ = first;
    fold_block_scalar_tokens(&tokens, 0)
}

fn fold_block_scalar_tokens(
    tokens: &[SyntaxToken],
    parent_indent: usize,
) -> Option<(char, String)> {
    // Locate the header. v1 emits the header (`|`, `|+`, `>1` …) as a
    // standalone YAML_SCALAR token and the body as separate per-line
    // tokens. v2 emits the entire block scalar (header + newline + body)
    // as a single YAML_SCALAR token. Detect either shape by inspecting
    // the chars before the first `\n` of the candidate token.
    let header_idx = tokens.iter().position(|t| {
        if t.kind() != SyntaxKind::YAML_SCALAR_TEXT {
            return false;
        }
        let header_part = t.text().split('\n').next().unwrap_or("");
        parse_block_scalar_indicator(header_part).is_some()
    })?;
    let header_text = tokens[header_idx].text();
    let header_part = header_text.split('\n').next().unwrap_or("");

    // Reconstruct the body source. Including `WHITESPACE` and
    // `YAML_COMMENT` tokens preserves the indentation needed for
    // content-indent calculation and lets a `# ...` line at column 0
    // (DK3J) land inside the body, while a less-indented `# Comment`
    // after a fully-indented body region (7T8X) gets recognized as a
    // body terminator.
    let mut raw = String::new();
    let unified_token = header_text.len() > header_part.len();
    if unified_token {
        // v2 shape: peel the header and its trailing newline out of the
        // single token, keep the rest as the body prefix. Then append
        // any later tokens verbatim.
        raw.push_str(&header_text[header_part.len() + 1..]);
        for tok in &tokens[header_idx + 1..] {
            raw.push_str(tok.text());
        }
    } else {
        // v1 shape: skip the standalone header's trailing NEWLINE and
        // stitch every later token verbatim.
        let mut skipped_header_newline = false;
        for tok in &tokens[header_idx + 1..] {
            if !skipped_header_newline && tok.kind() == SyntaxKind::NEWLINE {
                skipped_header_newline = true;
                continue;
            }
            raw.push_str(tok.text());
        }
    }

    fold_block_scalar_raw(header_part, &raw, parent_indent)
}

fn fold_block_scalar_raw(
    header_part: &str,
    raw: &str,
    parent_indent: usize,
) -> Option<(char, String)> {
    let (indicator, chomp, explicit_indent) = parse_block_scalar_indicator(header_part)?;

    let raw_trailing_newlines = raw.chars().rev().take_while(|c| *c == '\n').count();

    let lines: Vec<&str> = raw.split('\n').collect();

    // Per YAML 1.2 §8.1.1.1, the content indentation level is set by the
    // first non-empty line of the contents — unless an explicit indent
    // indicator is given in the header, in which case the absolute
    // content indent is `parent_indent + m`. `parent_indent` is the
    // column of the parent block (block-map-entry or block-sequence-item)
    // that contains the block-scalar; nested map/seq values pick up
    // the right anchor (e.g. `- aaa: |2` → parent col 2 + 2 → 4).
    //
    // §6.1: indentation only counts as spaces. A tab (or other non-space
    // char) past the leading spaces is content, so a line like ` \t`
    // counts as non-empty with leading-space count 1 (Y79Y/001).
    // If every line is space-only, fall back to the max leading-space
    // count among all lines per §8.1.1.1 paragraph 2 (JEF9/01-02).
    let leading_spaces = |l: &str| l.chars().take_while(|c| *c == ' ').count();
    let content_indent = match explicit_indent {
        Some(m) => parent_indent + m,
        None => lines
            .iter()
            .find(|l| l.chars().any(|c| c != ' '))
            .map(|l| leading_spaces(l))
            .unwrap_or_else(|| lines.iter().map(|l| leading_spaces(l)).max().unwrap_or(0)),
    };

    // Truncate at the first non-empty line whose indentation drops below the
    // content indent — that's where the block scalar's body ends per spec.
    // Trailing blanks coming from the source are kept; only the synthetic
    // final empty produced by `split('\n')` over a trailing newline is
    // dropped (and only when we walked off the end of the input — when we
    // broke out early on a dedented line, the trailing blank is real).
    let mut body_lines: Vec<&str> = Vec::new();
    let mut seen_content = false;
    let mut broke_out = false;
    for line in lines.iter() {
        let is_blank = line.trim().is_empty();
        let indent = line.chars().take_while(|c| *c == ' ').count();
        if !is_blank && seen_content && indent < content_indent {
            broke_out = true;
            break;
        }
        body_lines.push(line);
        if !is_blank {
            seen_content = true;
        }
    }
    if !broke_out && body_lines.last().is_some_and(|s| s.is_empty()) {
        body_lines.pop();
    }

    let stripped: Vec<BlockBodyLine> = body_lines
        .iter()
        .map(|l| {
            // Always strip up to `content_indent` columns; for `|` style this
            // preserves trailing spaces past the content indent (T26H).
            let text = if l.len() >= content_indent {
                l[content_indent..].to_string()
            } else {
                String::new()
            };
            // "Blank" for folding is decided on the stripped text, not the
            // raw line: a line of pure whitespace less-indented than content
            // (e.g. ` ` with content_indent=2) strips to empty and is blank,
            // while a stripped tab (` \t` with content_indent=1 → `\t`) is
            // content, not blank. More-indented lines (per §8.1.3) preserve
            // literal line breaks; the spec defines them as content lines
            // beginning with extra whitespace, so we test the stripped text's
            // first character rather than counting only leading spaces (which
            // would miss tab-prefixed content like R4YG/MJS9).
            let is_blank = text.is_empty();
            let is_mi = !is_blank && text.starts_with([' ', '\t']);
            BlockBodyLine {
                text,
                is_blank,
                is_mi,
            }
        })
        .collect();

    let folded = match indicator {
        '|' => stripped
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        '>' => fold_greater_lines(&stripped),
        _ => unreachable!(),
    };

    let trimmed = folded.trim_end_matches('\n');
    let body = match chomp {
        BlockScalarChomp::Strip => trimmed.to_string(),
        BlockScalarChomp::Clip => {
            if trimmed.is_empty() {
                String::new()
            } else {
                format!("{trimmed}\n")
            }
        }
        BlockScalarChomp::Keep => {
            // Keep chomping preserves the line break after the last
            // content line plus one line break per trailing empty line.
            // "Empty" is checked on the stripped text (so a raw `  `
            // line stripped to ` ` is content, not empty).
            //
            // When there are no content lines (`seen_content == false`),
            // each whitespace-only body line still contributes one `\n`
            // (JEF9/02 produces `\n` even with no trailing source newline,
            // because the line break after the header is implicit). Fall
            // back to `raw_trailing_newlines` only when no body line was
            // captured at all (`|+\n` with no body source).
            let body_trailing_empty = stripped
                .iter()
                .rev()
                .take_while(|l| l.text.is_empty())
                .count();
            let count = if seen_content {
                body_trailing_empty + 1
            } else if !stripped.is_empty() {
                body_trailing_empty
            } else {
                raw_trailing_newlines
            };
            format!("{trimmed}{}", "\n".repeat(count))
        }
    };
    Some((indicator, body))
}

struct BlockBodyLine {
    text: String,
    is_blank: bool,
    is_mi: bool,
}

fn fold_greater_lines(lines: &[BlockBodyLine]) -> String {
    let mut out = String::new();
    let mut idx = 0usize;

    while idx < lines.len() && lines[idx].is_blank {
        out.push('\n');
        idx += 1;
    }
    if idx >= lines.len() {
        return out;
    }

    out.push_str(&lines[idx].text);
    let mut prev_is_mi = lines[idx].is_mi;
    idx += 1;

    while idx < lines.len() {
        let mut empty_count = 0usize;
        while idx < lines.len() && lines[idx].is_blank {
            empty_count += 1;
            idx += 1;
        }
        if idx >= lines.len() {
            break;
        }
        let line = &lines[idx];
        let mi_involved = prev_is_mi || line.is_mi;
        if mi_involved {
            for _ in 0..(empty_count + 1) {
                out.push('\n');
            }
        } else if empty_count == 0 {
            out.push(' ');
        } else {
            for _ in 0..empty_count {
                out.push('\n');
            }
        }
        out.push_str(&line.text);
        prev_is_mi = line.is_mi;
        idx += 1;
    }
    out
}

#[derive(Clone, Copy)]
enum BlockScalarChomp {
    Clip,
    Strip,
    Keep,
}

fn parse_block_scalar_indicator(text: &str) -> Option<(char, BlockScalarChomp, Option<usize>)> {
    let mut chars = text.chars().peekable();
    let indicator = match chars.next()? {
        '|' => '|',
        '>' => '>',
        _ => return None,
    };
    let mut chomp = BlockScalarChomp::Clip;
    let mut seen_chomp = false;
    let mut indent: Option<usize> = None;
    while let Some(&ch) = chars.peek() {
        match ch {
            '+' if !seen_chomp => {
                chomp = BlockScalarChomp::Keep;
                seen_chomp = true;
                chars.next();
            }
            '-' if !seen_chomp => {
                chomp = BlockScalarChomp::Strip;
                seen_chomp = true;
                chars.next();
            }
            '1'..='9' if indent.is_none() => {
                indent = Some(ch.to_digit(10).unwrap() as usize);
                chars.next();
            }
            ' ' | '\t' => {
                // Trailing whitespace + optional comment is allowed after
                // the indicators per YAML 1.2 §8.1.1 (the header line
                // can carry a comment, e.g. `| # description`).
                for rest in chars.by_ref() {
                    if rest == '#' {
                        // Rest of the header line is a comment — ignore.
                        return Some((indicator, chomp, indent));
                    }
                    if rest != ' ' && rest != '\t' {
                        return None;
                    }
                }
                return Some((indicator, chomp, indent));
            }
            _ => return None,
        }
    }
    Some((indicator, chomp, indent))
}

fn project_flow_map_entries(flow_map: &SyntaxNode, handles: &TagHandles, out: &mut Vec<String>) {
    // Walk the flow_map's children left-to-right, tracking any orphan
    // scalar text (`pending`) that sits between entries. A scalar that
    // isn't enclosed in a `YAML_FLOW_MAP_ENTRY` reaches us in two
    // shapes:
    //
    //   1. A multi-line plain scalar that the v2 scanner couldn't
    //      register as a simple-key candidate before the `:` arrived
    //      (NJ66, ZF4X, UDR7's `sky`, 8KB6, ...). In that case the
    //      following entry has an empty `KEY` (just the `:`), and the
    //      orphan IS the key — we merge them.
    //
    //   2. A standalone scalar with no `:` at all (`{a, b: c}` shape;
    //      8KB6's `single line, ...`). YAML 1.2 says this is a key with
    //      an implicit empty value, projecting as `=VAL :a` then
    //      `=VAL :`.
    //
    // Both shapes resolve to flushing `pending` either as the key of
    // the next empty-key entry or as a value-less standalone entry
    // (when we hit a `,` or `}` before a matching empty-key entry).
    let mut pending = String::new();
    let mut pending_has_content = false;
    // A flow-sequence/flow-map node sitting *between* entries is an
    // orphan collection key: `{[d, e]: f}` lands `[d, e]` as a sibling
    // node, then a separate empty-key entry carries the `:` and value
    // (SBG9). Hold it until the following entry so we project it as
    // that entry's key instead of dropping it on the `_ => {}` arm.
    let mut pending_key_collection: Option<SyntaxNode> = None;
    for child in flow_map.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::YAML_COMMENT => {
                    if pending_has_content {
                        pending.push_str(tok.text());
                    }
                }
                SyntaxKind::YAML_FLOW_INDICATOR => match tok.text() {
                    "{" | "}" => {}
                    "," if pending_has_content => {
                        flush_pending_orphan(&pending, handles, out);
                        pending.clear();
                        pending_has_content = false;
                    }
                    _ => {}
                },
                SyntaxKind::YAML_KEY => {
                    pending.push_str(tok.text());
                    pending_has_content = true;
                }
                _ => {}
            },
            // An orphan scalar (not wrapped in a `YAML_FLOW_MAP_ENTRY`)
            // accumulates into `pending` as the next entry's key.
            rowan::NodeOrToken::Node(node) if node.kind() == SyntaxKind::YAML_SCALAR => {
                pending.push_str(&node.text().to_string());
                pending_has_content = true;
            }
            rowan::NodeOrToken::Node(node) if node.kind() == SyntaxKind::YAML_FLOW_MAP_ENTRY => {
                if let Some(key_collection) = pending_key_collection.take() {
                    // The orphan collection is this entry's key; the
                    // entry itself contributes only the `:` and value.
                    project_flow_collection_node(&key_collection, handles, out);
                    if let Some(value_node) = node
                        .children()
                        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_VALUE)
                    {
                        project_flow_map_value(&value_node, handles, out);
                    } else {
                        out.push("=VAL :".to_string());
                    }
                } else {
                    project_flow_map_entry(
                        &node,
                        if pending_has_content {
                            Some(pending.as_str())
                        } else {
                            None
                        },
                        handles,
                        out,
                    );
                }
                pending.clear();
                pending_has_content = false;
            }
            rowan::NodeOrToken::Node(node)
                if matches!(
                    node.kind(),
                    SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
                ) =>
            {
                pending_key_collection = Some(node);
            }
            _ => {}
        }
    }
    // A trailing orphan collection with no following entry is a key
    // with an implicit empty value: `{[a, b]}` ≡ `{[a, b]: ~}`.
    if let Some(key_collection) = pending_key_collection.take() {
        project_flow_collection_node(&key_collection, handles, out);
        out.push("=VAL :".to_string());
    }
    if pending_has_content {
        flush_pending_orphan(&pending, handles, out);
    }
}

fn flush_pending_orphan(pending: &str, handles: &TagHandles, out: &mut Vec<String>) {
    let trimmed = pending.trim();
    if trimmed.is_empty() {
        return;
    }
    if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        if trimmed.contains('\n') {
            out.push(quoted_val_event_multi_line(trimmed));
        } else {
            out.push(quoted_val_event(trimmed));
        }
    } else {
        let folded = cooking::cook_plain(trimmed);
        let stripped = strip_explicit_key_indicator(&folded);
        if stripped.is_empty() {
            out.push("=VAL :".to_string());
        } else {
            out.push(flow_scalar_event(stripped, handles));
        }
    }
    out.push("=VAL :".to_string());
}

fn project_flow_map_entry(
    entry: &SyntaxNode,
    external_key: Option<&str>,
    handles: &TagHandles,
    out: &mut Vec<String>,
) {
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
    let key_has_content = key_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR_TEXT | SyntaxKind::YAML_KEY
            )
        });

    let key_collection = key_node.children().find(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
        )
    });
    if let Some(collection) = key_collection {
        if let Some(ext) = external_key {
            flush_pending_orphan(ext, handles, out);
        }
        let anchor = anchor_preceding_node(&key_node, &collection);
        project_flow_collection_node_with_anchor(&collection, anchor.as_deref(), handles, out);
        project_flow_map_value(&value_node, handles, out);
        return;
    }

    let mut raw_key = key_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR_TEXT
                    | SyntaxKind::YAML_KEY
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::WHITESPACE
                    | SyntaxKind::NEWLINE
            )
        })
        .map(|tok| tok.text().to_string())
        .collect::<Vec<_>>()
        .join("");

    if let Some(ext) = external_key
        && !key_has_content
    {
        raw_key = format!("{ext}{raw_key}");
    } else if let Some(ext) = external_key {
        flush_pending_orphan(ext, handles, out);
    }

    if has_explicit_colon {
        let key_for_classify = raw_key.trim();
        let stripped_key = strip_explicit_key_indicator(key_for_classify);
        if stripped_key.is_empty() {
            let key_tag = key_node
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
                .map(|tok| tok.text().to_string());
            if let Some(t) = key_tag
                && let Some(long) = resolve_long_tag(&t, handles)
            {
                out.push(format!("=VAL {long} :"));
            } else {
                out.push("=VAL :".to_string());
            }
        } else if stripped_key.starts_with('"') || stripped_key.starts_with('\'') {
            if stripped_key.contains('\n') {
                out.push(quoted_val_event_multi_line(stripped_key));
            } else {
                out.push(quoted_val_event(stripped_key));
            }
        } else {
            let folded = cooking::cook_plain(stripped_key);
            out.push(flow_scalar_event(&folded, handles));
        }
        project_flow_map_value(&value_node, handles, out);
    } else {
        let raw_value = value_node
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| {
                matches!(
                    tok.kind(),
                    SyntaxKind::YAML_SCALAR_TEXT | SyntaxKind::YAML_ANCHOR | SyntaxKind::YAML_ALIAS
                )
            })
            .map(|tok| tok.text().to_string())
            .collect::<Vec<_>>()
            .join("");
        let combined = format!("{raw_key}{raw_value}");
        let folded = cooking::cook_plain(&combined);
        let stripped = strip_explicit_key_indicator(&folded);
        if stripped.is_empty() {
            out.push("=VAL :".to_string());
        } else {
            out.push(plain_val_event(stripped));
        }
        out.push("=VAL :".to_string());
    }
}

fn project_flow_map_value(value_node: &SyntaxNode, handles: &TagHandles, out: &mut Vec<String>) {
    let decoration_tag = value_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
        .and_then(|tok| resolve_long_tag(tok.text(), handles));
    if let Some(flow_seq) = value_node
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
    {
        out.push(match decoration_tag {
            Some(t) => format!("+SEQ [] {t}"),
            None => "+SEQ []".to_string(),
        });
        project_flow_sequence_items_cst(&flow_seq, handles, out);
        out.push("-SEQ".to_string());
        return;
    }
    if let Some(nested_map) = value_node
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
    {
        out.push(match decoration_tag {
            Some(t) => format!("+MAP {{}} {t}"),
            None => "+MAP {}".to_string(),
        });
        project_flow_map_entries(&nested_map, handles, out);
        out.push("-MAP".to_string());
        return;
    }

    let raw_value = value_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR_TEXT
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::YAML_COLON
            )
        })
        .map(|tok| tok.text().to_string())
        .collect::<Vec<_>>()
        .join("");
    if raw_value.trim().is_empty() {
        let tag = value_node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
            .map(|tok| tok.text().to_string());
        if let Some(t) = tag
            && let Some(long) = resolve_long_tag(&t, handles)
        {
            out.push(format!("=VAL {long} :"));
            return;
        }
    }
    out.push(flow_scalar_event(&raw_value, handles));
}

fn project_flow_collection_node(node: &SyntaxNode, handles: &TagHandles, out: &mut Vec<String>) {
    project_flow_collection_node_with_anchor(node, None, handles, out);
}

fn project_flow_collection_node_with_anchor(
    node: &SyntaxNode,
    anchor: Option<&str>,
    handles: &TagHandles,
    out: &mut Vec<String>,
) {
    let parent_tag = node
        .parent()
        .and_then(|p| {
            p.children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
        })
        .and_then(|tok| resolve_long_tag(tok.text(), handles));
    let decoration_suffix = match (anchor, parent_tag) {
        (Some(a), Some(t)) => format!(" &{a} {t}"),
        (Some(a), None) => format!(" &{a}"),
        (None, Some(t)) => format!(" {t}"),
        (None, None) => String::new(),
    };
    match node.kind() {
        SyntaxKind::YAML_FLOW_SEQUENCE => {
            out.push(format!("+SEQ []{decoration_suffix}"));
            project_flow_sequence_items_cst(node, handles, out);
            out.push("-SEQ".to_string());
        }
        SyntaxKind::YAML_FLOW_MAP => {
            out.push(format!("+MAP {{}}{decoration_suffix}"));
            project_flow_map_entries(node, handles, out);
            out.push("-MAP".to_string());
        }
        _ => {}
    }
}

fn anchor_preceding_node(container: &SyntaxNode, target: &SyntaxNode) -> Option<String> {
    let mut anchor: Option<String> = None;
    for el in container.children_with_tokens() {
        match el {
            rowan::NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::YAML_ANCHOR => {
                    anchor = tok.text().strip_prefix('&').map(|s| s.to_string());
                }
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::YAML_COMMENT => {}
                _ => anchor = None,
            },
            rowan::NodeOrToken::Node(node) => {
                if node == *target {
                    return anchor;
                }
                anchor = None;
            }
        }
    }
    None
}

/// Project the value side of a flow-sequence single-pair map item:
/// everything after the item's first direct-child colon. A trailing
/// flow collection projects structurally; otherwise the scalar text
/// (possibly empty → `=VAL :`) is emitted inline.
fn project_flow_seq_item_pair_value(
    item: &SyntaxNode,
    handles: &TagHandles,
    out: &mut Vec<String>,
) {
    let mut seen_colon = false;
    let mut value_text = String::new();
    for el in item.children_with_tokens() {
        match el {
            rowan::NodeOrToken::Token(tok) => {
                if !seen_colon {
                    if tok.kind() == SyntaxKind::YAML_COLON {
                        seen_colon = true;
                    }
                    continue;
                }
                if matches!(
                    tok.kind(),
                    SyntaxKind::YAML_KEY | SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE
                ) {
                    value_text.push_str(tok.text());
                }
            }
            rowan::NodeOrToken::Node(node)
                if seen_colon
                    && matches!(
                        node.kind(),
                        SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
                    ) =>
            {
                project_flow_collection_node(&node, handles, out);
                return;
            }
            rowan::NodeOrToken::Node(node)
                if seen_colon && node.kind() == SyntaxKind::YAML_SCALAR =>
            {
                value_text.push_str(&node.text().to_string());
            }
            _ => {}
        }
    }
    project_inline_scalar(&value_text, handles, out);
}

fn project_flow_sequence_items_cst(
    flow_seq: &SyntaxNode,
    handles: &TagHandles,
    out: &mut Vec<String>,
) {
    for item in flow_seq
        .children()
        .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE_ITEM)
    {
        if let Some(key_collection) = item.children().next().filter(|n| {
            matches!(
                n.kind(),
                SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
            )
        }) && item
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .any(|tok| tok.kind() == SyntaxKind::YAML_COLON)
        {
            out.push("+MAP {}".to_string());
            project_flow_collection_node(&key_collection, handles, out);
            project_flow_seq_item_pair_value(&item, handles, out);
            out.push("-MAP".to_string());
            continue;
        }
        if let Some(nested_seq) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
        {
            let anchor = anchor_preceding_node(&item, &nested_seq);
            project_flow_collection_node_with_anchor(&nested_seq, anchor.as_deref(), handles, out);
            continue;
        }
        if let Some(nested_map) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
        {
            let anchor = anchor_preceding_node(&item, &nested_map);
            project_flow_collection_node_with_anchor(&nested_map, anchor.as_deref(), handles, out);
            continue;
        }
        let item_text: String = item
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| {
                matches!(
                    tok.kind(),
                    SyntaxKind::YAML_SCALAR_TEXT
                        | SyntaxKind::YAML_KEY
                        | SyntaxKind::YAML_COLON
                        | SyntaxKind::YAML_ANCHOR
                        | SyntaxKind::YAML_ALIAS
                        | SyntaxKind::YAML_TAG
                        | SyntaxKind::WHITESPACE
                        | SyntaxKind::NEWLINE
                )
            })
            .map(|tok| tok.text().to_string())
            .collect();
        project_flow_seq_item(&item_text, handles, out);
    }
}

fn project_inline_scalar(text: &str, handles: &TagHandles, out: &mut Vec<String>) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        out.push("=VAL :".to_string());
        return;
    }
    if trimmed.starts_with('*') {
        out.push(format!("=ALI {trimmed}"));
        return;
    }
    let (anchor, body_tag, body) = decompose_scalar(trimmed, handles);
    out.push(scalar_event(anchor, body_tag.as_deref(), body));
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
            let mut suffix = String::new();
            let anchor = item
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::YAML_ANCHOR)
                .and_then(|tok| tok.text().strip_prefix('&').map(str::to_owned));
            if let Some(a) = anchor {
                suffix.push_str(&format!(" &{a}"));
            }
            let tag = item
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
                .and_then(|tok| resolve_long_tag(tok.text(), handles));
            if let Some(t) = tag {
                suffix.push(' ');
                suffix.push_str(&t);
            }
            out.push(format!("+SEQ{suffix}"));
            project_block_sequence_items(&nested_seq, handles, out);
            out.push("-SEQ".to_string());
            continue;
        }
        if let Some(nested_map) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
        {
            out.push(map_open_event_for_block_map(&nested_map, handles));
            project_block_map_entries(&nested_map, handles, out);
            out.push("-MAP".to_string());
            continue;
        }
        if let Some(flow_seq) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
        {
            out.push("+SEQ []".to_string());
            project_flow_sequence_items_cst(&flow_seq, handles, out);
            out.push("-SEQ".to_string());
            continue;
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
        if let Some((indicator, body)) = extract_block_scalar_body(&item) {
            let escaped = escape_block_scalar_text(&body);
            out.push(format!("=VAL {indicator}{escaped}"));
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
            .filter(|tok| {
                matches!(
                    tok.kind(),
                    SyntaxKind::YAML_SCALAR_TEXT
                        | SyntaxKind::YAML_ANCHOR
                        | SyntaxKind::YAML_ALIAS
                        | SyntaxKind::WHITESPACE
                        | SyntaxKind::NEWLINE
                )
            })
            .map(|tok| tok.text().to_string())
            .collect::<Vec<_>>()
            .join("");
        let scalar_trimmed = scalar_text.trim();
        let event = if scalar_trimmed.starts_with('*') {
            format!("=ALI {scalar_trimmed}")
        } else {
            let item_long_tag = item_tag
                .as_deref()
                .and_then(|t| resolve_long_tag(t, handles));
            let (anchor, body_tag, body) = decompose_scalar(scalar_trimmed, handles);
            let long_tag = item_long_tag.or(body_tag);
            let folded;
            let body_for_event: &str = if body.contains('\n') {
                folded = cooking::cook_plain(body);
                &folded
            } else {
                body
            };
            scalar_event(anchor, long_tag.as_deref(), body_for_event)
        };
        out.push(event);
    }
}

fn seq_open_event(seq_node: &SyntaxNode, handles: &TagHandles) -> String {
    let mut anchor: Option<String> = None;
    let mut long_tag: Option<String> = None;
    absorb_preceding_anchor_and_tag(seq_node, handles, &mut anchor, &mut long_tag);
    for child in seq_node.children_with_tokens() {
        if let Some(node) = child.as_node()
            && node.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
        {
            break;
        }
        let Some(tok) = child.as_token() else {
            continue;
        };
        absorb_anchor_or_tag(tok, handles, &mut anchor, &mut long_tag);
    }
    let mut event = String::from("+SEQ");
    if let Some(a) = anchor {
        event.push_str(" &");
        event.push_str(&a);
    }
    if let Some(t) = long_tag {
        event.push(' ');
        event.push_str(&t);
    }
    event
}

fn absorb_preceding_anchor_and_tag(
    child: &SyntaxNode,
    handles: &TagHandles,
    anchor: &mut Option<String>,
    long_tag: &mut Option<String>,
) {
    let Some(parent) = child.parent() else {
        return;
    };
    let target_range = child.text_range();
    for el in parent.children_with_tokens() {
        if let Some(node) = el.as_node() {
            if node.text_range() == target_range {
                break;
            }
            continue;
        }
        if let Some(tok) = el.as_token() {
            absorb_anchor_or_tag(tok, handles, anchor, long_tag);
        }
    }
}

fn absorb_anchor_or_tag(
    tok: &SyntaxToken,
    handles: &TagHandles,
    anchor: &mut Option<String>,
    long_tag: &mut Option<String>,
) {
    match tok.kind() {
        SyntaxKind::YAML_ANCHOR => {
            if anchor.is_none() {
                *anchor = Some(tok.text().trim_start_matches('&').to_string());
            }
        }
        SyntaxKind::YAML_TAG => {
            let trimmed = tok.text().trim();
            if let Some(name) = trimmed.strip_prefix('&') {
                if anchor.is_none() {
                    *anchor = Some(name.to_string());
                }
            } else if trimmed.starts_with('!')
                && long_tag.is_none()
                && let Some(long) = resolve_long_tag(trimmed, handles)
            {
                *long_tag = Some(long);
            }
        }
        _ => {}
    }
}

fn map_open_event_for_value(value_node: &SyntaxNode, handles: &TagHandles) -> String {
    let (anchor, long_tag, _residual) = extract_leading_node_properties(value_node, handles);
    map_open_event_from_props(anchor.as_deref(), long_tag.as_deref())
}

fn map_open_event_from_props(anchor: Option<&str>, long_tag: Option<&str>) -> String {
    let mut event = String::from("+MAP");
    if let Some(a) = anchor {
        event.push_str(" &");
        event.push_str(a);
    }
    if let Some(t) = long_tag {
        event.push(' ');
        event.push_str(t);
    }
    event
}

fn extract_leading_node_properties(
    node: &SyntaxNode,
    handles: &TagHandles,
) -> (Option<String>, Option<String>, String) {
    let mut anchor: Option<String> = None;
    let mut long_tag: Option<String> = None;
    let mut residual = String::new();
    for child in node.children_with_tokens() {
        if let Some(node) = child.as_node()
            && matches!(
                node.kind(),
                SyntaxKind::YAML_BLOCK_MAP
                    | SyntaxKind::YAML_FLOW_MAP
                    | SyntaxKind::YAML_FLOW_SEQUENCE
            )
        {
            break;
        }
        if let Some(scalar) = child
            .as_node()
            .filter(|n| n.kind() == SyntaxKind::YAML_SCALAR)
        {
            let scalar_text = scalar.text().to_string();
            let mut rest = scalar_text.trim();
            loop {
                if anchor.is_none()
                    && let Some(after) = rest.strip_prefix('&')
                {
                    let end = after
                        .find(|c: char| c.is_whitespace() || matches!(c, ',' | '}' | ']'))
                        .unwrap_or(after.len());
                    anchor = Some(after[..end].to_string());
                    rest = after[end..].trim_start();
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
            let extra = rest.trim();
            if !extra.is_empty() {
                if !residual.is_empty() {
                    residual.push(' ');
                }
                residual.push_str(extra);
            }
            continue;
        }
        let Some(tok) = child.as_token() else {
            continue;
        };
        match tok.kind() {
            SyntaxKind::YAML_ANCHOR => {
                if anchor.is_none() {
                    anchor = Some(tok.text().trim_start_matches('&').to_string());
                }
            }
            SyntaxKind::YAML_TAG => {
                if long_tag.is_none()
                    && let Some(long) = resolve_long_tag(tok.text(), handles)
                {
                    long_tag = Some(long);
                }
            }
            _ => {}
        }
    }
    (anchor, long_tag, residual)
}

fn map_open_event_for_block_map(map_node: &SyntaxNode, handles: &TagHandles) -> String {
    let mut anchor: Option<String> = None;
    let mut long_tag: Option<String> = None;
    absorb_preceding_anchor_and_tag(map_node, handles, &mut anchor, &mut long_tag);
    for child in map_node.children_with_tokens() {
        if let Some(node) = child.as_node() {
            if node.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY {
                break;
            }
            if node.kind() == SyntaxKind::YAML_SCALAR {
                let text = node.text().to_string();
                let trimmed = text.trim();
                if trimmed.starts_with("? ") || trimmed == "?" {
                    break;
                }
            }
            continue;
        }
        let Some(tok) = child.as_token() else {
            continue;
        };
        absorb_anchor_or_tag(tok, handles, &mut anchor, &mut long_tag);
    }
    map_open_event_from_props(anchor.as_deref(), long_tag.as_deref())
}

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
        let quoted = quoted_val_event(body);
        return quoted.replacen("=VAL ", &format!("=VAL {prefix}"), 1);
    }
    format!("=VAL {prefix}:{}", escape_for_event(body))
}

fn project_block_map_entries(map_node: &SyntaxNode, handles: &TagHandles, out: &mut Vec<String>) {
    let children: Vec<_> = map_node.children_with_tokens().collect();
    let mut idx = 0;
    while idx < children.len() {
        match &children[idx] {
            rowan::NodeOrToken::Node(scalar)
                if scalar.kind() == SyntaxKind::YAML_SCALAR && {
                    let t = scalar.text().to_string();
                    let ts = t.trim_start();
                    ts.starts_with("? ") || ts == "?"
                } =>
            {
                let scalar_text = scalar.text().to_string();
                let body = scalar_text.trim_start().trim_start_matches('?').trim();
                if body.is_empty() {
                    out.push("=VAL :".to_string());
                } else {
                    let (anchor, body_tag, rest) = decompose_scalar(body, handles);
                    out.push(scalar_event(anchor, body_tag.as_deref(), rest));
                }
                idx += 1;
                let mut peek = idx;
                while peek < children.len() {
                    if let rowan::NodeOrToken::Token(t) = &children[peek] {
                        if matches!(
                            t.kind(),
                            SyntaxKind::NEWLINE | SyntaxKind::WHITESPACE | SyntaxKind::YAML_COMMENT
                        ) {
                            peek += 1;
                            continue;
                        }
                        if t.kind() == SyntaxKind::YAML_COLON {
                            let mut value_tag: Option<String> = None;
                            let mut value_text = String::new();
                            let mut value_end = peek + 1;
                            while value_end < children.len() {
                                match &children[value_end] {
                                    rowan::NodeOrToken::Token(vt) => {
                                        if vt.kind() == SyntaxKind::NEWLINE {
                                            break;
                                        }
                                        if vt.kind() == SyntaxKind::YAML_TAG && value_tag.is_none()
                                        {
                                            value_tag = Some(vt.text().to_string());
                                        } else if matches!(
                                            vt.kind(),
                                            SyntaxKind::YAML_ANCHOR
                                                | SyntaxKind::YAML_ALIAS
                                                | SyntaxKind::WHITESPACE
                                        ) {
                                            value_text.push_str(vt.text());
                                        }
                                        value_end += 1;
                                    }
                                    rowan::NodeOrToken::Node(vn)
                                        if vn.kind() == SyntaxKind::YAML_SCALAR =>
                                    {
                                        value_text.push_str(&vn.text().to_string());
                                        value_end += 1;
                                    }
                                    _ => break,
                                }
                            }
                            let trimmed = value_text.trim();
                            let value_long_tag = value_tag
                                .as_deref()
                                .and_then(|t| resolve_long_tag(t, handles));
                            if trimmed.is_empty() {
                                if let Some(long) = value_long_tag {
                                    out.push(format!("=VAL {long} :"));
                                } else {
                                    out.push("=VAL :".to_string());
                                }
                            } else if trimmed.starts_with('"') || trimmed.starts_with('\'') {
                                let quoted = quoted_val_event(trimmed);
                                if let Some(long) = value_long_tag {
                                    out.push(quoted.replacen("=VAL ", &format!("=VAL {long} "), 1));
                                } else {
                                    out.push(quoted);
                                }
                            } else {
                                let (anchor, body_tag, body) = decompose_scalar(trimmed, handles);
                                let long_tag = value_long_tag.or(body_tag);
                                out.push(scalar_event(anchor, long_tag.as_deref(), body));
                            }
                            idx = value_end;
                            break;
                        }
                    }
                    out.push("=VAL :".to_string());
                    break;
                }
                if peek >= children.len() {
                    out.push("=VAL :".to_string());
                }
            }
            rowan::NodeOrToken::Node(entry) if entry.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY => {
                project_block_map_entry(entry, handles, out);
                idx += 1;
            }
            _ => {
                idx += 1;
            }
        }
    }
}

fn project_block_map_key_collection(
    key_node: &SyntaxNode,
    handles: &TagHandles,
    out: &mut Vec<String>,
) -> bool {
    for child in key_node.children() {
        match child.kind() {
            SyntaxKind::YAML_BLOCK_SEQUENCE => {
                out.push(seq_open_event(&child, handles));
                project_block_sequence_items(&child, handles, out);
                out.push("-SEQ".to_string());
                return true;
            }
            SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP => {
                let anchor = anchor_preceding_node(key_node, &child);
                project_flow_collection_node_with_anchor(&child, anchor.as_deref(), handles, out);
                return true;
            }
            SyntaxKind::YAML_BLOCK_MAP => {
                out.push("+MAP".to_string());
                project_block_map_entries(&child, handles, out);
                out.push("-MAP".to_string());
                return true;
            }
            _ => {}
        }
    }
    false
}

fn project_block_map_entry(entry: &SyntaxNode, handles: &TagHandles, out: &mut Vec<String>) {
    let key_node = entry
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
        .expect("key node");
    let value_node = entry
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
        .expect("value node");

    if project_block_map_key_collection(&key_node, handles, out) {
        project_block_map_entry_value(&value_node, handles, out);
        return;
    }

    let key_tag = key_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
        .map(|tok| tok.text().to_string());
    let key_text = key_node
        .children_with_tokens()
        .take_while(|el| el.as_token().map(|t| t.kind()) != Some(SyntaxKind::YAML_COLON))
        .filter_map(|el| match el {
            rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::YAML_SCALAR => {
                Some(n.text().to_string())
            }
            rowan::NodeOrToken::Token(t)
                if matches!(
                    t.kind(),
                    SyntaxKind::YAML_KEY
                        | SyntaxKind::YAML_ANCHOR
                        | SyntaxKind::YAML_ALIAS
                        | SyntaxKind::WHITESPACE
                        | SyntaxKind::NEWLINE
                ) =>
            {
                Some(t.text().to_string())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    let key_text = key_text.trim_end().to_string();

    let key_trimmed = strip_explicit_key_indicator(key_text.trim());
    if key_trimmed.starts_with('*') {
        out.push(format!("=ALI {key_trimmed}"));
    } else if key_tag.is_none()
        && let Some((indicator, body)) = extract_block_scalar_body(&key_node)
    {
        out.push(format!(
            "=VAL {indicator}{}",
            escape_block_scalar_text(&body)
        ));
    } else {
        let key_long_tag = key_tag
            .as_deref()
            .and_then(|t| resolve_long_tag(t, handles));
        let (anchor, body_tag, body) = decompose_scalar(key_trimmed, handles);
        let long_tag = key_long_tag.or(body_tag);
        let folded;
        let body_for_event: &str = if body.contains('\n') {
            folded = cooking::fold_quoted_inner(body, false);
            &folded
        } else {
            body
        };
        out.push(scalar_event(anchor, long_tag.as_deref(), body_for_event));
    }

    project_block_map_entry_value(&value_node, handles, out);
}

fn project_block_map_entry_value(
    value_node: &SyntaxNode,
    handles: &TagHandles,
    out: &mut Vec<String>,
) {
    if let Some(nested_map) = value_node
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
    {
        out.push(map_open_event_for_value(value_node, handles));
        project_block_map_entries(&nested_map, handles, out);
        out.push("-MAP".to_string());
        return;
    }

    if let Some(nested_seq) = value_node
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
    {
        out.push(seq_open_event(&nested_seq, handles));
        project_block_sequence_items(&nested_seq, handles, out);
        out.push("-SEQ".to_string());
        return;
    }

    if let Some(flow_map) = value_node
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
    {
        let anchor = anchor_preceding_node(value_node, &flow_map);
        project_flow_collection_node_with_anchor(&flow_map, anchor.as_deref(), handles, out);
        return;
    }

    if let Some(flow_seq) = value_node
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
    {
        let anchor = anchor_preceding_node(value_node, &flow_seq);
        project_flow_collection_node_with_anchor(&flow_seq, anchor.as_deref(), handles, out);
        return;
    }

    if let Some((indicator, body)) = extract_block_scalar_body(value_node) {
        let mut prefix = String::new();
        let anchor_text = value_node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::YAML_ANCHOR)
            .map(|tok| tok.text().to_string());
        if let Some(anchor) = anchor_text.as_deref().and_then(|t| t.strip_prefix('&')) {
            prefix.push_str(&format!("&{anchor} "));
        }
        let tag_text = value_node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
            .map(|tok| tok.text().to_string());
        if let Some(tag) = tag_text
            && let Some(long) = resolve_long_tag(&tag, handles)
        {
            prefix.push_str(&long);
            prefix.push(' ');
        }
        let escaped = escape_block_scalar_text(&body);
        out.push(format!("=VAL {prefix}{indicator}{escaped}"));
        return;
    }

    let value_tag = value_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
        .map(|tok| tok.text().to_string());
    let value_text = value_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR_TEXT
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::WHITESPACE
                    | SyntaxKind::NEWLINE
            )
        })
        .map(|tok| tok.text().to_string())
        .collect::<Vec<_>>()
        .join("");

    if value_text.trim().is_empty() {
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
        let trimmed = value_text.trim();
        if trimmed.starts_with('"') || trimmed.starts_with('\'') {
            let multi_line_text = collect_scalar_source(value_node);
            let is_multi_line = multi_line_text
                .trim_end_matches(['\n', '\r', ' ', '\t'])
                .contains('\n');
            let quoted = if is_multi_line {
                quoted_val_event_multi_line(&multi_line_text)
            } else {
                quoted_val_event(trimmed)
            };
            if let Some(long) = value_long_tag {
                out.push(quoted.replacen("=VAL ", &format!("=VAL {long} "), 1));
            } else {
                out.push(quoted);
            }
        } else {
            let (anchor, body_tag, body) = decompose_scalar(trimmed, handles);
            let long_tag = value_long_tag.or(body_tag);
            let folded;
            let body_for_event: &str = if body.contains('\n') {
                let escaped_breaks = body.trim_start().starts_with('"');
                folded = cooking::fold_quoted_inner(body, escaped_breaks);
                &folded
            } else {
                body
            };
            out.push(scalar_event(anchor, long_tag.as_deref(), body_for_event));
        }
    }
}
