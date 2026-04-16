//! Simple line-based RIS parser.
//!
//! RIS format is line-based with `TAG  - value` structure.
//! No CST needed - just parse line by line.

use std::collections::HashMap;

use crate::bib::{ParsedEntry, Span};

/// Parse RIS file and extract full entry data (id, type, fields).
///
/// Returns a vector of (id, entry_type, fields, span) tuples.
pub fn parse_ris_full(input: &str) -> Result<Vec<ParsedEntry>, String> {
    let records = parse_records(input)?;

    if records.is_empty() {
        return Err("RIS file contains no records".to_string());
    }

    let mut entries = Vec::new();
    for record in records {
        if let Some(entry) = extract_full_entry(&record)? {
            entries.push(entry);
        }
    }

    Ok(entries)
}

/// Legacy function: extract only citation keys.
pub fn parse_ris_entries(input: &str) -> Result<Vec<(String, Span)>, String> {
    let records = parse_records(input)?;

    if records.is_empty() {
        return Err("RIS file contains no records".to_string());
    }

    let mut entries = Vec::new();
    for record in records {
        if let Some((id, span)) = extract_id(&record)? {
            entries.push((id, span));
        }
    }

    Ok(entries)
}

/// Validate RIS file structure.
pub fn validate_ris(input: &str) -> Result<(), String> {
    let records = parse_records(input)?;

    if records.is_empty() {
        return Err("RIS file contains no records".to_string());
    }

    // Validation happens during parse_records
    Ok(())
}

/// A parsed RIS record with its tags.
#[derive(Debug)]
struct RisRecord {
    tags: Vec<RisTag>,
    #[allow(dead_code)] // May be useful for future error reporting
    start: usize,
    end: usize,
}

/// A single RIS tag with name, value, and position.
#[derive(Debug)]
struct RisTag {
    name: String,
    value: String,
    value_start: usize,
    value_end: usize,
}

/// Parse input into records (one record = TY...ER block).
fn parse_records(input: &str) -> Result<Vec<RisRecord>, String> {
    let mut records = Vec::new();
    let mut current_record: Option<RisRecord> = None;
    let mut line_start = 0;

    for line in input.lines() {
        let line_end = line_start + line.len();

        // Parse tag if present
        if let Some(tag) = parse_tag_line(line, line_start)? {
            // Start new record on TY
            if tag.name == "TY" {
                if let Some(_record) = current_record.take() {
                    return Err("RIS record missing ER tag".to_string());
                }
                current_record = Some(RisRecord {
                    tags: vec![tag],
                    start: line_start,
                    end: line_end,
                });
            }
            // End record on ER
            else if tag.name == "ER" {
                match current_record.as_mut() {
                    Some(record) => {
                        record.tags.push(tag);
                        record.end = line_end;
                        records.push(current_record.take().unwrap());
                    }
                    None => {
                        return Err("RIS record has ER tag without TY tag".to_string());
                    }
                }
            }
            // Regular tag
            else {
                match current_record.as_mut() {
                    Some(record) => {
                        record.tags.push(tag);
                        record.end = line_end;
                    }
                    None => {
                        return Err("RIS record contains tags outside TY/ER block".to_string());
                    }
                }
            }
        }
        // Handle continuation lines (leading whitespace)
        else if line.starts_with(|c: char| c.is_whitespace()) && !line.trim().is_empty() {
            match current_record.as_mut() {
                Some(record) => {
                    // Append to last tag's value (but not for ID, TY, or ER tags)
                    if let Some(last_tag) = record.tags.last_mut()
                        && last_tag.name != "ID"
                        && last_tag.name != "TY"
                        && last_tag.name != "ER"
                    {
                        if !last_tag.value.is_empty() {
                            last_tag.value.push(' ');
                        }
                        last_tag.value.push_str(line.trim());
                    }
                }
                None => {
                    return Err("RIS record contains invalid content".to_string());
                }
            }
        }
        // Empty lines are okay
        else if line.trim().is_empty() {
            // Skip
        }
        // Non-tag, non-continuation, non-empty lines are invalid
        else if !line.trim().is_empty() {
            return Err("RIS record contains invalid content".to_string());
        }

        line_start = line_end + 1; // +1 for newline
    }

    // Check for unclosed record
    if current_record.is_some() {
        return Err("RIS record missing ER tag".to_string());
    }

    Ok(records)
}

