use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::{GreenNode, GreenNodeBuilder};

mod architecture_tests;
mod bracketed_spans;
mod citations;
mod code_spans;
mod emphasis;
mod escapes;
mod future_tests;
mod inline_footnotes;
mod inline_math;
mod latex;
mod links;
mod strikeout;
mod subscript;
mod superscript;
mod tests;

use bracketed_spans::{emit_bracketed_span, try_parse_bracketed_span};
use citations::{
    emit_bare_citation, emit_bracketed_citation, try_parse_bare_citation,
    try_parse_bracketed_citation,
};
use code_spans::{emit_code_span, try_parse_code_span};
use emphasis::{emit_emphasis, try_parse_emphasis};
use escapes::{emit_escape, try_parse_escape};
use inline_footnotes::{emit_inline_footnote, try_parse_inline_footnote};
use inline_math::{
    emit_display_math, emit_inline_math, try_parse_display_math, try_parse_inline_math,
};
use latex::{parse_latex_command, try_parse_latex_command};
use links::{
    emit_autolink, emit_inline_image, emit_inline_link, try_parse_autolink, try_parse_inline_image,
    try_parse_inline_link,
};
use strikeout::{emit_strikeout, try_parse_strikeout};
use subscript::{emit_subscript, try_parse_subscript};
use superscript::{emit_superscript, try_parse_superscript};

