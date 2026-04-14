use crate::syntax::{SyntaxKind, SyntaxNode};

pub(super) fn contains_latex_command(node: &SyntaxNode) -> bool {
    node.descendants()
        .any(|child| child.kind() == SyntaxKind::LATEX_COMMAND)
}

pub(super) fn is_bookdown_text_reference(node: &SyntaxNode) -> bool {
    let text = node.text().to_string();
    let trimmed = text.trim_end_matches(['\r', '\n']);
    if !trimmed.starts_with("(ref:") || !trimmed.contains(") ") {
        return false;
    }
    !trimmed[trimmed.find(") ").unwrap() + 2..].contains('\n')
}
