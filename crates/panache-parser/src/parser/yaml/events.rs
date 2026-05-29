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
    // Verbatim tag `!<URI>` (YAML 1.2 §6.8.1): the URI between the angle
    // brackets is used as-is, bypassing handle resolution. Local verbatim
    // tags keep their leading `!` (`!<!bar>` → `<!bar>`). Checked before the
    // handle loop so a registered `!` primary handle can't claim it.
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

/// Walk the shadow CST for `input` and return the projected yaml-test-suite
/// event stream. Returns an empty vector if the input fails to parse.
pub fn project_events(input: &str) -> Vec<String> {
    let Some(tree) = parse_yaml_tree(input) else {
        return Vec::new();
    };
    project_events_from_tree(&tree)
}

/// Walk a shadow-parser CST and return the projected yaml-test-suite event
/// stream. Decoupled from `parse_yaml_tree` so the v2 parser can reuse the
/// same projection for parity comparisons.
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

/// True when the document holds no content beyond a `DocumentEnd`
/// marker and surrounding trivia (whitespace, newlines, comments).
/// Used to distinguish a real (possibly empty) document from a
/// synthetic doc the v2 builder wrapped around a bare `...`.
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

/// LX3P: a `[flow]` sequence written as a block-map key lands in the v2 CST
/// as a YAML_FLOW_SEQUENCE that's a direct child of the YAML_DOCUMENT,
/// preceding the YAML_BLOCK_MAP that the trailing `:` opens. Returns that
/// flow-sequence when this shape is present.
fn flow_seq_preceding_block_map_at_doc_level(
    doc: &SyntaxNode,
    block_map: &SyntaxNode,
) -> Option<SyntaxNode> {
    let block_map_offset = block_map.text_range().start();
    doc.children()
        .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
        .find(|n| n.text_range().end() <= block_map_offset)
}

