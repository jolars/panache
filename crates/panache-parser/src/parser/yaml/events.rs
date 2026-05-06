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

use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

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
/// map. Handles are checked first (so a `%TAG !` directive can override the
/// primary handle); we fall back to the built-in handling for unknown handles.
fn resolve_long_tag(tag: &str, handles: &TagHandles) -> Option<String> {
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
    // `--- |` / `--- >` packs a block-scalar header onto the directive-end
    // marker line. Detect that pattern first so the folded body (with proper
    // chomping) is emitted instead of a single-line plain scalar.
    if let Some((indicator, body)) = extract_scalar_doc_block_body(doc) {
        let escaped = escape_block_scalar_text(&body);
        return Some(format!("=VAL {indicator}{escaped}"));
    }
    // Bare top-level block scalar (no `---` marker) — e.g. a doc that begins
    // with `>\n …` or `|\n …`. Reuse the same folder; the only difference vs
    // the directive-end-packed form is the absence of a `YAML_DOCUMENT_START`
    // sentinel separating the header from the body.
    if let Some((indicator, body)) = extract_top_level_block_body(doc) {
        let escaped = escape_block_scalar_text(&body);
        return Some(format!("=VAL {indicator}{escaped}"));
    }
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
    let multi_line_text = collect_doc_scalar_text_with_newlines(doc);
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
            // quoted_val_event returns `=VAL "body` — splice the tag in.
            quoted.replacen("=VAL ", &format!("=VAL {long} "), 1)
        } else {
            format!("=VAL {long} :{trimmed_text}")
        }
    } else if is_multi_line_quoted {
        quoted_val_event_multi_line(&multi_line_text)
    } else if trimmed_text.starts_with('"') || trimmed_text.starts_with('\'') {
        quoted_val_event(&text)
    } else {
        let folded = fold_plain_document_lines(doc);
        // Plain top-level scalars may carry node properties (`&anchor`,
        // `!tag`) before the actual scalar body; decompose so events project
        // them in canonical `&anchor <tag> :body` order.
        let (anchor, body_tag, body) = decompose_scalar(folded.trim_start(), handles);
        if anchor.is_some() || body_tag.is_some() {
            scalar_event(anchor, body_tag.as_deref(), &escape_block_scalar_text(body))
        } else {
            format!("=VAL :{}", escape_block_scalar_text(&folded))
        }
    };
    Some(event)
}

/// Reconstruct the doc's scalar text with line breaks intact: walk
/// `YAML_SCALAR` + `NEWLINE` tokens in order (skipping directive lines).
/// Required for multi-line quoted folding because `YAML_SCALAR`-only joins
/// throw away the line structure that drives YAML 1.2 §7.3.2/§7.3.3 folding.
fn collect_doc_scalar_text_with_newlines(doc: &SyntaxNode) -> String {
    doc.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| matches!(tok.kind(), SyntaxKind::YAML_SCALAR | SyntaxKind::NEWLINE))
        .filter(|tok| !tok.text().trim_start().starts_with('%'))
        .map(|tok| tok.text().to_string())
        .collect()
}

fn plain_val_event(text: &str) -> String {
    format!("=VAL :{}", text.replace('\\', "\\\\"))
}

/// Fold the YAML-1.2 plain-scalar body of a top-level scalar `YAML_DOCUMENT`
/// into its canonical value: walk `YAML_SCALAR` and `NEWLINE` tokens in order
/// (skipping directive lines), then apply plain-scalar folding —
/// non-empty-line breaks fold to a single space, runs of `n` empty lines fold
/// to `n` line feeds. Leading/trailing empty lines are stripped.
fn fold_plain_document_lines(doc: &SyntaxNode) -> String {
    let raw: String = doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| matches!(tok.kind(), SyntaxKind::YAML_SCALAR | SyntaxKind::NEWLINE))
        .filter(|tok| !tok.text().trim_start().starts_with('%'))
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
        let inner = decode_single_quoted(text);
        format!("=VAL '{}", escape_for_event(&inner))
    } else {
        let inner = decode_double_quoted(text);
        format!("=VAL \"{}", escape_for_event(&inner))
    }
}

