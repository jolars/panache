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

use crate::syntax::{SyntaxKind, SyntaxNode};

use super::parser::parse_yaml_tree;

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

    if let Some(seq_node) = doc
        .descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
    {
        out.push("+SEQ".to_string());
        project_block_sequence_items(&seq_node, out);
        out.push("-SEQ".to_string());
    } else if let Some(root_map) = doc
        .descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
    {
        let mut values = Vec::new();
        project_block_map_entries(&root_map, &mut values);
        if !values.is_empty() {
            out.push("+MAP".to_string());
            out.append(&mut values);
            out.push("-MAP".to_string());
        } else if let Some(flow_map) = doc
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
        {
            let mut flow_values = Vec::new();
            project_flow_map_entries(&flow_map, &mut flow_values);
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
                if item.starts_with('"') || item.starts_with('\'') {
                    out.push(quoted_val_event(&item));
                } else {
                    out.push(plain_val_event(&item));
                }
            }
            out.push("-SEQ".to_string());
        } else if let Some(scalar) = scalar_document_value(doc) {
            out.push(scalar);
        } else {
            out.push("=VAL :".to_string());
        }
    } else if let Some(flow_map) = doc
        .descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
    {
        out.push("+MAP {}".to_string());
        project_flow_map_entries(&flow_map, out);
        out.push("-MAP".to_string());
    } else if let Some(flow_seq) = doc
        .descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
        && let Some(items) = simple_flow_sequence_items(&flow_seq.text().to_string())
    {
        out.push("+SEQ []".to_string());
        for item in items {
            if item.starts_with('"') || item.starts_with('\'') {
                out.push(quoted_val_event(&item));
            } else {
                out.push(plain_val_event(&item));
            }
        }
        out.push("-SEQ".to_string());
    } else if let Some(scalar) = scalar_document_value(doc) {
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

fn scalar_document_value(doc: &SyntaxNode) -> Option<String> {
    let text = doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| tok.kind() == SyntaxKind::YAML_SCALAR)
        .map(|tok| tok.text().to_string())
        .collect::<Vec<_>>()
        .join("");
    if text.is_empty() {
        return None;
    }
    let tag_text = doc
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::YAML_TAG)
        .map(|tok| tok.text().to_string());
    let event = if let Some(tag) = tag_text
        && let Some(long) = long_tag(&tag)
    {
        format!("=VAL {long} :{text}")
    } else if text.starts_with('"') || text.starts_with('\'') {
        quoted_val_event(&text)
    } else {
        plain_val_event(&text)
    };
    Some(event)
}

fn plain_val_event(text: &str) -> String {
    format!("=VAL :{}", text.replace('\\', "\\\\"))
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

fn long_tag(tag: &str) -> Option<String> {
    let builtin: Option<&'static str> = match tag {
        "!!str" => Some("<tag:yaml.org,2002:str>"),
        "!!int" => Some("<tag:yaml.org,2002:int>"),
        "!!bool" => Some("<tag:yaml.org,2002:bool>"),
        "!!null" => Some("<tag:yaml.org,2002:null>"),
        "!!float" => Some("<tag:yaml.org,2002:float>"),
        "!!seq" => Some("<tag:yaml.org,2002:seq>"),
        "!!map" => Some("<tag:yaml.org,2002:map>"),
        _ => None,
    };
    if let Some(s) = builtin {
        return Some(s.to_string());
    }
    if tag == "!" {
        return Some("<!>".to_string());
    }
    if tag.starts_with('!') && !tag.starts_with("!!") {
        return Some(format!("<{tag}>"));
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
        if !trimmed.is_empty() {
            pieces.push(trimmed.to_string());
        }
    }
    if pieces.is_empty() {
        return String::new();
    }
    pieces.join(" ")
}

fn project_flow_map_entries(flow_map: &SyntaxNode, out: &mut Vec<String>) {
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
        let raw_value = value_node
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_SCALAR)
            .map(|tok| tok.text().to_string())
            .collect::<Vec<_>>()
            .join("");

        if has_explicit_colon {
            out.push(plain_val_event(&fold_plain_scalar(&raw_key)));
            out.push(plain_val_event(&fold_plain_scalar(&raw_value)));
        } else {
            let combined = format!("{raw_key}{raw_value}");
            out.push(plain_val_event(&fold_plain_scalar(&combined)));
            out.push("=VAL :".to_string());
        }
    }
}

