//! Shared hashpipe header detection and normalization utilities.
//!
//! This module detects the contiguous hashpipe YAML preamble at the start of a
//! code block content string, strips line prefixes into normalized YAML text,
//! and records deterministic host↔normalized range mappings.

use std::ops::Range;

/// Prefix markers explicitly supported by hashpipe normalization.
pub const SUPPORTED_HASHPIPE_PREFIXES: [&str; 3] = ["#|", "//|", "--|"];

/// Per-line mapping between host (original content) and normalized YAML text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashpipeLineMapping {
    /// Byte range of the full host line (including newline, if present).
    pub host_line_range: Range<usize>,
    /// Byte range of stripped host line content (without trailing newline bytes).
    pub host_stripped_range: Range<usize>,
    /// Byte range of normalized line content (without normalized newline byte).
    pub normalized_content_range: Range<usize>,
    /// Byte range of normalized line including normalized newline, if present.
    pub normalized_line_range: Range<usize>,
    /// Host newline byte length for this line (0, 1 for LF, 2 for CRLF).
    pub host_newline_len: usize,
}

/// Result of hashpipe header detection and stripping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashpipeHeaderNormalization {
    /// Prefix that was used for detection and stripping.
    pub prefix: String,
    /// Number of contiguous hashpipe header lines consumed from the start.
    pub header_line_count: usize,
    /// Byte span of the detected header in host content.
    pub header_byte_span: Range<usize>,
    /// Prefix-stripped YAML text with deterministic `\n` newlines.
    pub normalized_yaml: String,
    /// Per-line host↔normalized mapping metadata.
    pub line_mappings: Vec<HashpipeLineMapping>,
}

#[derive(Debug, Clone, Copy)]
struct LineSlice<'a> {
    line_without_newline: &'a str,
    start: usize,
    end: usize,
    newline_len: usize,
}

/// Normalize a contiguous leading hashpipe header into YAML text.
///
/// Returns `None` when the input does not start with a hashpipe-prefixed line
/// for the provided prefix.
pub fn normalize_hashpipe_header(
    content: &str,
    prefix: &str,
) -> Option<HashpipeHeaderNormalization> {
    if !SUPPORTED_HASHPIPE_PREFIXES.contains(&prefix) {
        return None;
    }

    let lines = split_lines_with_offsets(content);
    if lines.is_empty() {
        return None;
    }

    let mut consumed = 0usize;
    let mut saw_prefix = false;

    while consumed < lines.len() {
        let line = lines[consumed];
        let trimmed = line.line_without_newline.trim_start_matches([' ', '\t']);

        if trimmed.starts_with(prefix) {
            saw_prefix = true;
            consumed += 1;
            continue;
        }

        break;
    }

    if !saw_prefix || consumed == 0 {
        return None;
    }

    let header_end = lines[consumed - 1].end;
    let mut normalized_yaml = String::new();
    let mut line_mappings = Vec::with_capacity(consumed);
    let mut normalized_pos = 0usize;

    for line in &lines[..consumed] {
        let stripped = strip_hashpipe_prefix_once(line.line_without_newline, prefix)?;

        let trimmed_start = line.line_without_newline.trim_start_matches([' ', '\t']);
        let leading_ws_len = line.line_without_newline.len() - trimmed_start.len();
        let after_prefix = &trimmed_start[prefix.len()..];
        let removed_space_len = usize::from(after_prefix.starts_with([' ', '\t']));
        let host_stripped_start = line.start + leading_ws_len + prefix.len() + removed_space_len;
        let host_stripped_end = line.start + line.line_without_newline.len();

        let normalized_content_start = normalized_pos;
        normalized_yaml.push_str(stripped);
        normalized_pos += stripped.len();
        if line.newline_len > 0 {
            normalized_yaml.push('\n');
            normalized_pos += 1;
        }

        line_mappings.push(HashpipeLineMapping {
            host_line_range: line.start..line.end,
            host_stripped_range: host_stripped_start..host_stripped_end,
            normalized_content_range: normalized_content_start
                ..(normalized_content_start + stripped.len()),
            normalized_line_range: normalized_content_start..normalized_pos,
            host_newline_len: line.newline_len,
        });
    }

    Some(HashpipeHeaderNormalization {
        prefix: prefix.to_string(),
        header_line_count: consumed,
        header_byte_span: 0..header_end,
        normalized_yaml,
        line_mappings,
    })
}

fn split_lines_with_offsets(content: &str) -> Vec<LineSlice<'_>> {
    let mut lines = Vec::new();
    let mut idx = 0usize;
    let bytes = content.as_bytes();

    while idx < content.len() {
        let mut end = idx;
        while end < content.len() && bytes[end] != b'\n' {
            end += 1;
        }
        if end < content.len() {
            end += 1; // include '\n'
        }

        let full = &content[idx..end];
        let newline_len = if full.ends_with("\r\n") {
            2
        } else if full.ends_with('\n') {
            1
        } else {
            0
        };
        let line_without_newline = &full[..full.len().saturating_sub(newline_len)];

        lines.push(LineSlice {
            line_without_newline,
            start: idx,
            end,
            newline_len,
        });

        idx = end;
    }

    lines
}