/// True when a YAML_BLOCK_MAP_ENTRY's KEY wrapper carries no key text —
/// only structural trivia and the `:` indicator. Used to detect the
/// implicit-empty-key shape (`: value`) and the LX3P pattern where the
/// real key lives in a sibling node preceding the map.
fn block_map_entry_key_is_empty(entry: &SyntaxNode) -> bool {
    let Some(key_node) = entry
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
    else {
        return false;
    };
    !key_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .take_while(|tok| tok.kind() != SyntaxKind::YAML_COLON)
        .any(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_KEY | SyntaxKind::YAML_SCALAR | SyntaxKind::YAML_TAG
            ) && !tok.text().trim().is_empty()
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
    // A v2 builder synthesizes a `YAML_DOCUMENT` around a bare `...`
    // (or comments preceding it) to keep the marker inside a document
    // for losslessness. v1 / yaml-test-suite considers such input an
    // empty stream — no `+DOC`/`-DOC` events. Skip the projection when
    // the only structural content is a `DocumentEnd` marker (HWV9,
    // QT73).
    if !has_doc_start && doc_is_marker_only(doc) {
        return;
    }
    out.push(if has_doc_start {
        "+DOC ---".to_string()
    } else {
        "+DOC".to_string()
    });
    let handles = collect_tag_handles(doc);

    // Top-level container detection must look at direct children, not
    // arbitrary descendants. A `descendants()` walk surfaces the first
    // BLOCK_SEQUENCE/BLOCK_MAP it finds in document order — which for a
    // block-map whose values contain nested block-sequences would be the
    // inner sequence, collapsing the entire map into a bare `+SEQ`.
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
        // Flow-sequence used as a block-map key (LX3P: `[flow]: block`).
        // v2 lands the `[flow]` flow-sequence as a sibling preceding the
        // YAML_BLOCK_MAP (the colon opens an empty-key entry inside the
        // map), but yaml-test-suite expects `+MAP +SEQ []…-SEQ value -MAP`.
        // Splice the flow-seq in as the first entry's key when this shape
        // is present.
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
        }
    } else if let Some(flow_collection) = doc.children().find(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_FLOW_MAP | SyntaxKind::YAML_FLOW_SEQUENCE
        )
    }) {
        // A doc-direct flow collection may be preceded by a doc-level
        // anchor token (`&flowseq [ ... ]`, CN3R). Carry the anchor
        // onto the open event so `+SEQ [] &flowseq` matches the
        // expected projection. Looking at `descendants()` (the prior
        // implementation) is wrong here because it surfaces the
        // first nested flow_map encountered in document order — for a
        // `&flowseq [ ... { e: f } ... ]` shape that collapses the
        // whole document into a bare flow-map projection.
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
    // Include WHITESPACE between tokens so a top-level `&anchor body`
    // joins as `&anchor body`, letting `decompose_scalar` find the
    // whitespace terminator on the anchor name.
    let text = doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR
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
            // Plain scalar: fold multi-line continuations the same way the
            // untagged path does so `!!str\nd\ne` projects as `:d e`. The
            // folded text may still carry a leading anchor token
            // (`&a1\nscalar1`, 9KAX) since `fold_plain_document_lines`
            // keeps YAML_ANCHOR tokens — peel it off so the event renders
            // as `=VAL &anchor <tag> :body` rather than burying `&anchor`
            // in the scalar body.
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
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::NEWLINE
            )
        })
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
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::WHITESPACE
                    | SyntaxKind::NEWLINE
            )
        })
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
        if trimmed.contains('\n') {
            return quoted_val_event_multi_line(trimmed);
        }
        return quoted_val_event(trimmed);
    }
    // Alias indicator (`*name`). YAML plain scalars cannot begin with `*`,
    // so a leading `*` is always an alias reference. The trimmed body
    // (`*name`) is the alias's serialized form.
    if trimmed.starts_with('*') {
        return format!("=ALI {trimmed}");
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
    // Verbatim tag `!<URI>`: the URI runs to the closing `>` and may contain
    // characters (`,`, `:`) that otherwise terminate a shorthand. Span the
    // whole `!<…>` so the URI isn't truncated at the first comma/colon.
    if let Some(uri) = rest.strip_prefix('<') {
        let close = uri.find('>')?;
        // `!` + `<` + URI + `>`
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
                // YAML 1.2 §7.4.2: a JSON-like key (here, a quoted scalar)
                // permits an adjacent value colon with no following space
                // (`"JSON like":adjacent`, 9MMW). Flow-collection keys are
                // projected structurally before reaching this text path.
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
        let trimmed = item.trim();
        // Multi-line quoted scalar inside a flow sequence: apply YAML
        // 1.2 §7.3 line-folding rules so embedded newlines fold to a
        // space (or `\n` for blank-line runs) before the event's escape
        // pass. Without this, joining tokens directly leaves the literal
        // newline inside the body.
        if trimmed.contains('\n') {
            out.push(quoted_val_event_multi_line(trimmed));
        } else {
            out.push(quoted_val_event(trimmed));
        }
    } else {
        // Route through `flow_scalar_event` so node properties on a
        // flow-seq item (`[&item a, b, c]`, 6BFJ) project as
        // `=VAL &item :a` and alias items (`[*b]`, X38W) project as
        // `=ALI *b`.
        out.push(flow_scalar_event(&fold_plain_scalar(item), handles));
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
        let folded = fold_quoted_inner(&inner_with_breaks, false);
        let decoded = folded.replace("''", "'");
        format!("=VAL '{}", escape_for_event(&decoded))
    } else {
        let inner_with_breaks = strip_quoted_wrapper(trimmed, '"');
        let folded = fold_quoted_inner(&inner_with_breaks, true);
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
///
/// `escaped_breaks` enables YAML §7.5 double-quoted escaped line breaks: a
/// continuation line whose predecessor ends in an unescaped (odd-count)
/// backslash joins directly with no folded space, and the escaping backslash
/// is dropped. Pass `false` for single-quoted and plain scalars, where a
/// trailing backslash is literal content.
fn fold_quoted_inner(inner: &str, escaped_breaks: bool) -> String {
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
        trim_trailing_ws_respecting_escape(&mut out, escaped_breaks);
        if escaped_breaks && blanks == 0 && have_first && ends_with_odd_backslashes(&out) {
            // The preceding line ends in an unescaped backslash: the line
            // break is escaped, so the continuation joins with no folded
            // space and the escaping backslash is consumed.
            out.pop();
            out.push_str(stripped);
            blanks = 0;
            continue;
        }
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
    if blanks > 0 {
        // A trailing run of blank/whitespace-only lines ends the scalar. The
        // accumulated content is followed by a fold, so strip its trailing
        // whitespace, then append the folded breaks: a single break collapses
        // to a space, a run of `n` breaks collapses to `n - 1` newlines. When
        // every line is empty/whitespace-only the content is empty and this is
        // the scalar's only contribution (yaml-test-suite NAT4).
        trim_trailing_ws_respecting_escape(&mut out, escaped_breaks);
        if blanks == 1 {
            out.push(' ');
        } else {
            for _ in 0..blanks - 1 {
                out.push('\n');
            }
        }
    }
    // No trailing blank run: the final line's trailing whitespace before the
    // closing quote is content (yaml-test-suite 7A4E) and is preserved as-is.
    out
}

/// Strip trailing space/tab chars from a double-quoted folding buffer,
/// preserving the first whitespace char of a `\<ws>` escape sequence.
///
/// YAML 1.2 §5.7 includes escapes `\<TAB>` (literal tab) and `\<SPACE>`
/// (literal space) — the whitespace after the backslash is the escape's
/// argument and must survive the trailing-whitespace strip that fold rules
/// apply on continuation. Without this, inputs like `"x\<TAB> \n y"`
/// (DE56/02) lose the tab and the trailing `\` is mis-detected as a
/// line-continuation marker, collapsing the value to `xy`.
///
/// For single-quoted / plain scalars (`escaped_breaks == false`), `\` is
/// literal content and the function degrades to a plain whitespace strip.
fn trim_trailing_ws_respecting_escape(out: &mut String, escaped_breaks: bool) {
    let bytes = out.as_bytes();
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b' ' || bytes[end - 1] == b'\t') {
        end -= 1;
    }
    if !escaped_breaks || end == bytes.len() || end == 0 || bytes[end - 1] != b'\\' {
        out.truncate(end);
        return;
    }
    let mut bs_start = end - 1;
    while bs_start > 0 && bytes[bs_start - 1] == b'\\' {
        bs_start -= 1;
    }
    let bs_count = end - bs_start;
    if bs_count % 2 == 1 {
        // Unescaped `\` — the next byte (a space or tab) is the escape's
        // argument; keep it and trim anything past it.
        out.truncate(end + 1);
    } else {
        out.truncate(end);
    }
}

/// Whether `s` ends with an odd-length run of `\` characters, i.e. the final
/// backslash is unescaped. Used to detect double-quoted escaped line breaks.
fn ends_with_odd_backslashes(s: &str) -> bool {
    s.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1
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
    fold_block_scalar_tokens(&tokens, block_scalar_parent_indent(value_node))
}

/// Detect a block-map value shaped `<node-properties> <block-scalar>`, where a
/// leading anchor and/or tag precede a literal (`|`) or folded (`>`) block
/// scalar (e.g. `folded: !foo >1\n value`, M5C3/Z67P). The scanner embeds the
/// node properties *inside* the leading `YAML_SCALAR` token rather than a
/// separate `YAML_TAG`, so [`extract_block_scalar_body`] sees a header line of
/// `!foo >1` that doesn't parse as an indicator and bails. Here we reconstruct
/// the value text with newlines, peel the leading `&anchor` / tag (either
/// order), and fold the remainder as a block scalar. Returns
/// `(anchor, long_tag, indicator, folded_body)` when the shape matches.
fn extract_tagged_block_scalar(
    value_node: &SyntaxNode,
    handles: &TagHandles,
) -> Option<(Option<String>, Option<String>, char, String)> {
    let full = collect_value_scalar_text_with_newlines(value_node);
    let mut rest = full.trim_start();

    // Peel a leading anchor and/or tag in either order. Bail unless at least
    // one property is present — a bare block scalar is handled upstream by
    // `extract_block_scalar_body`, and a plain scalar must not be re-read here.
    let mut anchor: Option<String> = None;
    let mut long_tag: Option<String> = None;
    loop {
        if anchor.is_none()
            && let Some(after) = rest.strip_prefix('&')
        {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
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
    if anchor.is_none() && long_tag.is_none() {
        return None;
    }

    // The block-scalar source begins at the indicator; the header line runs to
    // the first newline, the body follows it.
    let (header_part, body_raw) = match rest.split_once('\n') {
        Some((header, body)) => (header, body),
        None => (rest, ""),
    };
    let (indicator, body) = fold_block_scalar_raw(
        header_part,
        body_raw,
        block_scalar_parent_indent(value_node),
    )?;
    Some((anchor, long_tag, indicator, body))
}

/// Compute the column of the start-of-line for the parent scope of a
/// block-scalar value, used to anchor explicit indent indicators per
/// YAML 1.2 §8.1.1.1: when a block-scalar header carries an indentation
/// indicator `m`, the absolute content indent is `parent_indent + m`.
///
/// Walks up to the YAML_BLOCK_MAP_ENTRY (for map values) or treats a
/// passed YAML_BLOCK_SEQUENCE_ITEM as its own parent. Other shapes
/// (e.g. top-level YAML_DOCUMENT) fall back to the node's own column,
/// which is 0 at the document level.
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
                SyntaxKind::YAML_SCALAR
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
        if tok.kind() != SyntaxKind::YAML_SCALAR {
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
        if t.kind() != SyntaxKind::YAML_SCALAR {
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

/// Fold a block scalar from its header line (`|`, `>2-`, …) and the raw body
/// source that follows the header's trailing newline. Shared by the
/// token-walking [`fold_block_scalar_tokens`] and the tagged-value path
/// ([`extract_tagged_block_scalar`]), which strips a leading anchor/tag off the
/// embedded scalar token before reaching the indicator. `parent_indent`
/// anchors explicit indent indicators per YAML 1.2 §8.1.1.1.
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
                SyntaxKind::YAML_SCALAR => {
                    let text = tok.text();
                    match text {
                        "{" | "}" => {}
                        "," => {
                            if pending_has_content {
                                flush_pending_orphan(&pending, handles, out);
                                pending.clear();
                                pending_has_content = false;
                            }
                        }
                        _ => {
                            pending.push_str(text);
                            pending_has_content = true;
                        }
                    }
                }
                SyntaxKind::YAML_KEY => {
                    pending.push_str(tok.text());
                    pending_has_content = true;
                }
                _ => {}
            },
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

/// Flush an orphan scalar that wasn't followed by a matching
/// empty-key entry. YAML 1.2 treats this as an implicit-value entry
/// (`{a, b: c}` ≡ `{a: ~, b: c}`), so the projection emits the key
/// then an empty value.
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
        let folded = fold_plain_scalar(trimmed);
        let stripped = strip_explicit_key_indicator(&folded);
        if stripped.is_empty() {
            out.push("=VAL :".to_string());
        } else {
            // Resolve a leading anchor/tag/handle on the orphan key the
            // same way `flow_scalar_event` does for in-entry scalars.
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
        .any(|tok| matches!(tok.kind(), SyntaxKind::YAML_SCALAR | SyntaxKind::YAML_KEY));

    // A flow collection (`[...]` / `{...}`) nested directly inside the
    // KEY wrapper is a complex key (SBG9 `{[d, e]: f}`) and must project
    // structurally (`+SEQ [] ... -SEQ`) rather than as slurped scalar
    // text. With the scanner registering flow-collection-start as a
    // simple-key candidate, the resulting CST places the collection
    // node directly under `YAML_FLOW_MAP_KEY` instead of leaving it as
    // an orphan sibling.
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
        // Pick up an anchor sitting in the KEY wrapper before the
        // collection (`{ &a [a, &b b]: *b }`, X38W) so the structural
        // projection carries `&a` on the open event.
        let anchor = anchor_preceding_node(&key_node, &collection);
        project_flow_collection_node_with_anchor(&collection, anchor.as_deref(), handles, out);
        project_flow_map_value(&value_node, handles, out);
        return;
    }

    // Include WHITESPACE / NEWLINE so v2's separately-emitted `?`
    // (`YAML_KEY`) and key scalar (`YAML_SCALAR`) keep the original
    // trivia between them, letting `strip_explicit_key_indicator`
    // recognize the `?<sp>` pattern. v1 emitted both as a single
    // `YAML_KEY` token so the join was already a no-op there.
    // Include `YAML_ANCHOR`/`YAML_ALIAS` so node properties on a flow
    // map key (`{ &c c: d }`, CN3R) and an alias-as-key (`{ *a: v }`,
    // X38W) survive into the key text — `flow_scalar_event` then
    // peels the leading `&anchor` or projects the `*alias`.
    let mut raw_key = key_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR
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

    // External key prepends only when the entry's own key is empty
    // (the v2-scanner orphan-merge case): the orphan provides the key
    // bytes, the entry just contributes the `:` and the value.
    if let Some(ext) = external_key
        && !key_has_content
    {
        raw_key = format!("{ext}{raw_key}");
    } else if let Some(ext) = external_key {
        // Pending was non-empty but this entry already has a real
        // key — flush pending as a standalone implicit-value entry
        // first so neither side gets dropped.
        flush_pending_orphan(ext, handles, out);
    }

    if has_explicit_colon {
        // Strip the explicit-key `?` indicator (`{ ? foo : v }`) from
        // the projected key text. A bare `? :` entry (key reduces to
        // empty after stripping) projects to an empty `=VAL :`.
        let key_for_classify = raw_key.trim();
        let stripped_key = strip_explicit_key_indicator(key_for_classify);
        if stripped_key.is_empty() {
            // Tag-only key (`!!str : bar` in WZ62) — `raw_key` skips
            // YAML_TAG, so an entry whose key is only a tag arrives
            // here empty. Pick the YAML_TAG sibling off the KEY node.
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
            // Multi-line plain key text needs folding before
            // resolution; flow_scalar_event does it for plain text but
            // bypasses folding when the input contains explicit tag
            // bytes — handle the plain branch here so multi-line
            // orphans collapse to a single line.
            let folded = fold_plain_scalar(stripped_key);
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
                    SyntaxKind::YAML_SCALAR | SyntaxKind::YAML_ANCHOR | SyntaxKind::YAML_ALIAS
                )
            })
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

/// Project a `YAML_FLOW_MAP_VALUE` node, recursing into nested flow
/// collections (`+SEQ [] ... -SEQ`, `+MAP {} ... -MAP`) when present so that
/// multi-line nested flow values like `{ a: [ b, c, { d: [e, f] } ] }`
/// produce structured event streams instead of one slurped scalar.
fn project_flow_map_value(value_node: &SyntaxNode, handles: &TagHandles, out: &mut Vec<String>) {
    // A YAML_TAG sibling decorates the nested flow collection or scalar
    // that follows (EHF6 `k: !!seq [ a, !!str b]`).
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

    // Include `YAML_COLON` tokens alongside `YAML_SCALAR` so a
    // plain-scalar value that begins with `:` (e.g. 5T43's
    // `{ "key"::value }` and 58MP's `{x: :x}` — leading `:` after
    // the entry's key indicator) carries its colon into the event
    // body. The scanner emits the leading `:` as a stray Value token
    // that the v2 builder lands inside the VALUE wrapper; without
    // collecting `YAML_COLON` here the projection drops it and the
    // event becomes `=VAL :value` instead of `=VAL ::value`.
    let raw_value = value_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::YAML_COLON
            )
        })
        .map(|tok| tok.text().to_string())
        .collect::<Vec<_>>()
        .join("");
    if raw_value.trim().is_empty() {
        // Tag-only value (`!!str,` in WZ62) — no scalar content but a
        // YAML_TAG sibling annotates the empty value.
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

/// Emit the events for a flow collection node (`+SEQ [] ... -SEQ` or
/// `+MAP {} ... -MAP`). Shared by flow-map orphan-key projection and
/// flow-sequence single-pair-map projection so a collection sitting in
/// key position is projected structurally, not slurped as scalar text.
fn project_flow_collection_node(node: &SyntaxNode, handles: &TagHandles, out: &mut Vec<String>) {
    project_flow_collection_node_with_anchor(node, None, handles, out);
}

/// Variant of [`project_flow_collection_node`] that propagates a
/// caller-extracted anchor (e.g. `&a [a, &b b]`) into the collection's
/// open event (`+SEQ [] &a`, `+MAP {} &a`). The anchor name is passed
/// without its leading `&`. A `tag` (already resolved to the long form
/// `<tag:...>`) is appended after the anchor when the parent decorates
/// the flow collection (`--- !!map { ... }`, EHF6).
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

/// Walk `container`'s children-with-tokens from the start; return the
/// anchor name (sans `&`) of any `YAML_ANCHOR` token that sits before
/// `target` (and is not separated from it by a non-trivia token). Used
/// to splice a key/value anchor onto a structural projection of a
/// flow collection (`&a [...]`, `&a { ... }`).
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
                    SyntaxKind::YAML_SCALAR
                        | SyntaxKind::YAML_KEY
                        | SyntaxKind::WHITESPACE
                        | SyntaxKind::NEWLINE
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
            _ => {}
        }
    }
    project_inline_scalar(&value_text, handles, out);
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
        // A flow-sequence item shaped `<collection>: <value>` is an
        // implicit single-pair map keyed by the collection
        // (`[ [[b,c]]: d ]`, `[ {JSON: like}: adjacent ]`). Detect a
        // leading collection node followed by a direct-child colon and
        // wrap it in `+MAP {} ... -MAP`; scalar-keyed pairs keep the
        // proven text path (`flow_kv_split`) below.
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
            // Propagate an item-level anchor (`[ &g [...] ]`, CN3R-shape)
            // onto the nested collection's open event.
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
        // Build the item text from scalar/key/colon tokens plus
        // structural whitespace so an embedded `:` (e.g. an implicit
        // flow-map entry like `'k' : v` written inside `[...]`, see
        // 87E4 / L9U5 / LQZ7) survives into `flow_kv_split`. Skipping
        // colons collapsed the entry into a single `=VAL :scalar` and
        // hid the `+MAP {} ... -MAP` wrap; preserving them lets
        // `project_flow_seq_item` recognize the kv pattern.
        // `YAML_COMMENT` tokens stay excluded so leading/trailing
        // comments inside multi-line items don't leak into the value.
        // Include `YAML_ANCHOR`/`YAML_ALIAS` so node properties on a
        // plain item (`[&item a, b]`, 6BFJ) and bare aliases (`[*b]`,
        // X38W) survive into the item text — `flow_scalar_event`
        // (called from `project_flow_seq_item`) then peels them.
        let item_text: String = item
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| {
                matches!(
                    tok.kind(),
                    SyntaxKind::YAML_SCALAR
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
            // A YAML_TAG / YAML_ANCHOR sibling decorates the nested
            // sequence (`- !!seq\n - nested`, 57H4).
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
                        | SyntaxKind::YAML_ANCHOR
                        | SyntaxKind::YAML_ALIAS
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
            out.push(map_open_event_for_block_map(&nested_map, handles));
            project_block_map_entries(&nested_map, handles, out);
            out.push("-MAP".to_string());
            continue;
        }
        if let Some(flow_seq) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
        {
            // Walk the CST rather than re-splitting the flow text: only the
            // CST walker structurally projects items whose key is itself a
            // flow collection (`[ {JSON: like}:adjacent ]`, 9MMW) or a nested
            // flow sequence; the text splitter mis-folds those into scalars.
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
        // Include WHITESPACE so `&anchor body` joins as `&anchor body`,
        // letting `decompose_scalar` find the whitespace terminator on
        // the anchor name. See `project_block_map_entry_value` for the
        // matching rationale at value position.
        let scalar_text = item
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| {
                matches!(
                    tok.kind(),
                    SyntaxKind::YAML_SCALAR
                        | SyntaxKind::YAML_ANCHOR
                        | SyntaxKind::YAML_ALIAS
                        | SyntaxKind::WHITESPACE
                )
            })
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
            let folded;
            let body_for_event: &str = if body.contains('\n') {
                folded = fold_plain_scalar(body);
                &folded
            } else {
                body
            };
            scalar_event(anchor, long_tag.as_deref(), body_for_event)
        };
        out.push(event);
    }
}

