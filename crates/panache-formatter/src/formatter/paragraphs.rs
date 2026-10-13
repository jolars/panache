use crate::syntax::{
    AstNode, HugoShortcode, InlineExecutable, LinkDestUrl, SyntaxKind, SyntaxNode,
};

/// Container indentation is separate from whitespace inside opaque content.
/// Layout already removes `LINE_PREFIX` tokens, so trimming continuation
/// lines can change string values, language syntax, or template input.
pub(super) fn trim_prose_indent<'a>(node: &SyntaxNode, line: &'a str) -> &'a str {
    if line.starts_with(char::is_whitespace)
        && (node
            .descendants()
            .filter_map(InlineExecutable::cast)
            .any(|inline| inline.code_source().contains('\n'))
            || node
                .descendants()
                .filter_map(HugoShortcode::cast)
                .any(|shortcode| shortcode.syntax().text().to_string().contains('\n'))
            || node
                .descendants()
                .filter_map(LinkDestUrl::cast)
                .any(|url| url.syntax().text().to_string().contains('\n')))
    {
        line
    } else {
        line.trim_start()
    }
}

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
