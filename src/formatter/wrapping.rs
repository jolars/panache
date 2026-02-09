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
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::BlankLine => {
                        b.pending_space = true;
                    }
                    SyntaxKind::EscapedChar => {
                        let escaped = format!("\\{}", t.text());
                        b.push_piece(&escaped);
                    }
                    SyntaxKind::EmphasisMarker | SyntaxKind::StrongMarker => {
                        // Skip original markers - we'll add normalized ones
                    }
                    SyntaxKind::TEXT => {
                        let text = t.text();

                        // If text starts with 4+ spaces, it might be indented code - preserve as-is
                        if text.starts_with("    ") {
                            b.push_piece(text);
                        } else {
                            // Split TEXT tokens on whitespace to create separate words

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
                    }
                    _ => {
                        b.push_piece(t.text());
                    }
                },
                NodeOrToken::Node(n) => match n.kind() {
                    SyntaxKind::List => {
                        b.pending_space = true;
                    }
                    SyntaxKind::Emphasis => {
                        b.push_piece("*");
                        process_node_recursive(&n, b, format_inline_fn);
                        b.push_piece("*");
                    }
                    SyntaxKind::Strong => {
                        b.push_piece("**");
                        process_node_recursive(&n, b, format_inline_fn);
                        b.push_piece("**");
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