fn strip_hashpipe_prefix_once<'a>(line_without_newline: &'a str, prefix: &str) -> Option<&'a str> {
    let trimmed_start = line_without_newline.trim_start_matches([' ', '\t']);
    let after_prefix = trimmed_start.strip_prefix(prefix)?;
    if let Some(rest) = after_prefix.strip_prefix(' ') {
        return Some(rest);
    }
    if let Some(rest) = after_prefix.strip_prefix('\t') {
        return Some(rest);
    }
    Some(after_prefix)
}

#[cfg(test)]
mod tests {
    use super::normalize_hashpipe_header;

    #[test]
    fn normalizes_supported_prefixes() {
        for prefix in ["#|", "//|", "--|"] {
            let input = format!("{prefix} echo: true\n{prefix} warning: false\nx <- 1\n");
            let normalized = normalize_hashpipe_header(&input, prefix).expect("expected header");
            assert_eq!(normalized.header_line_count, 2);
            assert_eq!(
                normalized.header_byte_span,
                0..(input.lines().take(2).map(|l| l.len() + 1).sum())
            );
            assert_eq!(normalized.normalized_yaml, "echo: true\nwarning: false\n");
            assert_eq!(normalized.line_mappings.len(), 2);
        }
    }

    #[test]
    fn handles_multiline_quoted_value() {
        let input = "#| title: \"hello\n#|   world\"\n#| echo: true\nbody\n";
        let normalized = normalize_hashpipe_header(input, "#|").expect("expected header");
        assert_eq!(normalized.header_line_count, 3);
        assert_eq!(
            normalized.normalized_yaml,
            "title: \"hello\n  world\"\necho: true\n"
        );
    }

    #[test]
    fn handles_flow_collection_and_block_scalar_and_indented_value() {
        let flow = "#| tags: [a,\n#|   b,\n#|   c]\ncode\n";
        let flow_norm = normalize_hashpipe_header(flow, "#|").expect("expected flow header");
        assert_eq!(flow_norm.header_line_count, 3);
        assert_eq!(flow_norm.normalized_yaml, "tags: [a,\n  b,\n  c]\n");

        let block_scalar = "#| fig-cap: |\n#|   one\n#|   two\n#| echo: true\n";
        let block_norm =
            normalize_hashpipe_header(block_scalar, "#|").expect("expected scalar header");
        assert_eq!(block_norm.header_line_count, 4);
        assert_eq!(
            block_norm.normalized_yaml,
            "fig-cap: |\n  one\n  two\necho: true\n"
        );

        let indented = "#| fig-cap:\n#|   - A\n#|   - B\nplot()\n";
        let indented_norm =
            normalize_hashpipe_header(indented, "#|").expect("expected indented header");
        assert_eq!(indented_norm.header_line_count, 3);
        assert_eq!(indented_norm.normalized_yaml, "fig-cap:\n  - A\n  - B\n");
    }

    #[test]
    fn keeps_contiguous_prefixed_lines_even_when_not_option_shaped() {
        let input = "#| fig-subcap:\n#| - ROC\n#|  - PR Curve\nx <- 1\n";
        let normalized = normalize_hashpipe_header(input, "#|").expect("expected header");
        assert_eq!(normalized.header_line_count, 3);
        assert_eq!(
            normalized.normalized_yaml,
            "fig-subcap:\n- ROC\n - PR Curve\n"
        );
    }

    #[test]
    fn handles_no_header_and_partial_header() {
        assert!(normalize_hashpipe_header("plot(1:3)\n#| echo: true\n", "#|").is_none());

        let input = "#| echo: true\nplot(1:3)\n#| warning: false\n";
        let normalized = normalize_hashpipe_header(input, "#|").expect("expected leading header");
        assert_eq!(normalized.header_line_count, 1);
        assert_eq!(normalized.normalized_yaml, "echo: true\n");
        assert_eq!(normalized.header_byte_span.end, "#| echo: true\n".len());
    }

    #[test]
    fn handles_crlf_deterministically() {
        let input = "#| echo: true\r\n#|  warning: false\r\nbody\r\n";
        let normalized = normalize_hashpipe_header(input, "#|").expect("expected header");
        assert_eq!(normalized.header_line_count, 2);
        assert_eq!(normalized.normalized_yaml, "echo: true\n warning: false\n");
        assert_eq!(normalized.line_mappings[0].host_newline_len, 2);
        assert_eq!(normalized.line_mappings[1].host_newline_len, 2);
        assert_eq!(
            normalized.header_byte_span.end,
            "#| echo: true\r\n#|  warning: false\r\n".len()
        );
    }
}