/// Multi-line quoted scalar projection: applies YAML 1.2 §7.3.2 / §7.3.3 line
/// folding (single line break → space, blank-line run of `n` blanks → `n`
/// `\n`s) before escape decoding. Required when a top-level quoted document
/// spans more than one source line — the single-line `quoted_val_event`
/// concatenates `YAML_SCALAR` tokens directly and would lose all line
/// structure.
fn quoted_val_event_multi_line(raw: &str) -> String {
    let trimmed = raw.trim_start_matches([' ', '\t', '\n']);
    if trimmed.starts_with('\'') {
        let inner_with_breaks = strip_quoted_wrapper(trimmed, '\'');
        let folded = fold_quoted_inner(&inner_with_breaks);
        let decoded = folded.replace("''", "'");
        format!("=VAL '{}", escape_for_event(&decoded))
    } else {
        let inner_with_breaks = strip_quoted_wrapper(trimmed, '"');
        let folded = fold_quoted_inner(&inner_with_breaks);
        let decoded = decode_double_quoted_inner(&folded);
        format!("=VAL \"{}", escape_for_event(&decoded))
    }
}

/// Strip the surrounding quote characters from a multi-line quoted scalar's
/// raw source. Walks until the first un-escaped (for `"`) or non-doubled
/// (for `'`) closing quote so embedded `\"` / `''` don't terminate early.
fn strip_quoted_wrapper(text: &str, quote: char) -> String {
    let body = text.strip_prefix(quote).unwrap_or(text);
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(ch) = chars.next() {
        if quote == '"' {
            if ch == '\\' {
                out.push(ch);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
                continue;
            }
            if ch == '"' {
                break;
            }
        } else if ch == '\'' {
            if chars.peek() == Some(&'\'') {
                out.push('\'');
                out.push('\'');
                chars.next();
                continue;
            }
            break;
        }
        out.push(ch);
    }
    out
}

/// Fold the inner body of a multi-line quoted scalar per YAML §7.3:
/// - On the first line, leading whitespace is preserved as-is.
/// - On continuation lines, leading whitespace is stripped.
/// - Trailing whitespace from the running output is dropped before folding.
/// - A run of `n` consecutive empty lines folds to `n` `\n` chars.
/// - A single line break (no blank between) folds to a single space.
/// - Trailing whitespace of the final line is stripped (matching
///   yaml-test-suite event expectations for multi-line quoted scalars).
fn fold_quoted_inner(inner: &str) -> String {
    let mut out = String::new();
    let mut blanks = 0usize;
    let mut have_first = false;
    for (idx, line) in inner.split('\n').enumerate() {
        if idx == 0 {
            out.push_str(line);
            have_first = true;
            continue;
        }
        let stripped = line.trim_start_matches([' ', '\t']);
        if stripped.is_empty() {
            blanks += 1;
            continue;
        }
        let trimmed_end = out.trim_end_matches([' ', '\t']);
        out.truncate(trimmed_end.len());
        if !have_first {
            // No content yet, so prepend nothing — first-line leading
            // whitespace is preserved later by the `idx == 0` branch only.
        } else if blanks == 0 {
            out.push(' ');
        } else {
            for _ in 0..blanks {
                out.push('\n');
            }
        }
        out.push_str(stripped);
        blanks = 0;
        have_first = true;
    }
    let trimmed_tail = out.trim_end_matches([' ', '\t']);
    out.truncate(trimmed_tail.len());
    out
}

