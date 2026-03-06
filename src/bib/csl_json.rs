use crate::bib::Span;

pub(crate) fn parse_csl_json_entries(input: &str) -> Result<Vec<(String, Span)>, String> {
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