/// Decompose a node-property + scalar string into `(anchor, long_tag, body)`,
/// peeling off any leading `&anchor` and tag shorthand in either order
/// (`&a !!str foo` or `!!str &a foo`). Returns the raw body trimmed.
/// Build the `+SEQ` open event for a YAML_BLOCK_SEQUENCE, attaching any
/// document-level node properties (a tag, or a `&anchor` carried by the
/// block-sequence header line) that precede the first sequence item. The
/// parser stores those properties as YAML_TAG / YAML_SCALAR siblings of
/// the YAML_BLOCK_SEQUENCE_ITEM children, in source order.
fn seq_open_event(seq_node: &SyntaxNode, handles: &TagHandles) -> String {
    let mut anchor: Option<String> = None;
    let mut long_tag: Option<String> = None;
    // v2 emits anchors/tags as siblings of the YAML_BLOCK_SEQUENCE within
    // the parent container (e.g. directly under a YAML_DOCUMENT for the
    // top-level `&anchor\n- a` shape) — not as inner-prefix tokens like
    // v1. Scan parent siblings preceding the SEQ first.
    absorb_preceding_anchor_and_tag(seq_node, handles, &mut anchor, &mut long_tag);
    // v1 emits anchors/tags as inner-prefix tokens of the SEQ before the
    // first BLOCK_SEQUENCE_ITEM. Also walk those for backward compat.
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

/// Walk the parent's children and absorb `YAML_TAG`/`YAML_SCALAR` tokens
/// (carrying a `&...` anchor or `!...` tag) that appear *before* the
/// `child` node, stopping at `child`. Used by `seq_open_event` /
/// `map_open_event_for_block_map` to capture v2's emission of leading
/// anchor/tag tokens at the parent level rather than inside the
/// container.
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

/// Inspect a single token for an anchor or tag and update the
/// respective slot. Recognizes both v1's and v2's emission shape:
/// - v1 emits anchors as `YAML_SCALAR` tokens whose text starts with `&`.
/// - v2 emits anchors as `YAML_TAG` tokens (the synthesis of anchor and
///   tag into a single SyntaxKind), distinguishable by the leading byte.
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
        SyntaxKind::YAML_SCALAR => {
            let trimmed = tok.text().trim();
            if anchor.is_none()
                && let Some(name) = trimmed.strip_prefix('&')
            {
                *anchor = Some(name.to_string());
            } else if long_tag.is_none()
                && trimmed.starts_with('!')
                && let Some(long) = resolve_long_tag(trimmed, handles)
            {
                *long_tag = Some(long);
            }
        }
        _ => {}
    }
}

