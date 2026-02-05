use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

pub(crate) fn try_parse_paragraph(
    lines: &[&str],
    pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
) -> Option<usize> {
    log::debug!("Trying to parse paragraph at position {}", pos);

    if pos >= lines.len() {
        return None;
    }
    let line = lines[pos];

    if line.trim().is_empty() {
        return None;
    }

    // Start paragraph node
    builder.start_node(SyntaxKind::PARAGRAPH.into());

    let mut current_pos = pos;
    while current_pos < lines.len() {
        let line = lines[current_pos];
        if line.trim().is_empty() {
            break;
        }

        // Add line as TEXT token (could be improved to handle inline elements)
        builder.token(SyntaxKind::TEXT.into(), line);
        builder.token(SyntaxKind::NEWLINE.into(), "\n");

        current_pos += 1;

        log::debug!("Added line to paragraph: {}", line);
    }

    builder.finish_node(); // PARAGRAPH

    Some(current_pos)
}
