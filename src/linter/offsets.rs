pub(crate) fn line_col_to_byte_offset_1based(
    input: &str,
    line: usize,
    column: usize,
) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }

    let mut current_line = 1usize;
    let mut offset = 0usize;
    let bytes = input.as_bytes();

    for text_line in input.lines() {
        if current_line == line {
            let line_byte_offset = text_line
                .char_indices()
                .nth(column - 1)
                .map(|(byte_idx, _)| byte_idx)
                .unwrap_or(text_line.len());
            return Some(offset + line_byte_offset);
        }

        let line_end_offset = offset + text_line.len();
        let line_ending_len = if line_end_offset + 1 < input.len()
            && bytes[line_end_offset] == b'\r'
            && bytes[line_end_offset + 1] == b'\n'
        {
            2
        } else if line_end_offset < input.len() && bytes[line_end_offset] == b'\n' {
            1
        } else {
            0
        };

        offset += text_line.len() + line_ending_len;
        current_line += 1;
    }

    if current_line == line {
        Some(offset)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::line_col_to_byte_offset_1based;

    #[test]
    fn handles_unicode_scalar_columns() {
        let input = "éx\n";
        assert_eq!(line_col_to_byte_offset_1based(input, 1, 1), Some(0));
        assert_eq!(line_col_to_byte_offset_1based(input, 1, 2), Some(2));
        assert_eq!(line_col_to_byte_offset_1based(input, 1, 3), Some(3));
    }

    #[test]
    fn handles_crlf_lines() {
        let input = "a\r\nbé\r\n";
        assert_eq!(line_col_to_byte_offset_1based(input, 1, 2), Some(1));
        assert_eq!(line_col_to_byte_offset_1based(input, 2, 2), Some(4));
        assert_eq!(line_col_to_byte_offset_1based(input, 2, 3), Some(6));
    }
}