/// Parse a single line for a tag.
/// Returns None if line is not a tag line.
fn parse_tag_line(line: &str, line_start: usize) -> Result<Option<RisTag>, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    // Tag format: "AA  - value" where AA is 2 uppercase letters
    let bytes = trimmed.as_bytes();

    // Must start with 2 letters
    if bytes.len() < 2 {
        return Ok(None);
    }

    if !bytes[0].is_ascii_uppercase() || !bytes[1].is_ascii_uppercase() {
        return Ok(None);
    }

    let name = &trimmed[0..2];
    let rest = &trimmed[2..];

    // Should have whitespace and dash
    let rest = rest.trim_start();
    if !rest.starts_with('-') {
        return Ok(None);
    }

    let value = rest[1..].trim_start().to_string();

    // Calculate span for value (for ID tag)
    let value_offset = line.find(&value).unwrap_or(0);
    let value_start = line_start + value_offset;
    let value_end = value_start + value.len();

    Ok(Some(RisTag {
        name: name.to_string(),
        value,
        value_start,
        value_end,
    }))
}

/// Extract full entry data from a record.
fn extract_full_entry(record: &RisRecord) -> Result<Option<ParsedEntry>, String> {
    let mut has_ty = false;
    let mut has_er = false;
    let mut id_value: Option<(String, Span)> = None;
    let mut entry_type: Option<String> = None;
    let mut fields: HashMap<String, String> = HashMap::new();

    for tag in &record.tags {
        match tag.name.as_str() {
            "TY" => {
                has_ty = true;
                entry_type = Some(tag.value.clone());
            }
            "ER" => {
                has_er = true;
            }
            "ID" if id_value.is_none() && !tag.value.is_empty() => {
                id_value = Some((
                    tag.value.clone(),
                    Span {
                        start: tag.value_start,
                        end: tag.value_end,
                    },
                ));
            }
            _ => {
                // Store other fields
                fields.insert(tag.name.clone(), tag.value.clone());
            }
        }
    }

    if !has_ty {
        return Err("RIS record missing TY tag".to_string());
    }
    if !has_er {
        return Err("RIS record missing ER tag".to_string());
    }

    match id_value {
        Some((id, span)) => Ok(Some((id, entry_type, fields, span))),
        None => Ok(None),
    }
}

/// Extract only ID from a record.
fn extract_id(record: &RisRecord) -> Result<Option<(String, Span)>, String> {
    let mut has_ty = false;
    let mut has_er = false;
    let mut id_value: Option<(String, Span)> = None;

    for tag in &record.tags {
        match tag.name.as_str() {
            "TY" => has_ty = true,
            "ER" => has_er = true,
            "ID" => {
                if id_value.is_none() && !tag.value.is_empty() {
                    id_value = Some((
                        tag.value.clone(),
                        Span {
                            start: tag.value_start,
                            end: tag.value_end,
                        },
                    ));
                }
            }
            _ => {}
        }
    }

    if !has_ty {
        return Err("RIS record missing TY tag".to_string());
    }
    if !has_er {
        return Err("RIS record missing ER tag".to_string());
    }

    Ok(id_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_ris() {
        let input = "TY  - JOUR
ID  - Smith2020
AU  - Smith, John
TI  - Test Article
PY  - 2020
ER  - 
";
        let result = parse_ris_full(input).unwrap();
        assert_eq!(result.len(), 1);
        let (id, entry_type, fields, _span) = &result[0];
        assert_eq!(id, "Smith2020");
        assert_eq!(entry_type, &Some("JOUR".to_string()));
        assert_eq!(fields.get("AU"), Some(&"Smith, John".to_string()));
        assert_eq!(fields.get("TI"), Some(&"Test Article".to_string()));
        assert_eq!(fields.get("PY"), Some(&"2020".to_string()));
    }

    #[test]
    fn test_multiline_value() {
        let input = "TY  - JOUR
ID  - Test
TI  - First line
  Second line
  Third line
ER  - 
";
        let result = parse_ris_full(input).unwrap();
        assert_eq!(result.len(), 1);
        let (_id, _entry_type, fields, _span) = &result[0];
        assert_eq!(
            fields.get("TI"),
            Some(&"First line Second line Third line".to_string())
        );
    }

    #[test]
    fn test_missing_er() {
        let input = "TY  - JOUR
ID  - Test
";
        let result = parse_ris_full(input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing ER"));
    }

    #[test]
    fn test_missing_ty() {
        let input = "ID  - Test
ER  - 
";
        let result = parse_ris_full(input);
        assert!(result.is_err());
    }
}
