use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use textwrap::wrap_algorithms::WrapAlgorithm;

pub(super) fn build_words<'a>(
    _config: &Config,
    node: &SyntaxNode,
    arena: &'a mut Vec<Box<str>>,
    format_inline_fn: impl Fn(&SyntaxNode) -> String,
) -> Vec<textwrap::core::Word<'a>> {
    struct Builder<'a> {
        arena: &'a mut Vec<Box<str>>,
        piece_idx: Vec<usize>,
        whitespace_after: Vec<bool>,
        last_piece_pos: Option<usize>,
        pending_space: bool,
    }

    impl<'a> Builder<'a> {
        fn new(arena: &'a mut Vec<Box<str>>) -> Self {
            Self {
                arena,
                piece_idx: Vec::new(),
                whitespace_after: Vec::new(),
                last_piece_pos: None,
                pending_space: false,
            }
        }

        fn flush_pending(&mut self) {
            if self.pending_space {
                if let Some(prev) = self.last_piece_pos {
                    self.whitespace_after[prev] = true;
                }
                self.pending_space = false;
            }
        }

        fn attach_to_previous(&mut self, text: &str) {
            if let Some(pos) = self.last_piece_pos {
                let prev_idx = self.piece_idx[pos];
                let prev = &self.arena[prev_idx];
                let mut combined = String::with_capacity(prev.len() + text.len());
                combined.push_str(prev);
                combined.push_str(text);
                self.arena.push(combined.into_boxed_str());
                let new_idx = self.arena.len() - 1;
                self.piece_idx[pos] = new_idx;
            } else {
                // No previous piece; start a new one.
                self.start_new_piece(text);
            }
        }

        fn start_new_piece(&mut self, text: &str) {
            self.arena.push(Box::<str>::from(text));
            let idx = self.arena.len() - 1;
            self.piece_idx.push(idx);
            self.whitespace_after.push(false);
            self.last_piece_pos = Some(self.piece_idx.len() - 1);
        }

        fn push_piece(&mut self, text: &str) {
            if self.pending_space {
                self.flush_pending();
                self.start_new_piece(text);
            } else {
                self.attach_to_previous(text);
            }
        }
    }

    fn process_node_recursive<F>(node: &SyntaxNode, b: &mut Builder, format_inline_fn: &F)
    where
        F: Fn(&SyntaxNode) -> String,
    {
        for el in node.children_with_tokens() {
            match el {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::BLANK_LINE => {
                        b.pending_space = true;
                    }
                    // Skip blockquote markers - they're in the tree for losslessness but
                    // the formatter adds them dynamically when formatting BlockQuote nodes
                    SyntaxKind::BLOCKQUOTE_MARKER => {}
                    SyntaxKind::ESCAPED_CHAR => {
                        // Token already includes backslash (e.g., "\*")
                        b.push_piece(t.text());
                    }
                    SyntaxKind::EMPHASIS_MARKER | SyntaxKind::STRONG_MARKER => {
                        // Skip original markers - we'll add normalized ones
                    }
                    SyntaxKind::TEXT => {
                        let text = t.text();

                        // Split TEXT tokens on whitespace to create separate words
                        // Note: We don't preserve leading spaces here since list item continuations
                        // will be re-indented by the formatter

                        // Check if text starts with whitespace
                        if !text.is_empty() && text.starts_with(char::is_whitespace) {
                            b.pending_space = true;
                        }

                        let words: Vec<&str> = text.split_whitespace().collect();

                        for (i, word) in words.iter().enumerate() {
                            if i > 0 {
                                b.pending_space = true;
                            }
                            b.push_piece(word);
                        }

                        // If text ends with whitespace, mark pending space for next piece
                        if !words.is_empty() && text.ends_with(char::is_whitespace) {
                            b.pending_space = true;
                        }
                    }
                    _ => {
                        b.push_piece(t.text());
                    }
                },
                NodeOrToken::Node(n) => match n.kind() {
                    SyntaxKind::LIST => {
                        b.pending_space = true;
                    }
                    SyntaxKind::CODE_BLOCK | SyntaxKind::BLANK_LINE => {
                        // Skip code blocks and blank lines - they'll be handled separately
                        // Don't recurse into them
                    }
                    SyntaxKind::PARAGRAPH if matches!(node.kind(), SyntaxKind::LIST_ITEM) => {
                        // For PARAGRAPH children of ListItem, check if there's a BlankLine before it
                        // If yes, it's a true continuation paragraph (skip in wrapping)
                        // If no, it's a lazy continuation (include in wrapping)
                        let has_blank_before = n
                            .prev_sibling()
                            .map(|prev| prev.kind() == SyntaxKind::BLANK_LINE)
                            .unwrap_or(false);

                        if has_blank_before {
                            // True continuation paragraph - skip in wrapping, format separately
                        } else {
                            // Lazy continuation - include in wrapping
                            process_node_recursive(&n, b, format_inline_fn);
                        }
                    }
                    SyntaxKind::PARAGRAPH => {
                        // Recursively process PARAGRAPH content instead of treating it as a unit
                        process_node_recursive(&n, b, format_inline_fn);
                    }
                    SyntaxKind::EMPHASIS => {
                        b.push_piece("*");
                        process_node_recursive(&n, b, format_inline_fn);
                        b.push_piece("*");
                    }
                    SyntaxKind::STRONG => {
                        b.push_piece("**");
                        process_node_recursive(&n, b, format_inline_fn);
                        b.push_piece("**");
                    }
                    SyntaxKind::LINK => {
                        // Links can wrap at whitespace boundaries in link text
                        // Two types: inline [text](url) and reference [text][ref]
                        b.push_piece("[");

                        // Process link text recursively to allow wrapping
                        for child in n.children_with_tokens() {
                            if let NodeOrToken::Node(link_child) = child
                                && link_child.kind() == SyntaxKind::LINK_TEXT
                            {
                                process_node_recursive(&link_child, b, format_inline_fn);
                            }
                        }

                        // Collect closing parts: depends on link type
                        // Inline: "](" + LinkDest + ")" + Attribute
                        // Reference: "][" + LinkRef + "]" or shortcut "]"
                        let mut closing = String::new();
                        let mut past_link_text = false;

                        // Collect closing syntax
                        for child in n.children_with_tokens() {
                            match child {
                                NodeOrToken::Node(link_child) => match link_child.kind() {
                                    SyntaxKind::LINK_TEXT => {
                                        past_link_text = true;
                                    }
                                    SyntaxKind::LINK_DEST
                                    | SyntaxKind::LINK_REF
                                    | SyntaxKind::ATTRIBUTE => {
                                        if past_link_text {
                                            closing.push_str(&link_child.text().to_string());
                                        }
                                    }
                                    _ => {}
                                },
                                NodeOrToken::Token(t) => {
                                    if past_link_text && t.kind() == SyntaxKind::TEXT {
                                        closing.push_str(t.text());
                                    }
                                }
                            }
                        }

                        b.attach_to_previous(&closing);
                    }
                    SyntaxKind::IMAGE_LINK => {
                        // Image links work similarly to links but with "![" prefix
                        // Structure: ImageLinkStart "![" + ImageAlt (wrappable) + "](" + LinkDest + ")" + Attribute
                        b.push_piece("![");

                        // Process image alt text recursively to allow wrapping
                        for child in n.children_with_tokens() {
                            if let NodeOrToken::Node(img_child) = child
                                && img_child.kind() == SyntaxKind::IMAGE_ALT
                            {
                                process_node_recursive(&img_child, b, format_inline_fn);
                            }
                        }

                        // Collect closing parts: "](" + destination + ")" + attributes
                        let mut closing = String::new();
                        let mut past_image_alt = false;

                        for child in n.children_with_tokens() {
                            match child {
                                NodeOrToken::Node(img_child) => match img_child.kind() {
                                    SyntaxKind::IMAGE_ALT => {
                                        past_image_alt = true;
                                    }
                                    SyntaxKind::LINK_DEST | SyntaxKind::ATTRIBUTE => {
                                        if past_image_alt {
                                            closing.push_str(&img_child.text().to_string());
                                        }
                                    }
                                    _ => {}
                                },
                                NodeOrToken::Token(t) => {
                                    if past_image_alt && t.kind() == SyntaxKind::TEXT {
                                        closing.push_str(t.text());
                                    }
                                }
                            }
                        }

                        b.attach_to_previous(&closing);
                    }
                    _ => {
                        // For other inline nodes, format and push as single piece
                        let text = format_inline_fn(&n);
                        b.push_piece(&text);
                    }
                },
            }
        }
    }

    let mut b = Builder::new(arena);
    process_node_recursive(node, &mut b, &format_inline_fn);

    let mut words: Vec<textwrap::core::Word<'a>> = Vec::with_capacity(b.piece_idx.len());
    for (i, &idx) in b.piece_idx.iter().enumerate() {
        let s: &'a str = &b.arena[idx];
        let mut w = textwrap::core::Word::from(s);
        if b.whitespace_after.get(i).copied().unwrap_or(false) {
            w.whitespace = " ";
        }
        words.push(w);
    }
    words
}

