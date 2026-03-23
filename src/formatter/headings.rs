use crate::syntax::{SyntaxKind, SyntaxNode};

pub(super) fn format_heading(node: &SyntaxNode) -> String {
    let mut level = 1;
    let mut content = String::new();
    let mut saw_content = false;
    let mut attributes = String::new();

    for child in node.children() {
        match child.kind() {
            SyntaxKind::ATX_HEADING_MARKER => {
                let t = child.text().to_string();
                level = t.chars().take_while(|&c| c == '#').count().clamp(1, 6);
            }
            SyntaxKind::SETEXT_HEADING_UNDERLINE => {
                let t = child.text().to_string();
                let first_char = t.trim().chars().next().unwrap_or('=');
                if first_char == '=' {
                    level = 1;
                } else {
                    level = 2;
                }
            }
            SyntaxKind::HEADING_CONTENT => {
                let mut t = child.text().to_string();
                t = t.trim_end().to_string();
                let trimmed_hash = t.trim_end_matches('#').to_string();
                if trimmed_hash.len() != t.len() {
                    t = trimmed_hash.trim_end().to_string();
                }
                content = t.trim().to_string();
                saw_content = true;
            }
            SyntaxKind::ATTRIBUTE => {
                attributes = child.text().to_string();
            }
            _ => {}
        }
    }
    if !saw_content {
        content = node.text().to_string();
    }

    let mut out = format!("{} {}", "#".repeat(level), content);
    if !attributes.is_empty() {
        out.push(' ');
        out.push_str(attributes.trim());
    }
    out
}
