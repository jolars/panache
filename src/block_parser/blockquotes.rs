use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

pub(crate) fn try_identify_blockquote(children: &[SyntaxNode], start: usize) -> Option<usize> {
    if start >= children.len() {
        return None;
    }

    // Check if this paragraph looks like a blockquote (starts with >)
    let first_node = &children[start];
    if first_node.kind() != SyntaxKind::PARAGRAPH {
        return None;
    }

    let text = first_node.text().to_string();
    let first_line = text.lines().next().unwrap_or("");

    // Check if line has valid blockquote indentation (max 3 spaces before >)
    if !is_valid_blockquote_line(first_line) {
        return None;
    }

    // Find consecutive blockquote paragraphs and blank lines
    let mut end = start + 1;
    while end < children.len() {
        let node = &children[end];
        match node.kind() {
            SyntaxKind::PARAGRAPH => {
                let text = node.text().to_string();
                let first_line = text.lines().next().unwrap_or("");
                if is_valid_blockquote_line(first_line) {
                    end += 1;
                } else {
                    break;
                }
            }
            SyntaxKind::BlankLine => {
                // Blank lines can be part of blockquotes
                end += 1;
            }
            _ => break,
        }
    }

    Some(end)
}

pub(crate) fn is_valid_blockquote_line(line: &str) -> bool {
    // Check for up to 3 spaces, then >, following Pandoc spec
    if line.starts_with('>') {
        return true;
    }
    if line.starts_with(' ') && line.len() > 1 && line[1..].starts_with('>') {
        return true;
    }
    if line.starts_with("  ") && line.len() > 2 && line[2..].starts_with('>') {
        return true;
    }
    if line.starts_with("   ") && line.len() > 3 && line[3..].starts_with('>') {
        return true;
    }
    // 4 or more spaces before > is not a valid blockquote
    false
}

fn strip_blockquote_marker(line: &str) -> Option<&str> {
    // Handle up to 3 spaces before >, then extract content after >
    if let Some(stripped) = line.strip_prefix('>') {
        // Remove optional space after >
        Some(stripped.strip_prefix(' ').unwrap_or(stripped))
    } else if let Some(rest) = line.strip_prefix(' ')
        && let Some(stripped) = rest.strip_prefix('>')
    {
        Some(stripped.strip_prefix(' ').unwrap_or(stripped))
    } else if let Some(rest) = line.strip_prefix("  ")
        && let Some(stripped) = rest.strip_prefix('>')
    {
        Some(stripped.strip_prefix(' ').unwrap_or(stripped))
    } else if let Some(rest) = line.strip_prefix("   ")
        && let Some(stripped) = rest.strip_prefix('>')
    {
        Some(stripped.strip_prefix(' ').unwrap_or(stripped))
    } else {
        None
    }
}

pub(crate) fn build_blockquote_node(builder: &mut GreenNodeBuilder<'static>, nodes: &[SyntaxNode]) {
    use crate::block_parser::BlockParser;

    builder.start_node(SyntaxKind::BlockQuote.into());

    // Extract content from blockquote markers and recursively parse
    let mut content_lines = Vec::new();

    for node in nodes {
        match node.kind() {
            SyntaxKind::PARAGRAPH => {
                let text = node.text().to_string();
                for line in text.lines() {
                    if let Some(stripped) = strip_blockquote_marker(line) {
                        // Line has blockquote marker - extract content
                        content_lines.push(stripped.to_string());
                    } else {
                        // Lazy line without marker - include as-is
                        content_lines.push(line.to_string());
                    }
                }
            }
            SyntaxKind::BlankLine => {
                content_lines.push(String::new());
            }
            _ => {}
        }
    }

    if !content_lines.is_empty() {
        let content = content_lines.join("\n");
        if !content.trim().is_empty() {
            // Create a sub-parser for the blockquote content - this enables recursion!
            let sub_parser = BlockParser::new(&content);
            let sub_tree = sub_parser.parse();

            // Copy the sub-tree's document children into our blockquote
            if let Some(doc) = sub_tree
                .children()
                .find(|n| n.kind() == SyntaxKind::DOCUMENT)
            {
                for child in doc.children() {
                    copy_node_recursively(builder, &child);
                }
            }
        }
    }

    builder.finish_node();
}

fn copy_node_recursively(builder: &mut GreenNodeBuilder<'static>, node: &SyntaxNode) {
    builder.start_node(node.kind().into());

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => copy_node_recursively(builder, &n),
            rowan::NodeOrToken::Token(t) => {
                builder.token(t.kind().into(), t.text());
            }
        }
    }

    builder.finish_node();
}