/// Inner-only variant of [`decode_double_quoted`]: the input has no
/// surrounding quote characters and is consumed in full. Shares escape
/// decoding semantics with the wrapped form.
fn decode_double_quoted_inner(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            out.push('\\');
            break;
        };
        match next {
            '0' => out.push('\0'),
            'a' => out.push('\u{07}'),
            'b' => out.push('\u{08}'),
            't' | '\t' => out.push('\t'),
            'n' => out.push('\n'),
            'v' => out.push('\u{0B}'),
            'f' => out.push('\u{0C}'),
            'r' => out.push('\r'),
            'e' => out.push('\u{1B}'),
            ' ' => out.push(' '),
            '"' => out.push('"'),
            '/' => out.push('/'),
            '\\' => out.push('\\'),
            'N' => out.push('\u{85}'),
            '_' => out.push('\u{A0}'),
            'L' => out.push('\u{2028}'),
            'P' => out.push('\u{2029}'),
            'x' => {
                if let Some(c) = take_hex_char(&mut chars, 2) {
                    out.push(c);
                }
            }
            'u' => {
                if let Some(c) = take_hex_char(&mut chars, 4) {
                    out.push(c);
                }
            }
            'U' => {
                if let Some(c) = take_hex_char(&mut chars, 8) {
                    out.push(c);
                }
            }
            other => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

fn decode_single_quoted(text: &str) -> String {
    let body = text.strip_prefix('\'').unwrap_or(text);
    let body = body.strip_suffix('\'').unwrap_or(body);
    body.replace("''", "'")
}

/// Decode YAML double-quoted scalar escape sequences into actual characters
/// per YAML 1.2 §5.7. Unknown escapes are kept verbatim so the harness can
/// surface them as bare backslash-prefixed text.
fn decode_double_quoted(text: &str) -> String {
    let body = text.strip_prefix('"').unwrap_or(text);
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            break;
        }
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            out.push('\\');
            break;
        };
        match next {
            '0' => out.push('\0'),
            'a' => out.push('\u{07}'),
            'b' => out.push('\u{08}'),
            't' | '\t' => out.push('\t'),
            'n' => out.push('\n'),
            'v' => out.push('\u{0B}'),
            'f' => out.push('\u{0C}'),
            'r' => out.push('\r'),
            'e' => out.push('\u{1B}'),
            ' ' => out.push(' '),
            '"' => out.push('"'),
            '/' => out.push('/'),
            '\\' => out.push('\\'),
            'N' => out.push('\u{85}'),
            '_' => out.push('\u{A0}'),
            'L' => out.push('\u{2028}'),
            'P' => out.push('\u{2029}'),
            'x' => {
                if let Some(c) = take_hex_char(&mut chars, 2) {
                    out.push(c);
                }
            }
            'u' => {
                if let Some(c) = take_hex_char(&mut chars, 4) {
                    out.push(c);
                }
            }
            'U' => {
                if let Some(c) = take_hex_char(&mut chars, 8) {
                    out.push(c);
                }
            }
            other => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

fn take_hex_char(chars: &mut std::str::Chars<'_>, n: usize) -> Option<char> {
    let hex: String = chars.take(n).collect();
    if hex.len() != n {
        return None;
    }
    u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
}

/// Escape decoded scalar text for the yaml-test-suite event format, where
/// control characters and structural backslashes are rendered as backslash
/// escapes (`\n`, `\t`, `\b`, ...).
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
/// return the folded scalar body. Headers with explicit chomping (`-` strip,
/// `+` keep) or indent indicators are recognized; chomping is applied to the
/// final body. Default chomping is "clip" (single trailing newline).
fn extract_block_scalar_body(value_node: &SyntaxNode) -> Option<(char, String)> {
    let tokens: Vec<_> = value_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR
                    | SyntaxKind::NEWLINE
                    | SyntaxKind::WHITESPACE
                    | SyntaxKind::YAML_COMMENT,
            )
        })
        .collect();
    fold_block_scalar_tokens(&tokens)
}

/// Variant of [`extract_block_scalar_body`] that walks a full `YAML_DOCUMENT`
/// node and applies block-scalar folding to the tokens *after* a
/// `YAML_DOCUMENT_START` marker. Used for the directive-end-with-payload
/// pattern (`--- |\n  ab\n  cd\n`) where the block-scalar header is packed
/// onto the marker line itself rather than being a block-map value.
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
            SyntaxKind::YAML_SCALAR
            | SyntaxKind::NEWLINE
            | SyntaxKind::WHITESPACE
            | SyntaxKind::YAML_COMMENT => tokens.push(tok),
            _ => {}
        }
    }
    fold_block_scalar_tokens(&tokens)
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
                SyntaxKind::YAML_SCALAR
                    | SyntaxKind::NEWLINE
                    | SyntaxKind::WHITESPACE
                    | SyntaxKind::YAML_COMMENT,
            )
        })
        .collect();
    let first = tokens.iter().find(|tok| {
        tok.kind() == SyntaxKind::YAML_SCALAR && parse_block_scalar_indicator(tok.text()).is_some()
    })?;
    let _ = first;
    fold_block_scalar_tokens(&tokens)
}