/// Build the `+MAP` open event for a nested YAML_BLOCK_MAP that lives inside
/// a YAML_BLOCK_MAP_VALUE. Captures any anchor (`&name`) or tag (`!!str`,
/// `!shorthand`, etc.) tokens that precede the inner block map so that
/// projected events match patterns like `+MAP &node3` from yaml-test-suite
/// case 26DV (`top3: &node3` followed by an indented nested block map).
fn map_open_event_for_value(value_node: &SyntaxNode, handles: &TagHandles) -> String {
    let (anchor, long_tag, _residual) = extract_leading_node_properties(value_node, handles);
    map_open_event_from_props(anchor.as_deref(), long_tag.as_deref())
}

/// Render a `+MAP` open event from pre-extracted node properties, emitting them
/// in the canonical yaml-test-suite order: `&anchor` before `<tag>` (matching
/// [`scalar_event`] and `+MAP &a4 <tag:…>` fixtures).
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

/// Walk the leading children of a node that precedes a nested collection — a
/// YAML_BLOCK_MAP_VALUE (`key: &a !!map\n …`, BU8L) or a YAML_BLOCK_SEQUENCE_ITEM
/// (`- !!map\n …`, 6JWB) — stopping at any nested YAML_BLOCK_MAP / YAML_FLOW_MAP
/// / YAML_FLOW_SEQUENCE. Pulls out the optional anchor (`&name`, ending at
/// whitespace, comma, or flow-collection closer), the optional resolved tag,
/// and any residual scalar text that follows the node properties (e.g. the
/// `*alias1` in 26DV's `&node3 \n  *alias1` scalar, or the fused first key in
/// `&a !!map\n  a`). Both anchor and tag are peeled from the embedded scalar
/// text in either order, since the scanner fuses node properties and the first
/// key into one YAML_SCALAR token rather than emitting a separate YAML_TAG.
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
            SyntaxKind::YAML_SCALAR => {
                let mut rest = tok.text().trim();
                // Peel a leading `&anchor` and/or tag shorthand, in either
                // order, that haven't already been captured.
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
            }
            _ => {}
        }
    }
    (anchor, long_tag, residual)
}