fn project_block_sequence_items(seq_node: &SyntaxNode, out: &mut Vec<String>) {
    for item in seq_node
        .children()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
    {
        if let Some(nested_seq) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
        {
            out.push("+SEQ".to_string());
            project_block_sequence_items(&nested_seq, out);
            out.push("-SEQ".to_string());
            continue;
        }
        if let Some(nested_map) = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
        {
            out.push("+MAP".to_string());
            project_block_map_entries(&nested_map, out);
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
                    if value.starts_with('"') || value.starts_with('\'') {
                        out.push(quoted_val_event(&value));
                    } else {
                        out.push(plain_val_event(&value));
                    }
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
            project_flow_map_entries(&flow_map, out);
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
        let scalar_trimmed = scalar_text.trim_end();
        let event = if let Some(tag) = item_tag
            && let Some(long) = long_tag(&tag)
        {
            format!("=VAL {long} :{scalar_text}")
        } else if let Some(rest) = scalar_trimmed.strip_prefix('&') {
            if let Some((anchor, value)) = rest.split_once(' ') {
                format!("=VAL &{anchor} :{value}")
            } else {
                format!("=VAL &{rest} :")
            }
        } else if scalar_trimmed.starts_with('*') {
            format!("=ALI {scalar_trimmed}")
        } else if scalar_text.starts_with('"') || scalar_text.starts_with('\'') {
            quoted_val_event(&scalar_text)
        } else {
            plain_val_event(&scalar_text)
        };
        out.push(event);
    }
}

fn project_block_map_entries(map_node: &SyntaxNode, out: &mut Vec<String>) {
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
            .map(|tok| tok.text().to_string())
            .expect("key token");

        let key_event = if let Some(tag) = key_tag {
            if let Some(long) = long_tag(&tag) {
                format!("=VAL {long} :{key_text}")
            } else {
                plain_val_event(&key_text)
            }
        } else if let Some(rest) = key_text.strip_prefix('&') {
            if let Some((anchor, value)) = rest.split_once(' ') {
                format!("=VAL &{} :{}", anchor, value)
            } else {
                format!("=VAL &{} :", rest)
            }
        } else if key_text.starts_with('"') || key_text.starts_with('\'') {
            quoted_val_event(&key_text)
        } else if key_text.starts_with('*') {
            format!("=ALI {}", key_text.trim_end())
        } else {
            plain_val_event(&key_text)
        };
        out.push(key_event);

        if let Some(nested_map) = value_node
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
        {
            out.push("+MAP".to_string());
            project_block_map_entries(&nested_map, out);
            out.push("-MAP".to_string());
            continue;
        }

        if let Some(flow_map) = value_node
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
        {
            out.push("+MAP {}".to_string());
            project_flow_map_entries(&flow_map, out);
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
                if item.starts_with('"') || item.starts_with('\'') {
                    out.push(quoted_val_event(&item));
                } else {
                    out.push(plain_val_event(&item));
                }
            }
            out.push("-SEQ".to_string());
        } else if value_text.is_empty() {
            out.push("=VAL :".to_string());
        } else {
            let value_event = if let Some(tag) = value_tag {
                if let Some(long) = long_tag(&tag) {
                    if let Some(rest) = value_text.strip_prefix('&') {
                        if let Some((anchor, tail)) = rest.split_once(' ') {
                            format!("=VAL &{anchor} {long} :{tail}")
                        } else {
                            format!("=VAL &{rest} {long} :")
                        }
                    } else {
                        format!("=VAL {long} :{value_text}")
                    }
                } else {
                    plain_val_event(&value_text)
                }
            } else if value_text.starts_with('"') || value_text.starts_with('\'') {
                quoted_val_event(&value_text)
            } else if let Some(rest) = value_text.strip_prefix('&') {
                if let Some((anchor, value)) = rest.split_once(' ') {
                    format!("=VAL &{} :{}", anchor, value)
                } else {
                    format!("=VAL &{} :", rest)
                }
            } else if value_text.starts_with('*') {
                format!("=ALI {value_text}")
            } else {
                plain_val_event(&value_text)
            };
            out.push(value_event);
        }
    }
}
