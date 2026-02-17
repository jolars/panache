use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::utils::strip_newline;

/// Tries to parse a definition list marker (`:` or `~`)
///
/// Returns Some((marker_char, indent, spaces_after)) if found, None otherwise.
/// The marker can be indented 0-3 spaces and must be followed by whitespace.
pub(crate) fn try_parse_definition_marker(line: &str) -> Option<(char, usize, usize)> {
    // Count leading spaces (0-3 allowed)
    let indent = line.chars().take_while(|&c| c == ' ').count();
    if indent > 3 {
        return None;
    }

    let after_indent = &line[indent..];

    // Check for : or ~ marker
    let marker = after_indent.chars().next()?;
    if !matches!(marker, ':' | '~') {
        return None;
    }

    let after_marker = &after_indent[1..];

    // Must be followed by whitespace
    if !after_marker.starts_with(' ') && !after_marker.starts_with('\t') && !after_marker.is_empty()
    {
        return None;
    }

    let spaces_after = after_marker
        .chars()
        .take_while(|c| c.is_whitespace())
        .count();

    Some((marker, indent, spaces_after))
}

/// Emit a term line into the syntax tree
pub(crate) fn emit_term(builder: &mut GreenNodeBuilder<'static>, line: &str) {
    builder.start_node(SyntaxKind::Term.into());
    // Strip trailing newline from line (it will be emitted separately)
    let (text, newline_str) = strip_newline(line);
    builder.token(SyntaxKind::TEXT.into(), text.trim_end());
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }
    builder.finish_node(); // Term
}

/// Emit a definition marker
pub(crate) fn emit_definition_marker(
    builder: &mut GreenNodeBuilder<'static>,
    marker: char,
    indent: usize,
) {
    if indent > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &" ".repeat(indent));
    }
    builder.token(SyntaxKind::DefinitionMarker.into(), &marker.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_definition_marker_colon() {
        assert_eq!(
            try_parse_definition_marker(":   Definition"),
            Some((':', 0, 3))
        );
    }

    #[test]
    fn test_parse_definition_marker_tilde() {
        assert_eq!(
            try_parse_definition_marker("~   Definition"),
            Some(('~', 0, 3))
        );
    }

    #[test]
    fn test_parse_definition_marker_indented() {
        assert_eq!(
            try_parse_definition_marker("  : Definition"),
            Some((':', 2, 1))
        );
        assert_eq!(
            try_parse_definition_marker("   ~ Definition"),
            Some(('~', 3, 1))
        );
    }

    #[test]
    fn test_parse_definition_marker_too_indented() {
        assert_eq!(try_parse_definition_marker("    : Definition"), None);
    }

    #[test]
    fn test_parse_definition_marker_no_space_after() {
        assert_eq!(try_parse_definition_marker(":Definition"), None);
    }

    #[test]
    fn test_parse_definition_marker_at_eol() {
        assert_eq!(try_parse_definition_marker(":"), Some((':', 0, 0)));
    }
}

// Helper functions for definition list management in BlockParser

use super::container_stack::{Container, ContainerStack};

/// Check if we're in a definition list.
pub(super) fn in_definition_list(containers: &ContainerStack) -> bool {
    containers
        .stack
        .iter()
        .any(|c| matches!(c, Container::DefinitionList { .. }))
}

/// Look ahead past blank lines to find a definition marker.
/// Returns Some(blank_line_count) if found, None otherwise.
pub(super) fn next_line_is_definition_marker(lines: &[&str], pos: usize) -> Option<usize> {
    let mut check_pos = pos + 1;
    let mut blank_count = 0;
    while check_pos < lines.len() {
        let line = lines[check_pos];
        if line.trim().is_empty() {
            blank_count += 1;
            check_pos += 1;
            continue;
        }
        if try_parse_definition_marker(line).is_some() {
            return Some(blank_count);
        } else {
            return None;
        }
    }
    None
}
