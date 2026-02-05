use crate::block_parser::utils::{get_fence_count, strip_leading_spaces};
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

pub(crate) fn try_parse_fenced_code_block(
    lines: &[&str],
    pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
    has_blank_line_before: bool,
) -> Option<usize> {
    log::debug!("Trying to parse fenced code block at position {}", pos);

    if pos >= lines.len() {
        return None;
    }

    let line = lines[pos];
    let trimmed = strip_leading_spaces(line);

    // Check if this is a fenced code block opening
    let (fence_char, fence_count) = if let Some(count) = get_fence_count(trimmed, '`') {
        ('`', count)
    } else if let Some(count) = get_fence_count(trimmed, '~') {
        ('~', count)
    } else {
        return None;
    };

    // Must have at least 3 fence characters
    if fence_count < 3 {
        return None;
    }

    // blank_before_header: require blank line before, unless at BOF
    if !has_blank_line_before {
        return None;
    }

    // Extract info string (language, attributes, etc.)
    let info_string = trimmed[fence_count..].trim();

    // Start code block
    builder.start_node(SyntaxKind::CodeBlock.into());

    // Opening fence
    builder.start_node(SyntaxKind::CodeFenceOpen.into());
    builder.token(SyntaxKind::CodeFenceMarker.into(), &trimmed[..fence_count]);
    if !info_string.is_empty() {
        builder.token(SyntaxKind::CodeInfo.into(), info_string);
    }
    builder.finish_node(); // CodeFenceOpen

    let mut current_pos = pos + 1;

    // Collect content lines until we find a closing fence
    let mut content_lines = Vec::new();
    let mut found_closing = false;

    while current_pos < lines.len() {
        let line = lines[current_pos];
        let trimmed_line = strip_leading_spaces(line);

        // Check if this is a valid closing fence
        if let Some(closing_count) = get_fence_count(trimmed_line, fence_char)
            && closing_count >= fence_count
        {
            // Make sure the rest of the line is empty (or just whitespace)
            let after_fence = trimmed_line[closing_count..].trim();
            if after_fence.is_empty() {
                found_closing = true;
                break;
            }
        }

        content_lines.push(line);
        current_pos += 1;
    }

    // Add content
    if !content_lines.is_empty() {
        builder.start_node(SyntaxKind::CodeContent.into());
        for (i, content_line) in content_lines.iter().enumerate() {
            if i > 0 {
                builder.token(SyntaxKind::NEWLINE.into(), "\n");
            }
            builder.token(SyntaxKind::TEXT.into(), content_line);
        }
        builder.finish_node(); // CodeContent
    }

    // Closing fence (if found)
    if found_closing {
        let closing_line = lines[current_pos];
        let closing_trimmed = strip_leading_spaces(closing_line);
        let closing_count = get_fence_count(closing_trimmed, fence_char).unwrap();

        builder.start_node(SyntaxKind::CodeFenceClose.into());
        builder.token(
            SyntaxKind::CodeFenceMarker.into(),
            &closing_trimmed[..closing_count],
        );
        builder.finish_node(); // CodeFenceClose

        current_pos += 1;
    }

    builder.finish_node(); // CodeBlock

    log::debug!("Parsed fenced code block, found_closing: {}", found_closing);
    Some(current_pos)
}
