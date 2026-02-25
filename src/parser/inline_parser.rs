//! Inline element parsing for panache.
//!
//! This module handles parsing of inline elements within block-level content.
//! Uses a recursive approach

use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::{GreenNode, GreenNodeBuilder};

mod architecture_tests;
mod bracketed_spans;
mod citations;
mod code_spans;
pub mod core; // Public for use in block_parser inline_emission
mod escapes;
mod inline_footnotes;
mod latex;
pub mod links; // Public for try_parse_inline_image used by block parser
mod math;
mod native_spans;
mod raw_inline;
mod shortcodes;
mod strikeout;
mod subscript;
mod superscript;
mod tests;

/// Parse inline elements from concatenated text that may include newlines.
/// This function handles multi-line inline patterns (like display math) by checking for them first,
/// then emits NEWLINE tokens to preserve losslessness for the remaining text.
/// Used when parsing paragraphs and other blocks that concatenate TEXT/NEWLINE tokens.
pub fn parse_inline_text_with_newlines(
    builder: &mut GreenNodeBuilder,
    text: &str,
    config: &Config,
    _allow_reference_links: bool,
) {
    log::trace!(
        "Parsing inline text with newlines: {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len(),
    );

    // Use the recursive parser which handles newlines as part of the inline content stream
    // It will emit NEWLINE tokens to preserve losslessness, and properly handles multi-line
    // constructs like emphasis, links, and display math
    core::parse_inline_text_recursive(builder, text, config);
}

/// Parse inline elements from text content.
/// This is a standalone function used by both the main inline parser
/// and by nested contexts like link text.
///
/// The `allow_reference_links` parameter controls whether reference links/images should be parsed.
/// Set to `false` in nested contexts (inside link text, image alt, spans) to prevent recursive parsing.
///
/// **IMPLEMENTATION NOTE**: This now uses the two-phase parsing architecture which correctly
/// handles emphasis with nested inline elements (code, math, links, etc.).
pub fn parse_inline_text(
    builder: &mut GreenNodeBuilder,
    text: &str,
    config: &Config,
    _allow_reference_links: bool,
) {
    log::trace!(
        "Parsing inline text (recursive): {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len()
    );

    // Use recursive parsing with Pandoc's algorithm for emphasis
    core::parse_inline_text_recursive(builder, text, config);
}

/// The InlineParser takes a block-level CST and processes inline elements within text content.
/// It traverses the tree, finds TEXT tokens that need inline parsing, and replaces them
/// with properly parsed inline elements (emphasis, links, math, etc.).
pub struct InlineParser {
    root: SyntaxNode,
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
        log::trace!("copy_node_to_builder: {:?}", node.kind());
        builder.start_node(node.kind().into());