/// Parse inline elements from text content.
/// This is a standalone function used by both the main inline parser
/// and by nested contexts like link text.
pub fn parse_inline_text(builder: &mut GreenNodeBuilder, text: &str) {
    log::trace!(
        "Parsing inline text: {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len()
    );
    let mut pos = 0;
    let bytes = text.as_bytes();

    while pos < text.len() {
        // Try to parse backslash escape FIRST (highest precedence)
        // This prevents escaped delimiters from being parsed
        if bytes[pos] == b'\\'
            && let Some((len, ch, escape_type)) = try_parse_escape(&text[pos..])
        {
            log::debug!("Matched escape at pos {}: \\{}", pos, ch);
            emit_escape(builder, ch, escape_type);
            pos += len;
            continue;
        }

        // Try to parse LaTeX command (after escapes, before code)
        // This handles \cite{ref}, \textbf{text}, etc.
        if bytes[pos] == b'\\'
            && let Some(len) = try_parse_latex_command(&text[pos..])
        {
            log::debug!("Matched LaTeX command at pos {}", pos);
            parse_latex_command(builder, &text[pos..], len);
            pos += len;
            continue;
        }

        // Try to parse code span
        if bytes[pos] == b'`'
            && let Some((len, content, backtick_count, attributes)) =
                try_parse_code_span(&text[pos..])
        {
            log::debug!(
                "Matched code span at pos {}: {} backticks",
                pos,
                backtick_count
            );
            emit_code_span(builder, content, backtick_count, attributes);
            pos += len;
            continue;
        }

        // Try to parse inline footnote (^[...])
        if bytes[pos] == b'^'
            && pos + 1 < text.len()
            && bytes[pos + 1] == b'['
            && let Some((len, content)) = try_parse_inline_footnote(&text[pos..])
        {
            log::debug!("Matched inline footnote at pos {}", pos);
            emit_inline_footnote(builder, content);
            pos += len;
            continue;
        }

        // Try to parse superscript (^text^)
        // Must come after inline footnote check to avoid conflict with ^[
        if bytes[pos] == b'^'
            && let Some((len, content)) = try_parse_superscript(&text[pos..])
        {
            log::debug!("Matched superscript at pos {}", pos);
            emit_superscript(builder, content);
            pos += len;
            continue;
        }

        // Try to parse subscript (~text~)
        // Must come before strikeout check to avoid conflict with ~~
        if bytes[pos] == b'~'
            && let Some((len, content)) = try_parse_subscript(&text[pos..])
        {
            log::debug!("Matched subscript at pos {}", pos);
            emit_subscript(builder, content);
            pos += len;
            continue;
        }

        // Try to parse strikeout (~~text~~)
        if bytes[pos] == b'~'
            && pos + 1 < text.len()
            && bytes[pos + 1] == b'~'
            && let Some((len, content)) = try_parse_strikeout(&text[pos..])
        {
            log::debug!("Matched strikeout at pos {}", pos);
            emit_strikeout(builder, content);
            pos += len;
            continue;
        }

        // Try to parse inline math (must check for $$ first for display math)
        if bytes[pos] == b'$' {
            // Try display math first ($$...$$)
            if let Some((len, content)) = try_parse_display_math(&text[pos..]) {
                let dollar_count = text[pos..].chars().take_while(|&c| c == '$').count();
                log::debug!(
                    "Matched display math at pos {}: {} dollars",
                    pos,
                    dollar_count
                );
                emit_display_math(builder, content, dollar_count);
                pos += len;
                continue;
            }

            // Try inline math ($...$)
            if let Some((len, content)) = try_parse_inline_math(&text[pos..]) {
                log::debug!("Matched inline math at pos {}", pos);
                emit_inline_math(builder, content);
                pos += len;
                continue;
            }
        }

        // Try to parse automatic link
        if bytes[pos] == b'<'
            && let Some((len, url)) = try_parse_autolink(&text[pos..])
        {
            log::debug!("Matched autolink at pos {}: {}", pos, url);
            emit_autolink(builder, &text[pos..pos + len], url);
            pos += len;
            continue;
        }

        // Try to parse inline image (must come before inline link since it starts with ![)
        if pos + 1 < text.len()
            && bytes[pos] == b'!'
            && bytes[pos + 1] == b'['
            && let Some((len, alt_text, dest, attributes)) = try_parse_inline_image(&text[pos..])
        {
            log::debug!("Matched inline image at pos {}: dest={}", pos, dest);
            emit_inline_image(builder, &text[pos..pos + len], alt_text, dest, attributes);
            pos += len;
            continue;
        }

        // Try to parse inline link
        if bytes[pos] == b'['
            && let Some((len, link_text, dest)) = try_parse_inline_link(&text[pos..])
        {
            log::debug!("Matched inline link at pos {}: dest={}", pos, dest);
            emit_inline_link(builder, &text[pos..pos + len], link_text, dest);
            pos += len;
            continue;
        }

        // Try to parse bracketed citation (after link since both start with [)
        if bytes[pos] == b'['
            && let Some((len, content)) = try_parse_bracketed_citation(&text[pos..])
        {
            log::debug!("Matched bracketed citation at pos {}", pos);
            emit_bracketed_citation(builder, content);
            pos += len;
            continue;
        }

        // Try to parse bracketed span (after link and citation since all start with [)
        if bytes[pos] == b'['
            && let Some((len, content, attributes)) = try_parse_bracketed_span(&text[pos..])
        {
            log::debug!(
                "Matched bracketed span at pos {}: attrs={}",
                pos,
                attributes
            );
            emit_bracketed_span(builder, &content, &attributes);
            pos += len;
            continue;
        }

        // Try to parse bare citation (author-in-text)
        if (bytes[pos] == b'@'
            || (bytes[pos] == b'-' && pos + 1 < text.len() && bytes[pos + 1] == b'@'))
            && let Some((len, key, has_suppress)) = try_parse_bare_citation(&text[pos..])
        {
            log::debug!("Matched bare citation at pos {}: key={}", pos, key);
            emit_bare_citation(builder, key, has_suppress);
            pos += len;
            continue;
        }

        // Try to parse emphasis
        if (bytes[pos] == b'*' || bytes[pos] == b'_')
            && let Some((len, inner_text, level, delim_char)) = try_parse_emphasis(&text[pos..])
        {
            log::debug!(
                "Matched emphasis at pos {}: level={}, delim={}",
                pos,
                level,
                delim_char
            );
            emit_emphasis(builder, inner_text, level, delim_char);
            pos += len;
            continue;
        }

        // No inline element matched - emit as plain text
        let next_pos = find_next_inline_start(&text[pos..]);
        let text_chunk = if next_pos > 0 {
            &text[pos..pos + next_pos]
        } else {
            &text[pos..]
        };

        if !text_chunk.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), text_chunk);
        }

        if next_pos > 0 {
            pos += next_pos;
        } else {
            break;
        }
    }
}