fn fold_block_scalar_tokens(tokens: &[SyntaxToken]) -> Option<(char, String)> {
    let header_idx = tokens.iter().position(|t| {
        t.kind() == SyntaxKind::YAML_SCALAR && parse_block_scalar_indicator(t.text()).is_some()
    })?;
    let (indicator, chomp) = parse_block_scalar_indicator(tokens[header_idx].text())?;

    // Reconstruct the body source by stitching every token AFTER the header
    // and its trailing newline. Including `WHITESPACE` and `YAML_COMMENT`
    // tokens preserves the indentation needed for content-indent calculation
    // and lets a `# ...` line at column 0 (DK3J) land inside the body, while
    // a less-indented `# Comment` after a fully-indented body region (7T8X)
    // gets recognized as a body terminator.
    let mut raw = String::new();
    let mut skipped_header_newline = false;
    for tok in &tokens[header_idx + 1..] {
        if !skipped_header_newline && tok.kind() == SyntaxKind::NEWLINE {
            skipped_header_newline = true;
            continue;
        }
        raw.push_str(tok.text());
    }

    let raw_trailing_newlines = raw.chars().rev().take_while(|c| *c == '\n').count();

    let lines: Vec<&str> = raw.split('\n').collect();

    // Per YAML 1.2 §8.1.1.1, the content indentation level is set by the
    // first non-empty line of the contents.
    let content_indent = lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.chars().take_while(|c| *c == ' ').count())
        .unwrap_or(0);

    // Truncate at the first non-empty line whose indentation drops below the
    // content indent — that's where the block scalar's body ends per spec.
    // Trailing blanks (and the final empty split-tail) are kept; chomping
    // re-derives the right number of trailing newlines below.
    let mut body_lines: Vec<&str> = Vec::new();
    let mut seen_content = false;
    for line in lines.iter() {
        let is_blank = line.trim().is_empty();
        let indent = line.chars().take_while(|c| *c == ' ').count();
        if !is_blank && seen_content && indent < content_indent {
            break;
        }
        body_lines.push(line);
        if !is_blank {
            seen_content = true;
        }
    }
    if body_lines.last().is_some_and(|s| s.is_empty()) {
        body_lines.pop();
    }

    let stripped: Vec<BlockBodyLine> = body_lines
        .iter()
        .map(|l| {
            let is_blank = l.trim().is_empty();
            let indent = l.chars().take_while(|c| *c == ' ').count();
            // Always strip up to `content_indent` columns; for `|` style this
            // preserves trailing spaces past the content indent (T26H).
            let text = if l.len() >= content_indent {
                l[content_indent..].to_string()
            } else {
                String::new()
            };
            // More-indented lines (per §8.1.3) keep literal line breaks in
            // folded scalars. Blank lines are not flagged MI here; the folder
            // counts them and applies the surrounding-line rule.
            let is_mi = !is_blank && indent > content_indent;
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
            format!("{trimmed}{}", "\n".repeat(raw_trailing_newlines))
        }
    };
    Some((indicator, body))
}

struct BlockBodyLine {
    text: String,
    is_blank: bool,
    is_mi: bool,
}

/// Apply the YAML 1.2 §8.1.3 folded-scalar rules to a sequence of
/// content-indent-stripped body lines:
/// - Each leading blank line contributes a single `\n` to the output.
/// - Between two adjacent non-MI content lines, a single line break folds to
///   ` `; a run of `n` blank lines folds to `n` `\n` chars.
/// - When either side of the boundary is more-indented, *all* line breaks
///   between the two content lines are preserved literally.
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