        // For nodes that contain inline content (like paragraphs), concatenate TEXT/NEWLINE tokens
        // into a single string and parse it. This handles multi-line patterns like display math
        // while being much simpler than the TokenStream approach.
        //
        // IMPORTANT: We must NOT concatenate structural tokens like BLOCKQUOTE_MARKER.
        // These need to be emitted directly to preserve the CST structure and prevent
        // them from being treated as inline text content.
        if self.should_concatenate_for_parsing(node) {
            let mut concatenated = String::new();
            let mut children = node.children_with_tokens().peekable();

            while let Some(child) = children.next() {
                if let Some(token) = child.into_token() {
                    // Skip structural tokens - emit them directly without inline parsing
                    if self.is_structural_token(token.kind()) {
                        // First, parse any accumulated text before emitting the structural token
                        if !concatenated.is_empty() {
                            parse_inline_text_with_newlines(
                                builder,
                                &concatenated,
                                &self.config,
                                true,
                            );
                            concatenated.clear();
                        }
                        // Emit the structural token as-is
                        builder.token(token.kind().into(), token.text());

                        // Check if the next token is WHITESPACE that belongs to the structural marker
                        // (e.g., the space after ">"). If so, emit it too.
                        if let Some(rowan::NodeOrToken::Token(next)) = children.peek()
                            && next.kind() == SyntaxKind::WHITESPACE
                        {
                            let ws_token = children.next().unwrap().into_token().unwrap();
                            builder.token(SyntaxKind::WHITESPACE.into(), ws_token.text());
                        }
                    } else {
                        // Concatenate TEXT and NEWLINE tokens for inline parsing
                        concatenated.push_str(token.text());
                    }
                }
            }

            // Parse any remaining concatenated text
            if !concatenated.is_empty() {
                parse_inline_text_with_newlines(builder, &concatenated, &self.config, true);
            }
        } else {
            // For other nodes, recursively process children as before
            // BUT: Skip nodes that already have inline structure from integrated parsing
            if self.should_skip_already_parsed(node) {
                // Node already has inline structure - just copy it verbatim without recursion
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => {
                            // Copy entire subtree without parsing
                            self.copy_subtree_verbatim(builder, &n);
                        }
                        rowan::NodeOrToken::Token(t) => {
                            builder.token(t.kind().into(), t.text());
                        }
                    }
                }
            } else {
                // Process children normally
                let mut children = node.children_with_tokens().peekable();
                while let Some(child) = children.next() {
                    match child {
                        rowan::NodeOrToken::Node(n) => {
                            self.copy_node_to_builder(builder, &n);
                        }
                        rowan::NodeOrToken::Token(t) => {
                            // Check for hard line breaks: two or more spaces at end of line, or backslash at end of line
                            // Only in non-verbatim contexts
                            if t.kind() == SyntaxKind::TEXT
                                && self.should_parse_inline(&t)
                                && let Some(rowan::NodeOrToken::Token(next)) = children.peek()
                                && next.kind() == SyntaxKind::NEWLINE
                            {
                                let text = t.text();

                                // Check for backslash-newline hard line break (requires escaped_line_breaks extension)
                                if self.config.extensions.escaped_line_breaks
                                    && text.ends_with('\\')
                                {
                                    // Emit the text before the backslash
                                    let text_before = &text[..text.len() - 1];
                                    if !text_before.is_empty() {
                                        self.parse_text_with_refs(builder, text_before);
                                    }
                                    // Emit hard line break - preserve the backslash for losslessness
                                    builder.token(SyntaxKind::HARD_LINE_BREAK.into(), "\\\n");
                                    // Skip the NEWLINE token
                                    children.next();
                                    continue;
                                }

                                // Check for two-or-more-spaces hard line break (always enabled in Pandoc)
                                let trailing_spaces =
                                    text.chars().rev().take_while(|&c| c == ' ').count();
                                if trailing_spaces >= 2 {
                                    // Emit the text before the trailing spaces
                                    let text_before = &text[..text.len() - trailing_spaces];
                                    if !text_before.is_empty() {
                                        self.parse_text_with_refs(builder, text_before);
                                    }
                                    // Emit hard line break - preserve the trailing spaces for losslessness
                                    let spaces = " ".repeat(trailing_spaces);
                                    builder.token(
                                        SyntaxKind::HARD_LINE_BREAK.into(),
                                        &format!("{}\n", spaces),
                                    );
                                    // Skip the NEWLINE token
                                    children.next();
                                    continue;
                                }
                            }

                            // Normal token processing
                            if self.should_parse_inline(&t) {
                                // Special handling for REFERENCE_DEFINITION: parse label as LINK
                                if let Some(parent) = t.parent()
                                    && parent.kind() == SyntaxKind::REFERENCE_DEFINITION
                                {
                                    self.parse_reference_definition_label(builder, t.text());
                                } else {
                                    // Parse inline text, passing registry for reference resolution
                                    self.parse_text_with_refs(builder, t.text());
                                }
                            } else {
                                builder.token(t.kind().into(), t.text());
                            }
                        }
                    }
                }
            }
        }

        builder.finish_node();
    }

    /// Check if a node should be skipped because it already has inline structure
    /// from integrated parsing (when use_integrated_inline_parsing=true).
    fn should_skip_already_parsed(&self, node: &SyntaxNode) -> bool {
        if !self.config.parser.use_integrated_inline_parsing {
            false
        } else {
            // Skip nodes that already have inline elements emitted during block parsing
            matches!(
                node.kind(),
                SyntaxKind::HEADING_CONTENT
                    | SyntaxKind::TABLE_CAPTION
                    | SyntaxKind::TERM
                    | SyntaxKind::LINE_BLOCK_LINE
                    | SyntaxKind::PLAIN
                    | SyntaxKind::PARAGRAPH
            )
        }
    }

    /// Copy a node and all its descendants verbatim, without any parsing or modification.
    /// Used for nodes that already have inline structure from integrated parsing.
    #[allow(clippy::only_used_in_recursion)]
    fn copy_subtree_verbatim(&self, builder: &mut GreenNodeBuilder, node: &SyntaxNode) {
        builder.start_node(node.kind().into());
        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Node(n) => {
                    self.copy_subtree_verbatim(builder, &n);
                }
                rowan::NodeOrToken::Token(t) => {
                    builder.token(t.kind().into(), t.text());
                }
            }
        }
        builder.finish_node();
    }

    /// Check if a node should concatenate tokens for parsing.
    /// We always concatenate for paragraphs and plain text blocks to properly handle
    /// multi-line inline constructs (emphasis, links, display math).
    ///
    /// EXCEPT: When using integrated inline parsing, PLAIN and PARAGRAPH blocks already have inline
    /// structure emitted during block parsing, so they should skip this step.
    fn should_concatenate_for_parsing(&self, node: &SyntaxNode) -> bool {
        match node.kind() {
            SyntaxKind::PARAGRAPH => {
                // Skip concatenation if PARAGRAPH already has inline structure from integrated parsing
                !self.config.parser.use_integrated_inline_parsing
            }
            SyntaxKind::PLAIN => {
                // Skip concatenation if PLAIN already has inline structure from integrated parsing
                !self.config.parser.use_integrated_inline_parsing
            }
            _ => false,
        }
    }

    /// Check if a token is a structural marker that should NOT be concatenated for inline parsing.
    /// Structural tokens must be emitted directly to preserve CST structure.
    fn is_structural_token(&self, kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::BLOCKQUOTE_MARKER | SyntaxKind::LINE_BLOCK_MARKER
        )
    }

    /// Parse inline text with reference link/image resolution support.
    fn parse_text_with_refs(&self, builder: &mut GreenNodeBuilder, text: &str) {
        parse_inline_text(builder, text, &self.config, true);
    }

    /// Parse reference definition label as a LINK node.
    /// Input: "[label]: url..." → Output: LINK(LINK_START, LINK_TEXT, "]") + TEXT(": url...")
    fn parse_reference_definition_label(&self, builder: &mut GreenNodeBuilder, text: &str) {
        // Parse the label part: [label]:
        if !text.starts_with('[') {
            // Fallback to normal parsing if doesn't start with [
            self.parse_text_with_refs(builder, text);
            return;
        }

        // Find the closing ]
        let rest = &text[1..];
        if let Some(close_pos) = rest.find(']') {
            let label = &rest[..close_pos];
            let after_bracket = &rest[close_pos + 1..];

            // Must be followed by : for reference definition
            if after_bracket.starts_with(':') {
                // Emit LINK node with the label
                builder.start_node(SyntaxKind::LINK.into());

                // LINK_START
                builder.start_node(SyntaxKind::LINK_START.into());
                builder.token(SyntaxKind::LINK_START.into(), "[");
                builder.finish_node();

                // LINK_TEXT
                builder.start_node(SyntaxKind::LINK_TEXT.into());
                builder.token(SyntaxKind::TEXT.into(), label);
                builder.finish_node();

                // Closing bracket (as TEXT, following old behavior)
                builder.token(SyntaxKind::TEXT.into(), "]");

                builder.finish_node(); // LINK

                // Rest of the line (": url...")
                builder.token(SyntaxKind::TEXT.into(), after_bracket);
                return;
            }
        }

        // Fallback: not a valid reference definition format
        self.parse_text_with_refs(builder, text);
    }

    /// Check if a token should be parsed for inline elements.
    /// Per spec: "Backslash escapes do not work in verbatim contexts"
    fn should_parse_inline(&self, token: &SyntaxToken) -> bool {
        if token.kind() != SyntaxKind::TEXT {
            return false;
        }

        // Check if we're in a verbatim context (code block, LaTeX environment, HTML block)
        // or line block (where inline parsing is handled differently - preserves line structure)
        if let Some(parent) = token.parent() {
            match parent.kind() {
                SyntaxKind::CODE_BLOCK
                | SyntaxKind::CODE_FENCE_OPEN
                | SyntaxKind::CODE_INFO
                | SyntaxKind::CODE_CONTENT
                | SyntaxKind::LATEX_ENVIRONMENT
                | SyntaxKind::LATEX_ENV_BEGIN
                | SyntaxKind::LATEX_ENV_END
                | SyntaxKind::LATEX_ENV_CONTENT
                | SyntaxKind::HTML_BLOCK
                | SyntaxKind::HTML_BLOCK_TAG
                | SyntaxKind::HTML_BLOCK_CONTENT
                | SyntaxKind::LINE_BLOCK_LINE => {
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
    use crate::config::{Config, Flavor};
    use crate::parser::block_parser::BlockParser;

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
        let config = Config {
            flavor: Flavor::Pandoc,
            ..Config::default()
        };
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

        let autolinks = find_nodes_by_kind(&inline_tree, SyntaxKind::AUTO_LINK);
        assert_eq!(autolinks.len(), 1);
        assert_eq!(autolinks[0], "<https://example.com>");
    }

    #[test]
    fn test_parse_email_autolink() {
        let input = "Email me at <user@example.com>.";
        let inline_tree = parse_inline(input);

        let autolinks = find_nodes_by_kind(&inline_tree, SyntaxKind::AUTO_LINK);
        assert_eq!(autolinks.len(), 1);
        assert_eq!(autolinks[0], "<user@example.com>");
    }

    #[test]
    fn test_parse_inline_link() {
        let input = "Click [here](https://example.com) to continue.";
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::LINK);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], "[here](https://example.com)");
    }

    #[test]
    fn test_parse_link_with_title() {
        let input = r#"See [this](url "title") link."#;
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::LINK);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], r#"[this](url "title")"#);
    }

    #[test]
    fn test_multiple_links() {
        let input = "Visit [site1](url1) and [site2](url2).";
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::LINK);
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn test_parse_inline_image() {
        let input = "Here is an image: ![alt text](image.jpg).";
        let inline_tree = parse_inline(input);

        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::IMAGE_LINK);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0], "![alt text](image.jpg)");
    }

    #[test]
    fn test_parse_image_with_title() {
        let input = r#"See ![photo](pic.jpg "My Photo") here."#;
        let inline_tree = parse_inline(input);

        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::IMAGE_LINK);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0], r#"![photo](pic.jpg "My Photo")"#);
    }

    #[test]
    fn test_multiple_images() {
        let input = "![img1](a.jpg) and ![img2](b.jpg)";
        let inline_tree = parse_inline(input);

        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::IMAGE_LINK);
        assert_eq!(images.len(), 2);
    }

    #[test]
    fn test_link_and_image_together() {
        let input = "A [link](url) and an ![image](pic.jpg).";
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::LINK);
        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::IMAGE_LINK);
        assert_eq!(links.len(), 1);
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn test_parse_bare_citation() {
        let input = "As @doe99 notes, the result is clear.";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::CITATION);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "@doe99");
    }

    #[test]
    fn test_parse_bracketed_citation() {
        let input = "This is a fact [@doe99].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::CITATION);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[@doe99]");
    }

    #[test]
    fn test_parse_multiple_citations() {
        let input = "Multiple sources [@doe99; @smith2000; @jones2010].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::CITATION);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[@doe99; @smith2000; @jones2010]");
    }

    #[test]
    fn test_parse_citation_with_locator() {
        let input = "See the discussion [@doe99, pp. 33-35].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::CITATION);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[@doe99, pp. 33-35]");
    }

    #[test]
    fn test_parse_suppress_author_citation() {
        let input = "Smith says blah [-@smith04].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::CITATION);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[-@smith04]");
    }

    #[test]
    fn test_parse_bare_suppress_citation() {
        let input = "See -@doe99 for details.";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::CITATION);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "-@doe99");
    }

    #[test]
    fn test_citation_not_conflicting_with_email() {
        // Email in autolink should not be parsed as citation
        let input = "Email <user@example.com> for info.";
        let inline_tree = parse_inline(input);

        let autolinks = find_nodes_by_kind(&inline_tree, SyntaxKind::AUTO_LINK);
        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::CITATION);
        assert_eq!(autolinks.len(), 1);
        assert_eq!(citations.len(), 0);
    }

    #[test]
    fn test_parse_native_span_simple() {
        let input = "Text with <span>highlighted</span> content.";
        let inline_tree = parse_inline(input);

        let spans = find_nodes_by_kind(&inline_tree, SyntaxKind::BRACKETED_SPAN);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].contains("highlighted"));
    }

    #[test]
    fn test_parse_native_span_with_class() {
        let input = r#"Use <span class="important">this</span> wisely."#;
        let inline_tree = parse_inline(input);

        let spans = find_nodes_by_kind(&inline_tree, SyntaxKind::BRACKETED_SPAN);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].contains("this"));
    }

    #[test]
    fn test_parse_native_span_with_markdown() {
        let input = "<span>Contains *emphasis* and `code`</span>.";
        let inline_tree = parse_inline(input);

        let spans = find_nodes_by_kind(&inline_tree, SyntaxKind::BRACKETED_SPAN);
        assert_eq!(spans.len(), 1);

        // Should have parsed the emphasis and code inside
        let emphasis = find_nodes_by_kind(&inline_tree, SyntaxKind::EMPHASIS);
        let code = find_nodes_by_kind(&inline_tree, SyntaxKind::CODE_SPAN);
        assert_eq!(emphasis.len(), 1);
        assert_eq!(code.len(), 1);
    }

    #[test]
    fn test_native_span_not_confused_with_autolink() {
        let input = "Link <https://example.com> and <span>text</span>.";
        let inline_tree = parse_inline(input);

        let autolinks = find_nodes_by_kind(&inline_tree, SyntaxKind::AUTO_LINK);
        let spans = find_nodes_by_kind(&inline_tree, SyntaxKind::BRACKETED_SPAN);
        assert_eq!(autolinks.len(), 1);
        assert_eq!(spans.len(), 1);
    }
}
