use serde::Deserialize;
use std::collections::HashMap;

use crate::bib::{ParsedEntry, Span};

/// CSL-YAML entry structure.
#[derive(Debug, Deserialize)]
struct CslYamlEntry {
    id: String,
    #[serde(rename = "type")]
    entry_type: Option<String>,
    #[serde(flatten)]
    fields: HashMap<String, serde_json::Value>,
}

/// Parse CSL-YAML file and extract full entry data (id, type, fields).
///
/// Returns a vector of (id, entry_type, fields, span) tuples.
pub fn parse_csl_yaml_full(input: &str) -> Result<Vec<ParsedEntry>, String> {
    // CSL-YAML can be either an array of entries or a single entry
    let entries: Vec<CslYamlEntry> = serde_saphyr::from_str(input)
        .or_else(|_| {
            // Try parsing as a single entry wrapped in array
            let single: CslYamlEntry =
                serde_saphyr::from_str(input).map_err(|e| format!("Invalid CSL-YAML: {}", e))?;
            Ok(vec![single])
        })
        .map_err(|e: String| e)?;

    let mut result = Vec::new();

    for entry in entries {
        let id = entry.id;
        let span = find_yaml_id_span(input, &id);

        // Convert serde_json::Value fields to strings
        let mut string_fields = HashMap::new();
        for (key, value) in entry.fields {
            string_fields.insert(key, json_value_to_string(&value));
        }

        result.push((id, entry.entry_type, string_fields, span));
    }

    Ok(result)
}

/// Legacy function: extract only citation keys and spans.
pub fn parse_csl_yaml_entries(input: &str) -> Result<Vec<(String, Span)>, String> {
    let mut entries = Vec::new();
    let mut offset = 0usize;

    for line in input.split('\n') {
        let trimmed = line.trim_start();
        let key_start = line.len() - trimmed.len();
        let value_start = if trimmed.starts_with("id:") {
            key_start + 3
        } else if trimmed.starts_with("- id:") {
            key_start + 5
        } else {
            offset += line.len() + 1;
            continue;
        };

        let mut value_idx = value_start;
        while line.as_bytes().get(value_idx) == Some(&b' ') {
            value_idx += 1;
        }
        if value_idx >= line.len() {
            offset += line.len() + 1;
            continue;
        }

        let (value, span) = parse_yaml_value(line, offset, value_idx);
        if !value.is_empty() {
            entries.push((value, span));
        }
        offset += line.len() + 1;
    }

    Ok(entries)
}

fn parse_yaml_value(line: &str, line_offset: usize, value_idx: usize) -> (String, Span) {
    let bytes = line.as_bytes();
    let quote = bytes.get(value_idx).copied();
    if matches!(quote, Some(b'"') | Some(b'\'')) {
        let quote_char = bytes[value_idx] as char;
        if let Some(end_rel) = line[value_idx + 1..].find(quote_char) {
            let start = value_idx + 1;
            let end = value_idx + 1 + end_rel;
            let value = line[start..end].to_string();
            let span = Span {
                start: line_offset + start,
                end: line_offset + end,
            };
            return (value, span);
        }
    }

    let mut raw = &line[value_idx..];
    if let Some(comment_idx) = raw.find('#') {
        raw = &raw[..comment_idx];
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (String::new(), Span { start: 0, end: 0 });
    }
    let trim_start = raw.find(trimmed).unwrap_or(0);
    let start = value_idx + trim_start;
    let end = start + trimmed.len();
    (
        trimmed.to_string(),
        Span {
            start: line_offset + start,
            end: line_offset + end,
        },
    )
}

/// Convert serde_json::Value to display string (used for flattened fields).
fn json_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Array(arr) => {
            // Handle author arrays
            if let Some(serde_json::Value::Object(first)) = arr.first()
                && first.contains_key("family")
            {
                let names: Vec<String> = arr
                    .iter()
                    .filter_map(|v| {
                        let obj = v.as_object()?;
                        let family = obj.get("family")?.as_str()?;
                        let given = obj.get("given").and_then(|g| g.as_str());
                        Some(if let Some(g) = given {
                            format!("{}, {}", family, g)
                        } else {
                            family.to_string()
                        })
                    })
                    .collect();
                if !names.is_empty() {
                    return names.join("; ");
                }
            }

            // Fallback: join as strings
            arr.iter()
                .map(json_value_to_string)
                .collect::<Vec<_>>()
                .join(", ")
        }
        serde_json::Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
        serde_json::Value::Null => String::new(),
    }
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
