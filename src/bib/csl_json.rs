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

/// Legacy function: extract only citation keys and spans.
pub fn parse_csl_json_entries(input: &str) -> Result<Vec<(String, Span)>, String> {
    let mut entries = Vec::new();
    let bytes = input.as_bytes();
    let mut idx = 0usize;

    while idx < bytes.len() {
        if bytes[idx] != b'"' {
            idx += 1;
            continue;
        }

        let (key, _, next_idx) = parse_json_string(input, idx)?;
        let mut cursor = skip_whitespace(bytes, next_idx);
        if bytes.get(cursor) != Some(&b':') {
            idx = next_idx;
            continue;
        }

        cursor = skip_whitespace(bytes, cursor + 1);
        if bytes.get(cursor) != Some(&b'"') {
            idx = cursor;
            continue;
        }

        let (value, span, value_end) = parse_json_string(input, cursor)?;
        if key == "id" && !value.is_empty() {
            entries.push((value, span));
        }
        idx = value_end;
    }

    Ok(entries)
}

fn skip_whitespace(bytes: &[u8], mut idx: usize) -> usize {
    while matches!(bytes.get(idx), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        idx += 1;
    }
    idx
}

fn parse_json_string(input: &str, start: usize) -> Result<(String, Span, usize), String> {
    let bytes = input.as_bytes();
    if bytes.get(start) != Some(&b'"') {
        return Err("expected JSON string".to_string());
    }

    let mut output = String::new();
    let mut idx = start + 1;

    while idx < bytes.len() {
        match bytes[idx] {
            b'"' => {
                let span = Span {
                    start: start + 1,
                    end: idx,
                };
                return Ok((output, span, idx + 1));
            }
            b'\\' => {
                idx += 1;
                if idx >= bytes.len() {
                    return Err("unterminated JSON escape".to_string());
                }
                match bytes[idx] {
                    b'"' => output.push('"'),
                    b'\\' => output.push('\\'),
                    b'/' => output.push('/'),
                    b'b' => output.push('\u{0008}'),
                    b'f' => output.push('\u{000C}'),
                    b'n' => output.push('\n'),
                    b'r' => output.push('\r'),
                    b't' => output.push('\t'),
                    b'u' => {
                        let hex_end = idx + 5;
                        if hex_end > bytes.len() {
                            return Err("unterminated JSON unicode escape".to_string());
                        }
                        let hex = &input[idx + 1..hex_end];
                        let code = u32::from_str_radix(hex, 16)
                            .map_err(|_| "invalid JSON unicode escape".to_string())?;
                        let ch = char::from_u32(code)
                            .ok_or_else(|| "invalid unicode codepoint".to_string())?;
                        output.push(ch);
                        idx += 4;
                    }
                    _ => return Err("invalid JSON escape".to_string()),
                }
                idx += 1;
            }
            _ => {
                let ch = input[idx..]
                    .chars()
                    .next()
                    .ok_or_else(|| "invalid UTF-8 while parsing JSON string".to_string())?;
                output.push(ch);
                idx += ch.len_utf8();
            }
        }
    }

    Err("unterminated JSON string".to_string())
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