fn parse_block_scalar_indicator(text: &str) -> Option<(char, BlockScalarChomp)> {
    let mut chars = text.chars();
    let indicator = match chars.next()? {
        '|' => '|',
        '>' => '>',
        _ => return None,
    };
    let mut chomp = BlockScalarChomp::Clip;
    let mut seen_chomp = false;
    let mut seen_indent = false;
    for ch in chars {
        match ch {
            '+' if !seen_chomp => {
                chomp = BlockScalarChomp::Keep;
                seen_chomp = true;
            }
            '-' if !seen_chomp => {
                chomp = BlockScalarChomp::Strip;
                seen_chomp = true;
            }
            '1'..='9' if !seen_indent => seen_indent = true,
            _ => return None,
        }
    }
    Some((indicator, chomp))
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

/// Locate a key/colon split in a block-context scalar. Honors a leading
/// quoted body (`"key": value`, `'key': value`) and percent-encoded URIs by
/// only treating `:` as a key indicator when followed by whitespace, a flow
/// indicator, or end-of-input. Per YAML 1.2 §7.4.3.1, embedded `"` / `'`
/// inside plain scalars are literal, so no quote-toggling occurs after the
/// leading-quote phase.
fn find_block_scalar_kv_split(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let lead = bytes
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t'))
        .unwrap_or(bytes.len());
    let mut idx = lead;
    match bytes.get(idx) {
        Some(b'"') => {
            idx += 1;
            let mut escaped = false;
            while idx < bytes.len() {
                let b = bytes[idx];
                idx += 1;
                if escaped {
                    escaped = false;
                    continue;
                }
                if b == b'\\' {
                    escaped = true;
                    continue;
                }
                if b == b'"' {
                    break;
                }
            }
        }
        Some(b'\'') => {
            idx += 1;
            while idx < bytes.len() {
                let b = bytes[idx];
                idx += 1;
                if b == b'\'' {
                    if bytes.get(idx) == Some(&b'\'') {
                        idx += 1;
                        continue;
                    }
                    break;
                }
            }
        }
        _ => {}
    }
    while idx < bytes.len() {
        if bytes[idx] == b':' {
            let after = idx + 1;
            let next = bytes.get(after);
            // In block context (which is where this helper runs) only
            // whitespace or end-of-input qualifies as the key/value
            // indicator's trailing context. The flow-collection terminators
            // (`,`, `}`, `]`) are literal here — `- :,` is a single scalar
            // `:,`, not an empty-key map.
            let is_separator = matches!(next, None | Some(b' ' | b'\t' | b'\n' | b'\r'));
            if is_separator {
                return Some(idx);
            }
        }
        idx += 1;
    }
    None
}

/// Project a single scalar (without surrounding `+MAP`/`-MAP`) for an inline
/// map key or value position. Anchors/tags are decomposed in canonical order;
/// alias references (`*name`) emit `=ALI`. An empty body emits `=VAL :`.
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
            out.push("+SEQ".to_string());
            project_block_sequence_items(&nested_seq, handles, out);
            out.push("-SEQ".to_string());
            continue;
        }
        // Inline-map sequence item: `- key: value` (with optional continuation
        // lines that the parser captures as a nested YAML_BLOCK_MAP). The
        // direct YAML_SCALAR/YAML_TAG/whitespace token chain encodes the first
        // entry; subsequent entries live in the nested map node. Including
        // YAML_TAG keeps tagged empty keys/values (`- !!str : !!null`) intact
        // so `decompose_scalar` can recover the tag.
        let direct_scalar: String = item
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| {
                matches!(
                    tok.kind(),
                    SyntaxKind::YAML_SCALAR
                        | SyntaxKind::YAML_TAG
                        | SyntaxKind::YAML_KEY
                        | SyntaxKind::YAML_COLON
                        | SyntaxKind::WHITESPACE,
                )
            })
            .map(|tok| tok.text().to_string())
            .collect();
        if let Some(colon_idx) = find_block_scalar_kv_split(&direct_scalar) {
            let nested_map = item
                .children()
                .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP);
            out.push("+MAP".to_string());
            project_inline_scalar(&direct_scalar[..colon_idx], handles, out);
            project_inline_scalar(&direct_scalar[colon_idx + 1..], handles, out);
            if let Some(nm) = nested_map {
                project_block_map_entries(&nm, handles, out);
            }
            out.push("-MAP".to_string());
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
    for child in map_node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(tok)
                if tok.kind() == SyntaxKind::YAML_SCALAR
                    && tok.text().trim_start().starts_with("? ") =>
            {
                let body = tok.text().trim_start().trim_start_matches("? ").trim();
                if body.is_empty() {
                    out.push("=VAL :".to_string());
                } else {
                    let (anchor, body_tag, rest) = decompose_scalar(body, handles);
                    out.push(scalar_event(anchor, body_tag.as_deref(), rest));
                }
                out.push("=VAL :".to_string());
            }
            rowan::NodeOrToken::Node(entry) if entry.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY => {
                project_block_map_entry(&entry, handles, out);
            }
            _ => {}
        }
    }
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

    let key_trimmed = key_text.trim();
    if key_trimmed.starts_with('[')
        && key_trimmed.ends_with(']')
        && let Some(items) = simple_flow_sequence_items(key_trimmed)
    {
        out.push("+SEQ []".to_string());
        for item in items {
            project_flow_seq_item(&item, handles, out);
        }
        out.push("-SEQ".to_string());
    } else if key_trimmed.starts_with('*') {
        out.push(format!("=ALI {key_trimmed}"));
    } else {
        let key_long_tag = key_tag
            .as_deref()
            .and_then(|t| resolve_long_tag(t, handles));
        let (anchor, body_tag, body) = decompose_scalar(key_trimmed, handles);
        let long_tag = key_long_tag.or(body_tag);
        out.push(scalar_event(anchor, long_tag.as_deref(), body));
    }

    if let Some(nested_map) = value_node
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
    {
        out.push("+MAP".to_string());
        project_block_map_entries(&nested_map, handles, out);
        out.push("-MAP".to_string());
        return;
    }

    if let Some(flow_map) = value_node
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
    {
        out.push("+MAP {}".to_string());
        project_flow_map_entries(&flow_map, handles, out);
        out.push("-MAP".to_string());
        return;
    }

    if let Some((indicator, body)) = extract_block_scalar_body(&value_node) {
        let escaped = escape_block_scalar_text(&body);
        out.push(format!("=VAL {indicator}{escaped}"));
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