/// Find the next position where an inline element might start.
/// Returns the number of bytes to skip, or 0 if at end.
fn find_next_inline_start(text: &str) -> usize {
    for (i, ch) in text.char_indices() {
        match ch {
            '\\' | '`' | '*' | '_' | '[' | '!' | '<' | '$' | '^' | '~' | '@' => return i.max(1),
            '-' => {
                // Check if this might be a suppress-author citation -@
                if i + 1 < text.len() && text.as_bytes()[i + 1] == b'@' {
                    return i.max(1);
                }
            }
            _ => {}
        }
    }
    text.len()
}

/// The InlineParser takes a block-level CST and processes inline elements within text content.
/// It traverses the tree, finds TEXT tokens that need inline parsing, and replaces them
/// with properly parsed inline elements (emphasis, links, math, etc.).
pub struct InlineParser {
    root: SyntaxNode,
    #[allow(dead_code)] // TODO: Will be used for extension configuration
    config: Config,
}

impl InlineParser {
    pub fn new(root: SyntaxNode, config: Config) -> Self {
        Self { root, config }
    }

    /// Parse inline elements within the block-level CST.
    /// Traverses the tree and replaces TEXT tokens with parsed inline elements.
    pub fn parse(self) -> SyntaxNode {
        let green = self.parse_node(&self.root);
        SyntaxNode::new_root(green)
    }

    /// Recursively parse a node, replacing TEXT tokens with inline elements.
    fn parse_node(&self, node: &SyntaxNode) -> GreenNode {
        let mut builder = GreenNodeBuilder::new();
        self.copy_node_to_builder(&mut builder, node);
        builder.finish()
    }

    /// Copy a node and its children to the builder, recursively parsing inline elements.
    fn copy_node_to_builder(&self, builder: &mut GreenNodeBuilder, node: &SyntaxNode) {
        builder.start_node(node.kind().into());

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Node(n) => {
                    self.copy_node_to_builder(builder, &n);
                }
                rowan::NodeOrToken::Token(t) => {
                    if self.should_parse_inline(&t) {
                        parse_inline_text(builder, t.text());
                    } else {
                        builder.token(t.kind().into(), t.text());
                    }
                }
            }
        }

        builder.finish_node();
    }

    /// Check if a token should be parsed for inline elements.
    /// Per spec: "Backslash escapes do not work in verbatim contexts"
    fn should_parse_inline(&self, token: &SyntaxToken) -> bool {
        if token.kind() != SyntaxKind::TEXT {
            return false;
        }

        // Check if we're in a verbatim context (code block, math block, LaTeX environment, HTML block)
        if let Some(parent) = token.parent() {
            match parent.kind() {
                SyntaxKind::CodeBlock
                | SyntaxKind::MathBlock
                | SyntaxKind::CodeContent
                | SyntaxKind::LatexEnvironment
                | SyntaxKind::LatexEnvBegin
                | SyntaxKind::LatexEnvEnd
                | SyntaxKind::LatexEnvContent
                | SyntaxKind::HtmlBlock
                | SyntaxKind::HtmlBlockTag
                | SyntaxKind::HtmlBlockContent => {
                    return false;
                }
                _ => {}
            }
        }

        true
    }
}

#[cfg(test)]
mod inline_tests {
    use super::*;
    use crate::block_parser::BlockParser;
    use crate::config::Config;

    fn find_nodes_by_kind(node: &SyntaxNode, kind: SyntaxKind) -> Vec<String> {
        let mut results = Vec::new();
        for child in node.descendants() {
            if child.kind() == kind {
                results.push(child.to_string());
            }
        }
        results
    }

