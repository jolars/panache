//! Blockquote parsing utilities.

/// Check if line starts with a blockquote marker (up to 3 spaces + >).
/// Returns (marker_end_byte, content_start_byte) if found.
pub(crate) fn try_parse_blockquote_marker(line: &str) -> Option<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut i = 0;

    // Skip up to 3 spaces
    let mut spaces = 0;
    while i < bytes.len() && bytes[i] == b' ' && spaces < 3 {
        spaces += 1;
        i += 1;
    }

    // Must have > next
    if i >= bytes.len() || bytes[i] != b'>' {
        return None;
    }
    let marker_end = i + 1;

    // Optional space after >
    let content_start = if marker_end < bytes.len() && bytes[marker_end] == b' ' {
        marker_end + 1
    } else {
        marker_end
    };

    Some((marker_end, content_start))
}

/// Count how many blockquote levels a line has, returning (depth, remaining_content).
pub(crate) fn count_blockquote_markers(line: &str) -> (usize, &str) {
    let mut depth = 0;
    let mut remaining = line;

    while let Some((_, content_start)) = try_parse_blockquote_marker(remaining) {
        depth += 1;
        remaining = &remaining[content_start..];
    }

    (depth, remaining)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_marker() {
        assert_eq!(try_parse_blockquote_marker("> text"), Some((1, 2)));
    }

    #[test]
    fn test_marker_no_space() {
        assert_eq!(try_parse_blockquote_marker(">text"), Some((1, 1)));
    }

    #[test]
    fn test_marker_with_leading_spaces() {
        assert_eq!(try_parse_blockquote_marker("   > text"), Some((4, 5)));
    }

    #[test]
    fn test_four_spaces_not_blockquote() {
        assert_eq!(try_parse_blockquote_marker("    > text"), None);
    }

    #[test]
    fn test_count_nested() {
        let (depth, content) = count_blockquote_markers("> > > nested");
        assert_eq!(depth, 3);
        assert_eq!(content, "nested");
    }
}
