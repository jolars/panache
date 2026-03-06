use crate::bib::Span;

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
