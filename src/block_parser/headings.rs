// Old heading parser - kept for reference but no longer used.
// Inline heading parsing is now in block_parser.rs.
#![allow(dead_code)]

use crate::block_parser::utils::strip_leading_spaces;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

pub(crate) fn try_parse_atx_heading(
    lines: &[&str],
    pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
    has_blank_line_before: bool,
) -> Option<usize> {
    log::debug!("Trying to parse ATX heading at position {}", pos);

    if pos >= lines.len() {
        return None;
    }
    let line = lines[pos];

    // Allow up to 3 leading spaces
    let trimmed = strip_leading_spaces(line);

    // Must start with 1-6 '#'s
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }

    // Must be followed by a space (Pandoc: space_in_atx_header)
    let after_hashes = &trimmed[hashes..];
    if !after_hashes.starts_with(' ') {
        return None;
    }

    // blank_before_header: require blank line before, unless at BOF
    if !has_blank_line_before {
        return None;
    }

    // The rest after hashes is the content (may have trailing hashes)
    let mut content = after_hashes.trim_start();
    // Remove optional trailing hashes and spaces
    if let Some(idx) = content.rfind(|c| c != '#' && c != ' ') {
        content = content[..=idx].trim_end();
    } else {
        content = "";
    }

    // Emit nodes
    builder.start_node(SyntaxKind::Heading.into());

    // Marker node for the hashes
    builder.start_node(SyntaxKind::AtxHeadingMarker.into());
    builder.token(SyntaxKind::AtxHeadingMarker.into(), &trimmed[..hashes]);
    builder.finish_node();

    // Heading content node
    builder.start_node(SyntaxKind::HeadingContent.into());
    builder.token(SyntaxKind::TEXT.into(), content);
    builder.finish_node();

    builder.finish_node(); // Heading

    Some(pos + 1)
}
