use serde::Deserialize;
use std::collections::HashMap;

use crate::bib::{ParsedEntry, Span};

/// CSL-JSON entry structure.
#[derive(Debug, Deserialize)]
struct CslJsonEntry {
    id: String,
    #[serde(rename = "type")]
    entry_type: Option<String>,
    #[serde(flatten)]
    fields: HashMap<String, serde_json::Value>,
}

/// Parse CSL-JSON file and extract full entry data (id, type, fields).
///
/// Returns a vector of (id, entry_type, fields, span) tuples where:
/// - `id`: Citation key
/// - `entry_type`: Optional type field (e.g., "article-journal")
/// - `fields`: All other fields as key-value strings
/// - `span`: Location of the id value in source
pub fn parse_csl_json_full(input: &str) -> Result<Vec<ParsedEntry>, String> {
    let entries: Vec<CslJsonEntry> =
        serde_json::from_str(input).map_err(|e| format!("Invalid CSL-JSON: {}", e))?;

    let mut result = Vec::new();

    for entry in entries {
        let id = entry.id;
        let span = find_id_span(input, &id).unwrap_or(Span { start: 0, end: 0 });

        // Convert serde_json::Value fields to strings
        let mut string_fields = HashMap::new();
        for (key, value) in entry.fields {
            string_fields.insert(key, value_to_string(&value));
        }

        result.push((id, entry.entry_type, string_fields, span));
    }

    Ok(result)
}

/// Helper: Convert JSON value to display string.
fn value_to_string(value: &serde_json::Value) -> String {
    use serde_json::Value;

    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(arr) => {
            // Special handling for common CSL patterns
            if arr.is_empty() {
                return String::new();
            }

            // Author arrays: extract names
            if let Some(first) = arr.first()
                && first.is_object()
            {
                // Likely author/contributor array
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

            // Date arrays: extract year
            if let Some(Value::Array(date_parts)) = arr.first()
                && let Some(Value::Number(year)) = date_parts.first()
            {
                return year.to_string();
            }

            // Fallback: JSON representation
            serde_json::to_string(arr).unwrap_or_default()
        }
        Value::Object(_) => {
            // For nested objects, use JSON representation
            serde_json::to_string(value).unwrap_or_default()
        }
        Value::Null => String::new(),
    }
}

/// Find the span of an "id" field value in CSL-JSON text.
///
/// This is a best-effort heuristic since we're not tracking spans during
/// serde parsing. It searches for the pattern `"id": "value"`.
fn find_id_span(input: &str, id: &str) -> Option<Span> {
    let pattern = r#""id""#.to_string();
    let id_key_pos = input.find(&pattern)?;

    // Find the colon after "id"
    let after_key = &input[id_key_pos + pattern.len()..];
    let colon_pos = after_key.find(':')?;

    // Find the opening quote of the value
    let after_colon = &after_key[colon_pos + 1..];
    let quote_offset = after_colon.find('"')?;

    let value_start = id_key_pos + pattern.len() + colon_pos + 1 + quote_offset + 1;

    // Verify this is the right ID
    if input[value_start..].starts_with(id) {
        Some(Span {
            start: value_start,
            end: value_start + id.len(),
        })
    } else {
        // Fallback: search for the exact id string
        None
    }
}
