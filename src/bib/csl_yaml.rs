use rowan::ast::AstNode as _;
use std::collections::HashMap;

use crate::bib::{ParsedEntry, Span};

/// Parse CSL-YAML file and extract full entry data (id, type, fields).
///
/// Returns a vector of (id, entry_type, fields, span) tuples.
pub fn parse_csl_yaml_full(input: &str) -> Result<Vec<ParsedEntry>, String> {
    let entry_maps = parse_csl_entry_maps(input)?;
    let mut result = Vec::new();
    for entry in entry_maps {
        let id =
            block_map_value_to_scalar(map_entry_value(&entry, "id").ok_or_else(|| {
                "Invalid CSL-YAML: entry missing required 'id' field".to_string()
            })?)
            .ok_or_else(|| "Invalid CSL-YAML: 'id' must be a scalar".to_string())?;
        let entry_type = map_entry_value(&entry, "type").and_then(block_map_value_to_scalar);
        let span = find_yaml_id_span(input, &id);
        let mut string_fields = HashMap::new();
        for item in entry.entries() {
            let Some(key) = block_map_entry_key(&item) else {
                continue;
            };
            let Some(value) = item.value() else {
                continue;
            };
            if key == "id" || key == "type" {
                continue;
            }
            string_fields.insert(key, block_map_value_to_string(&value));
        }
        result.push((id, entry_type, string_fields, span));
    }
    Ok(result)
}

/// Legacy function: extract only citation keys and spans.
pub fn parse_csl_yaml_entries(input: &str) -> Result<Vec<(String, Span)>, String> {
    parse_csl_yaml_full(input).map(|entries| {
        entries
            .into_iter()
            .map(|(id, _entry_type, _fields, span)| (id, span))
            .collect()
    })
}

fn parse_csl_entry_maps(input: &str) -> Result<Vec<yaml_parser::ast::BlockMap>, String> {
    let root = yaml_parser::ast::Root::cast(
        yaml_parser::parse(input).map_err(|err| format!("Invalid CSL-YAML: {}", err.message()))?,
    )
    .ok_or_else(|| "Invalid CSL-YAML: missing root node".to_string())?;
    let document = root
        .documents()
        .next()
        .ok_or_else(|| "Invalid CSL-YAML: no YAML document found".to_string())?;
    let block = document
        .block()
        .ok_or_else(|| "Invalid CSL-YAML: expected block content".to_string())?;
    if let Some(map) = block.block_map() {
        return Ok(vec![map]);
    }
    let seq = block
        .block_seq()
        .ok_or_else(|| "Invalid CSL-YAML: expected sequence of entries".to_string())?;
    let mut entries = Vec::new();
    for entry in seq.entries() {
        let Some(entry_block) = entry.block() else {
            return Err("Invalid CSL-YAML: sequence entry must be a mapping".to_string());
        };
        let Some(map) = entry_block.block_map() else {
            return Err("Invalid CSL-YAML: sequence entry must be a mapping".to_string());
        };
        entries.push(map);
    }
    Ok(entries)
}

fn map_entry_value(
    map: &yaml_parser::ast::BlockMap,
    key: &str,
) -> Option<yaml_parser::ast::BlockMapValue> {
    map.entries()
        .find(|entry| block_map_entry_key(entry).as_deref() == Some(key))
        .and_then(|entry| entry.value())
}

fn block_map_entry_key(entry: &yaml_parser::ast::BlockMapEntry) -> Option<String> {
    let key = entry.key()?;
    if let Some(flow) = key.flow() {
        return flow_scalar_text(&flow);
    }
    let block = key.block()?;
    let flow = block_to_flow_scalar(&block)?;
    flow_scalar_text(&flow)
}

fn block_map_value_to_scalar(value: yaml_parser::ast::BlockMapValue) -> Option<String> {
    if let Some(flow) = value.flow() {
        return flow_scalar_text(&flow);
    }
    let block = value.block()?;
    let flow = block_to_flow_scalar(&block)?;
    flow_scalar_text(&flow)
}

fn block_map_value_to_string(value: &yaml_parser::ast::BlockMapValue) -> String {
    if let Some(author) = author_list_to_string(value) {
        return author;
    }
    block_map_value_to_scalar(value.clone()).unwrap_or_else(|| value.syntax().text().to_string())
}

fn author_list_to_string(value: &yaml_parser::ast::BlockMapValue) -> Option<String> {
    let block = value.block()?;
    let seq = block.block_seq()?;
    let mut names = Vec::new();
    for entry in seq.entries() {
        let entry_block = entry.block()?;
        let map = entry_block.block_map()?;
        let family = map_entry_value(&map, "family").and_then(block_map_value_to_scalar)?;
        let given = map_entry_value(&map, "given").and_then(block_map_value_to_scalar);
        names.push(match given {
            Some(given) if !given.is_empty() => format!("{}, {}", family, given),
            _ => family,
        });
    }
    (!names.is_empty()).then(|| names.join("; "))
}

fn block_to_flow_scalar(block: &yaml_parser::ast::Block) -> Option<yaml_parser::ast::Flow> {
    block
        .syntax()
        .children()
        .find_map(yaml_parser::ast::Flow::cast)
}

fn flow_scalar_text(flow: &yaml_parser::ast::Flow) -> Option<String> {
    if let Some(token) = flow.plain_scalar() {
        return Some(token.text().to_string());
    }
    if let Some(token) = flow.single_quoted_scalar() {
        return Some(token.text().trim_matches('\'').to_string());
    }
    if let Some(token) = flow.double_qouted_scalar() {
        return Some(token.text().trim_matches('"').to_string());
    }
    None
}

/// Find span of id field in YAML text.
fn find_yaml_id_span(input: &str, id: &str) -> Span {
    // Simple heuristic: search for "id: value" pattern
    if let Some(pos) = input.find(&format!("id: {}", id)) {
        let start = pos + 4; // Skip "id: "
        return Span {
            start,
            end: start + id.len(),
        };
    }

    // Try quoted version
    if let Some(pos) = input.find(&format!(r#"id: "{}""#, id)) {
        let start = pos + 5; // Skip 'id: "'
        return Span {
            start,
            end: start + id.len(),
        };
    }

    // Fallback: zero span
    Span { start: 0, end: 0 }
}