    fn parse_inline(input: &str) -> SyntaxNode {
        let config = Config::default();
        let block_tree = BlockParser::new(input, &config).parse();
        InlineParser::new(block_tree, config).parse()
    }

    #[test]
    fn test_inline_parser_preserves_text() {
        let input = "This is plain text.";
        let inline_tree = parse_inline(input);

        // Should preserve the text unchanged for now
        let text = inline_tree.to_string();
        assert!(text.contains("This is plain text."));
    }

    #[test]
    fn test_parse_autolink() {
        let input = "Visit <https://example.com> for more.";
        let inline_tree = parse_inline(input);

        let autolinks = find_nodes_by_kind(&inline_tree, SyntaxKind::AutoLink);
        assert_eq!(autolinks.len(), 1);
        assert_eq!(autolinks[0], "<https://example.com>");
    }

    #[test]
    fn test_parse_email_autolink() {
        let input = "Email me at <user@example.com>.";
        let inline_tree = parse_inline(input);

        let autolinks = find_nodes_by_kind(&inline_tree, SyntaxKind::AutoLink);
        assert_eq!(autolinks.len(), 1);
        assert_eq!(autolinks[0], "<user@example.com>");
    }

    #[test]
    fn test_parse_inline_link() {
        let input = "Click [here](https://example.com) to continue.";
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::Link);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], "[here](https://example.com)");
    }

    #[test]
    fn test_parse_link_with_title() {
        let input = r#"See [this](url "title") link."#;
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::Link);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], r#"[this](url "title")"#);
    }

    #[test]
    fn test_multiple_links() {
        let input = "Visit [site1](url1) and [site2](url2).";
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::Link);
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn test_parse_inline_image() {
        let input = "Here is an image: ![alt text](image.jpg).";
        let inline_tree = parse_inline(input);

        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::ImageLink);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0], "![alt text](image.jpg)");
    }

    #[test]
    fn test_parse_image_with_title() {
        let input = r#"See ![photo](pic.jpg "My Photo") here."#;
        let inline_tree = parse_inline(input);

        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::ImageLink);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0], r#"![photo](pic.jpg "My Photo")"#);
    }

    #[test]
    fn test_multiple_images() {
        let input = "![img1](a.jpg) and ![img2](b.jpg)";
        let inline_tree = parse_inline(input);

        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::ImageLink);
        assert_eq!(images.len(), 2);
    }

    #[test]
    fn test_link_and_image_together() {
        let input = "A [link](url) and an ![image](pic.jpg).";
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::Link);
        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::ImageLink);
        assert_eq!(links.len(), 1);
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn test_parse_bare_citation() {
        let input = "As @doe99 notes, the result is clear.";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "@doe99");
    }

    #[test]
    fn test_parse_bracketed_citation() {
        let input = "This is a fact [@doe99].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[@doe99]");
    }

    #[test]
    fn test_parse_multiple_citations() {
        let input = "Multiple sources [@doe99; @smith2000; @jones2010].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[@doe99; @smith2000; @jones2010]");
    }

    #[test]
    fn test_parse_citation_with_locator() {
        let input = "See the discussion [@doe99, pp. 33-35].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[@doe99, pp. 33-35]");
    }

    #[test]
    fn test_parse_suppress_author_citation() {
        let input = "Smith says blah [-@smith04].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[-@smith04]");
    }

    #[test]
    fn test_parse_bare_suppress_citation() {
        let input = "See -@doe99 for details.";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "-@doe99");
    }

    #[test]
    fn test_citation_not_conflicting_with_email() {
        // Email in autolink should not be parsed as citation
        let input = "Email <user@example.com> for info.";
        let inline_tree = parse_inline(input);

        let autolinks = find_nodes_by_kind(&inline_tree, SyntaxKind::AutoLink);
        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(autolinks.len(), 1);
        assert_eq!(citations.len(), 0);
    }
}
