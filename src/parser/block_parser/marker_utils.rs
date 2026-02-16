//! Consolidated utilities for parsing block markers (blockquotes, lists, definitions).
//!
//! This module provides common functionality for parsing markers that follow
//! similar patterns: optional leading spaces, marker character(s), optional trailing space.

/// Information about a single blockquote marker.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BlockQuoteMarkerInfo {
    pub leading_spaces: usize,
    pub has_trailing_space: bool,
}

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

/// Parse all blockquote markers from a line and return detailed info about each.
/// Returns Vec of BlockQuoteMarkerInfo for each marker found.
/// This is useful for lossless parsing where we need to preserve exact whitespace.
pub(crate) fn parse_blockquote_marker_info(line: &str) -> Vec<BlockQuoteMarkerInfo> {
    let mut markers = Vec::new();
    let mut remaining = line;

    loop {
        let bytes = remaining.as_bytes();
        let mut i = 0;

        // Count leading whitespace (up to 3 spaces before >)
        let mut spaces = 0;
        while i < bytes.len() && bytes[i] == b' ' && spaces < 3 {
            spaces += 1;
            i += 1;
        }

        // Check if there's a > marker
        if i >= bytes.len() || bytes[i] != b'>' {
            break;
        }
        i += 1; // skip '>'

        // Check for optional space after >
        let has_trailing_space = i < bytes.len() && bytes[i] == b' ';
        if has_trailing_space {
            i += 1;
        }

        markers.push(BlockQuoteMarkerInfo {
            leading_spaces: spaces,
            has_trailing_space,
        });
        remaining = &remaining[i..];
    }

    markers
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

    #[test]
    fn test_parse_marker_info_single() {
        let markers = parse_blockquote_marker_info("> text");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].leading_spaces, 0);
        assert_eq!(markers[0].has_trailing_space, true);
    }

    #[test]
    fn test_parse_marker_info_nested() {
        let markers = parse_blockquote_marker_info("> > > nested");
        assert_eq!(markers.len(), 3);
        assert_eq!(markers[0].leading_spaces, 0);
        assert_eq!(markers[1].leading_spaces, 0);
        assert_eq!(markers[2].leading_spaces, 0);
        assert!(markers.iter().all(|m| m.has_trailing_space));
    }

    #[test]
    fn test_parse_marker_info_with_leading_spaces() {
        let markers = parse_blockquote_marker_info("  > text");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].leading_spaces, 2);
        assert_eq!(markers[0].has_trailing_space, true);
    }
}