/// Build the `+MAP` open event for a YAML_BLOCK_MAP rooted directly under
/// a YAML_DOCUMENT. Captures any anchor (`&name`) or tag (`!!str`,
/// `!shorthand`, etc.) tokens that the parser absorbed at the top of the
/// block map so that documents like `--- !!set\n? a\n? b` project as
/// `+MAP <tag:yaml.org,2002:set>`.
fn map_open_event_for_block_map(map_node: &SyntaxNode, handles: &TagHandles) -> String {
    let mut anchor: Option<String> = None;
    let mut long_tag: Option<String> = None;
    // Mirror `seq_open_event`: scan parent siblings preceding this MAP
    // first (v2 emission), then the MAP's inner-prefix tokens (v1).
    absorb_preceding_anchor_and_tag(map_node, handles, &mut anchor, &mut long_tag);
    for child in map_node.children_with_tokens() {
        if let Some(node) = child.as_node()
            && node.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY
        {
            break;
        }
        let Some(tok) = child.as_token() else {
            continue;
        };
        if tok.kind() == SyntaxKind::YAML_SCALAR {
            let trimmed = tok.text().trim();
            // A `? `-prefixed scalar is the first key of the map; stop
            // scanning header tokens at that point so we don't pick up
            // entry-level data as document-level node properties.
            if trimmed.starts_with("? ") || trimmed == "?" {
                break;
            }
        }
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
    // yaml-test-suite events escape `\`, control characters, and embedded
    // newlines in plain-scalar bodies. Apply that here so callers can pass
    // raw (or fold-only) text and not pre-escape.
    format!("=VAL {prefix}:{}", escape_for_event(body))
}

fn project_block_map_entries(map_node: &SyntaxNode, handles: &TagHandles, out: &mut Vec<String>) {
    let children: Vec<_> = map_node.children_with_tokens().collect();
    let mut idx = 0;
    while idx < children.len() {
        match &children[idx] {
            rowan::NodeOrToken::Token(tok)
                if tok.kind() == SyntaxKind::YAML_SCALAR
                    && (tok.text().trim_start().starts_with("? ")
                        || tok.text().trim_start() == "?") =>
            {
                let body = tok.text().trim_start().trim_start_matches('?').trim();
                if body.is_empty() {
                    out.push("=VAL :".to_string());
                } else {
                    let (anchor, body_tag, rest) = decompose_scalar(body, handles);
                    out.push(scalar_event(anchor, body_tag.as_deref(), rest));
                }
                idx += 1;
                // Look ahead for the matching `:value` line. Skip
                // intervening newlines, whitespace, and comments. Stop at
                // anything else — that means the value is implicitly null.
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
                            // Colon found: collect value tokens up to the
                            // next NEWLINE.
                            let mut value_tag: Option<String> = None;
                            let mut value_text = String::new();
                            let mut value_end = peek + 1;
                            while value_end < children.len() {
                                if let rowan::NodeOrToken::Token(vt) = &children[value_end] {
                                    if vt.kind() == SyntaxKind::NEWLINE {
                                        break;
                                    }
                                    if vt.kind() == SyntaxKind::YAML_TAG && value_tag.is_none() {
                                        value_tag = Some(vt.text().to_string());
                                    } else if matches!(
                                        vt.kind(),
                                        SyntaxKind::YAML_SCALAR
                                            | SyntaxKind::YAML_ANCHOR
                                            | SyntaxKind::YAML_ALIAS
                                            | SyntaxKind::WHITESPACE
                                    ) {
                                        value_text.push_str(vt.text());
                                    }
                                    value_end += 1;
                                } else {
                                    break;
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
                    // Non-trivia, non-colon: implicit null value.
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

/// Project a YAML_BLOCK_MAP_KEY whose content is a nested collection — the
/// explicit-key `? <seq-or-map>` shape — into the key position. Mirrors the
/// nested-collection branches of [`project_block_map_entry_value`]. Returns
/// `true` when a collection child was found and projected, `false` when the
/// key is a plain scalar the caller should handle with its token-join logic.
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
                // A flow collection in key position may carry an anchor
                // sitting as a sibling token inside the KEY wrapper
                // (`&key [a, b]: value`, 6BFJ). Surface it on the open
                // event so the projection matches `+SEQ [] &key …`.
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

    // Explicit-key (`?`) entry whose key content is a nested collection (block
    // or flow sequence/map) rather than a scalar. The collection lives as a
    // child NODE of YAML_BLOCK_MAP_KEY, so the token-join key-text logic below
    // sees only the `?` indicator and would emit an empty `=VAL :`. Project the
    // collection in the key position instead. M5DY: block/flow seq keys; V9D5:
    // nested block-map key.
    if project_block_map_key_collection(&key_node, handles, out) {
        project_block_map_entry_value(&value_node, handles, out);
        return;
    }

    let key_tag = key_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
        .map(|tok| tok.text().to_string());
    // The key text lives in either a `YAML_KEY` token (v1's emission, used
    // both for the explicit `?` indicator and for implicit key text) or
    // a `YAML_SCALAR` token (v2's emission, where wrapper position
    // carries the role and the explicit `?` is the only `YAML_KEY`).
    // Concatenate matching tokens — interleave WHITESPACE / NEWLINE so the
    // explicit `?` and any subsequent key scalar are separated by their
    // original trivia, letting `strip_explicit_key_indicator` recognize
    // the `?<sp>` pattern. Stops at the trailing `:` (`YAML_COLON`).
    // Falls back to empty for the empty-implicit-key shorthand
    // (`: value` — KEY wrapper holds only the colon).
    let key_text = key_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .take_while(|tok| tok.kind() != SyntaxKind::YAML_COLON)
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_KEY
                    | SyntaxKind::YAML_SCALAR
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::WHITESPACE
                    | SyntaxKind::NEWLINE
            )
        })
        .map(|tok| tok.text().to_string())
        .collect::<Vec<_>>()
        .join("");
    let key_text = key_text.trim_end().to_string();

    // Strip an explicit-key `?` indicator that precedes the actual key
    // text. v2 emits the `?` as a `YAML_KEY` token sibling of the
    // `YAML_SCALAR`, so it ends up in `key_text` after the join above.
    // v1 wouldn't reach this strip because its v1-shape `YAML_KEY`
    // token carried only the implicit key body.
    let key_trimmed = strip_explicit_key_indicator(key_text.trim());
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
    } else if key_tag.is_none()
        && let Some((indicator, body)) = extract_block_scalar_body(&key_node)
    {
        // Explicit-key whose key is itself a literal (`|`) or folded
        // (`>`) block scalar (5WE3, KK5P complex4).
        // `extract_block_scalar_body` ignores the `?` indicator (a
        // `YAML_KEY` token) and the trailing `:` (`YAML_COLON`), folding
        // only the scalar body — the same path as a block-scalar value.
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
            folded = fold_quoted_inner(body, false);
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

    // A flow-sequence value with embedded `:` (an implicit flow-map
    // entry inside `[...]`, e.g. 87E4 / L9U5 / LQZ7) needs the
    // CST-walking item projector — the text-based fallback below
    // strips colons during `value_text` assembly so `flow_kv_split`
    // never sees them and the entry collapses into one bare scalar.
    if let Some(flow_seq) = value_node
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
    {
        let anchor = anchor_preceding_node(value_node, &flow_seq);
        project_flow_collection_node_with_anchor(&flow_seq, anchor.as_deref(), handles, out);
        return;
    }

    if let Some((indicator, body)) = extract_block_scalar_body(value_node) {
        // Tag/anchor siblings of the block scalar (e.g. `!foo >1\n value`,
        // `!!binary | ...`) decorate the scalar — splice them into the
        // event in canonical `&anchor <tag> <indicator>body` order.
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

    // Block scalar preceded by node properties (`!foo >1\n value`): the tag /
    // anchor live inside the scalar token, so the bare detector above misses
    // it. Re-read with the leading properties peeled off and emit them in
    // canonical `&anchor <tag> <indicator>body` order.
    if let Some((anchor, long_tag, indicator, body)) =
        extract_tagged_block_scalar(value_node, handles)
    {
        let mut prefix = String::new();
        if let Some(a) = anchor {
            prefix.push_str(&format!("&{a} "));
        }
        if let Some(t) = long_tag {
            prefix.push_str(&t);
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
    // Include WHITESPACE between scalar-ish tokens so a value like
    // `&anchor body` joins as `&anchor body` (not `&anchorbody`),
    // letting `decompose_scalar` find the whitespace terminator on the
    // anchor name. The scanner emits multi-line plain scalars as one
    // YAML_SCALAR token, so the WS we pick up here is only the inter-
    // property indentation that yaml-test-suite trims anyway.
    let value_text = value_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::WHITESPACE
            )
        })
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
        let trimmed = value_text.trim();
        if trimmed.starts_with('"') || trimmed.starts_with('\'') {
            // Multi-line quoted scalar value: rebuild the source text with
            // newlines intact (parser splits each physical line into its own
            // YAML_SCALAR token), then run the YAML 1.2 §7.3 line-folding
            // rules so blank lines fold to `\n` and single breaks fold to
            // space. Without this, joining YAML_SCALAR tokens directly drops
            // line structure (yaml-test-suite case XV9V).
            let multi_line_text = collect_value_scalar_text_with_newlines(value_node);
            // Strip trailing whitespace/newlines that come AFTER the
            // closing quote. v2 keeps a single quoted-scalar token so
            // those bytes are post-value trivia (NEWLINE) — they don't
            // make the scalar body multi-line. Without this trim, a
            // single-line quoted with trailing significant whitespace
            // (J3BT's `"Quoted \t"`) hits the multi-line folder which
            // strips trailing tabs/spaces from the scalar body.
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
                // A tag/anchor can precede a multi-line double-quoted value
                // (`!!binary "\\\n …"`, 565N), so the quoted branch above is
                // skipped. Enable §7.5 escaped line breaks when the decomposed
                // body is itself double-quoted; the later `decode_double_quoted`
                // in `scalar_event` strips the quotes and remaining escapes.
                let escaped_breaks = body.trim_start().starts_with('"');
                folded = fold_quoted_inner(body, escaped_breaks);
                &folded
            } else {
                body
            };
            out.push(scalar_event(anchor, long_tag.as_deref(), body_for_event));
        }
    }
}

/// Reconstruct a YAML_BLOCK_MAP_VALUE's scalar text with line breaks intact
/// for multi-line quoted-scalar folding. Mirrors
/// [`collect_doc_scalar_text_with_newlines`] but bounded to a single
/// block-map value so it doesn't pull in scalars from nested blocks.
fn collect_value_scalar_text_with_newlines(value_node: &SyntaxNode) -> String {
    value_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| {
            matches!(
                tok.kind(),
                SyntaxKind::YAML_SCALAR
                    | SyntaxKind::YAML_ANCHOR
                    | SyntaxKind::YAML_ALIAS
                    | SyntaxKind::NEWLINE
            )
        })
        .map(|tok| tok.text().to_string())
        .collect()
}
