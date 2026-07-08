use rowan::ast::AstNode as _;
use std::collections::HashMap;

use crate::bib::{ParsedEntry, Span};
use crate::syntax::{YamlBlockMap, YamlBlockMapValue, parse_yaml_document};

/// Parse CSL-YAML file and extract full entry data (id, type, fields).
///
/// Returns a vector of (id, entry_type, fields, span) tuples.
pub fn parse_csl_yaml_full(input: &str) -> Result<Vec<ParsedEntry>, String> {
    let entry_maps = parse_csl_entry_maps(input)?;
    let mut result = Vec::new();
    for entry in entry_maps {
        let id_value = map_entry_value(&entry, "id")
            .ok_or_else(|| "Invalid CSL-YAML: entry missing required 'id' field".to_string())?;
        let id = block_map_value_to_scalar(&id_value)
            .ok_or_else(|| "Invalid CSL-YAML: 'id' must be a scalar".to_string())?;
        let entry_type = map_entry_value(&entry, "type")
            .as_ref()
            .and_then(block_map_value_to_scalar);
        let span = find_yaml_id_span(input, &id);
        let mut string_fields = HashMap::new();
        for item in entry.entries() {
            let Some(key) = item.key_text() else {
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

fn parse_csl_entry_maps(input: &str) -> Result<Vec<YamlBlockMap>, String> {
    let document = parse_yaml_document(input)
        .ok_or_else(|| "Invalid CSL-YAML: missing root node".to_string())?;
    if let Some(map) = document.block_map() {
        return Ok(vec![map]);
    }
    let seq = document
        .block_sequence()
        .ok_or_else(|| "Invalid CSL-YAML: expected sequence of entries".to_string())?;
    let mut entries = Vec::new();
    for item in seq.items() {
        let Some(map) = item.as_block_map() else {
            return Err("Invalid CSL-YAML: sequence entry must be a mapping".to_string());
        };
        entries.push(map);
    }
    Ok(entries)
}

fn map_entry_value(map: &YamlBlockMap, key: &str) -> Option<YamlBlockMapValue> {
    map.value_of(key)
}

fn block_map_value_to_scalar(value: &YamlBlockMapValue) -> Option<String> {
    value.as_scalar().map(|scalar| scalar.value())
}

fn block_map_value_to_string(value: &YamlBlockMapValue) -> String {
    if let Some(author) = author_list_to_string(value) {
        return author;
    }
    block_map_value_to_scalar(value).unwrap_or_else(|| value.syntax().text().to_string())
}

fn author_list_to_string(value: &YamlBlockMapValue) -> Option<String> {
    let seq = value.as_block_sequence()?;
    let mut names = Vec::new();
    for item in seq.items() {
        let map = item.as_block_map()?;
        let family = map
            .value_of("family")
            .as_ref()
            .and_then(block_map_value_to_scalar)?;
        let given = map
            .value_of("given")
            .as_ref()
            .and_then(block_map_value_to_scalar);
        names.push(match given {
            Some(given) if !given.is_empty() => format!("{}, {}", family, given),
            _ => family,
        });
    }
    (!names.is_empty()).then(|| names.join("; "))
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