pub(super) fn wrapped_lines_for_paragraph(
    _config: &Config,
    node: &SyntaxNode,
    width: usize,
    format_inline_fn: impl Fn(&SyntaxNode) -> String,
) -> Vec<String> {
    log::debug!("wrapped_lines_for_paragraph called with width={}", width);

    // Check if paragraph contains hard line breaks
    let has_hard_breaks = node
        .descendants_with_tokens()
        .any(|el| el.kind() == SyntaxKind::HARD_LINE_BREAK);

    if has_hard_breaks {
        // Don't wrap paragraphs with hard line breaks - preserve the breaks
        // But normalize hard breaks and format inline elements
        log::debug!("Paragraph contains hard line breaks - preserving them");

        let mut result = String::new();
        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(n) => {
                    result.push_str(&format_inline_fn(&n));
                }
                NodeOrToken::Token(t) => {
                    if t.kind() == SyntaxKind::HARD_LINE_BREAK {
                        // Normalize to backslash-newline if extension enabled
                        if _config.extensions.escaped_line_breaks {
                            result.push_str("\\\n");
                        } else {
                            result.push_str(t.text());
                        }
                    } else {
                        result.push_str(t.text());
                    }
                }
            }
        }

        return result.lines().map(|s| s.to_string()).collect();
    }

    let mut arena: Vec<Box<str>> = Vec::new();
    let words = build_words(_config, node, &mut arena, format_inline_fn);
    log::debug!("Built {} words for paragraph", words.len());
    log::trace!(
        "Words: {:?}",
        words.iter().map(|w| w.word).collect::<Vec<_>>()
    );

    let algo = WrapAlgorithm::new();
    let line_widths = [width];
    let lines = algo.wrap(&words, &line_widths);
    log::debug!("Wrapped into {} lines", lines.len());

    let mut out_lines = Vec::with_capacity(lines.len());

    for line in lines {
        let mut acc = String::new();
        for (i, w) in line.iter().enumerate() {
            acc.push_str(w.word);
            if i + 1 < line.len() {
                acc.push_str(w.whitespace);
            } else {
                acc.push_str(w.penalty);
            }
        }
        log::trace!("Line: '{}'", acc);
        out_lines.push(acc);
    }
    out_lines
}
