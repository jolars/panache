use crate::config::{BlankLines, Config, WrapMode};
use crate::syntax::{SyntaxKind, SyntaxNode};

use rowan::NodeOrToken;
use textwrap::wrap_algorithms::WrapAlgorithm;

pub struct Formatter {
    output: String,
    config: Config,
    consecutive_blank_lines: usize,
}

fn is_block_element(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PARAGRAPH
            | SyntaxKind::List
            | SyntaxKind::BlockQuote
            | SyntaxKind::MathBlock
            | SyntaxKind::CodeBlock
            | SyntaxKind::SimpleTable
    )
}

impl Formatter {
    pub fn new(config: Config) -> Self {
        Self {
            output: String::with_capacity(8192),
            config,
            consecutive_blank_lines: 0,
        }
    }

    fn build_words<'a>(
        &self,
        node: &SyntaxNode,
        arena: &'a mut Vec<Box<str>>,
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

            // Glue when there was no whitespace; otherwise start a new word and mark the space.
            fn push_piece(&mut self, text: &str) {
                if self.pending_space {
                    self.flush_pending();
                    self.start_new_piece(text);
                } else {
                    self.attach_to_previous(text);
                }
            }
        }

        let mut b = Builder::new(arena);

        for el in node.children_with_tokens() {
            match el {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::BlankLine => {
                        b.pending_space = true;
                    }
                    _ => {
                        b.push_piece(t.text());
                    }
                },
                NodeOrToken::Node(n) => {
                    if n.kind() == SyntaxKind::List {
                        b.pending_space = true;
                        continue;
                    }
                    let text = n.text().to_string();
                    b.push_piece(&text);
                }
            }
        }

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

    fn wrapped_lines_for_paragraph(&self, node: &SyntaxNode, width: usize) -> Vec<String> {
        let mut arena: Vec<Box<str>> = Vec::new();
        let words = self.build_words(node, &mut arena);

        let algo = WrapAlgorithm::new();
        let line_widths = [width];
        let lines = algo.wrap(&words, &line_widths);

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
            out_lines.push(acc);
        }
        out_lines
    }

    pub fn format(mut self, node: &SyntaxNode) -> String {
        self.format_node(node, 0);
        self.output
    }

    fn format_node(&mut self, node: &SyntaxNode, indent: usize) {
        // Reset blank line counter when we hit a non-blank node
        if node.kind() != SyntaxKind::BlankLine {
            self.consecutive_blank_lines = 0;
        }

        let line_width = self.config.line_width;

        match node.kind() {
            SyntaxKind::ROOT | SyntaxKind::DOCUMENT => {
                for el in node.children_with_tokens() {
                    match el {
                        rowan::NodeOrToken::Node(n) => self.format_node(&n, indent),
                        rowan::NodeOrToken::Token(t) => match t.kind() {
                            SyntaxKind::WHITESPACE => {}
                            SyntaxKind::NEWLINE => {}
                            SyntaxKind::BlankLine => {
                                self.output.push('\n');
                            }
                            SyntaxKind::ImageLinkStart
                            | SyntaxKind::LinkStart
                            | SyntaxKind::LatexCommand => {
                                self.output.push_str(t.text());
                            }
                            _ => self.output.push_str(t.text()),
                        },
                    }
                }
            }

            SyntaxKind::Heading => {
                // Determine level
                let mut level = 1;
                let mut content = String::new();
                let mut saw_content = false;

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::AtxHeadingMarker => {
                            let t = child.text().to_string();
                            level = t.chars().take_while(|&c| c == '#').count().clamp(1, 6);
                        }
                        SyntaxKind::SetextHeadingUnderline => {
                            let t = child.text().to_string();
                            if t.chars().all(|c| c == '=') {
                                level = 1;
                            } else {
                                level = 2;
                            }
                        }
                        SyntaxKind::HeadingContent => {
                            let mut t = child.text().to_string();
                            // Trim trailing spaces and closing hashes in ATX form
                            t = t.trim_end().to_string();
                            // Remove trailing " ###" if present
                            let trimmed_hash = t.trim_end_matches('#').to_string();
                            if trimmed_hash.len() != t.len() {
                                t = trimmed_hash.trim_end().to_string();
                            }
                            // Normalize internal newlines
                            content = t.trim().to_string();
                            saw_content = true;
                        }
                        _ => {}
                    }
                }
                if !saw_content {
                    content = node.text().to_string();
                }
                self.output.push_str(&"#".repeat(level));
                self.output.push(' ');
                self.output.push_str(&content);
                self.output.push('\n');

                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::LatexEnvironment => {
                // Output the environment exactly as written
                let text = node.text().to_string();
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::Comment => {
                let text = node.text().to_string();
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::LatexCommand => {
                // Standalone LaTeX commands - preserve exactly as written
                let text = node.text().to_string();
                self.output.push_str(&text);
                // Don't add extra newlines for standalone LaTeX commands
            }

            SyntaxKind::BlockQuote => {
                // Determine nesting depth by counting ancestor BlockQuote nodes (including self)
                let mut depth = 0usize;
                let mut cur = Some(node.clone());
                while let Some(n) = cur {
                    if n.kind() == SyntaxKind::BlockQuote {
                        depth += 1;
                    }
                    cur = n.parent();
                }

                // Prefixes for quoted content and blank quoted lines
                let content_prefix = "> ".repeat(depth); // includes trailing space
                let blank_prefix = content_prefix.trim_end(); // no trailing space

                // Format children (paragraphs, blank lines) with proper > prefix per depth
                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::PARAGRAPH => match wrap_mode {
                            WrapMode::Preserve => {
                                let text = child.text().to_string();
                                for line in text.lines() {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(line);
                                    self.output.push('\n');
                                }
                            }
                            WrapMode::Reflow => {
                                let width =
                                    self.config.line_width.saturating_sub(content_prefix.len());
                                let lines = self.wrapped_lines_for_paragraph(&child, width);
                                for line in lines {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(&line);
                                    self.output.push('\n');
                                }
                            }
                        },
                        SyntaxKind::BlankLine => {
                            self.output.push_str(blank_prefix);
                            self.output.push('\n');
                        }
                        _ => {
                            // Handle other content within block quotes
                            self.format_node(&child, indent);
                        }
                    }
                }
            }

            SyntaxKind::PARAGRAPH => {
                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                match wrap_mode {
                    WrapMode::Preserve => {
                        let text = node.text().to_string();
                        self.output.push_str(&text);
                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                    WrapMode::Reflow => {
                        let lines = self.wrapped_lines_for_paragraph(node, line_width);

                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            self.output.push_str(line);
                        }

                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                }
            }

            SyntaxKind::List => {
                // Add blank line before top-level lists (indent == 0) that follow content.
                // Don't add for nested lists (indent > 0) as they follow their parent item's content.
                if indent == 0 && !self.output.is_empty() && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
                let mut prev_was_item = false;
                for child in node.children() {
                    if child.kind() == SyntaxKind::ListItem {
                        if prev_was_item {
                            while self.output.ends_with("\n\n") {
                                self.output.pop();
                            }
                        }
                        prev_was_item = true;
                    }
                    if child.kind() == SyntaxKind::BlankLine {
                        continue;
                    }
                    self.format_node(&child, indent);
                }
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::SimpleTable => {
                // Preserve table as-is, including line breaks and spacing
                self.output.push_str(&node.text().to_string());
            }

            SyntaxKind::InlineMath => {
                for child in node.children() {
                    self.output.push_str(&child.text().to_string());
                }
            }

            SyntaxKind::ListItem => {
                // Compute indent and marker from leading tokens
                let mut marker = String::new();
                let mut local_indent = 0;
                let mut content_start = false;

                for el in node.children_with_tokens() {
                    match el {
                        NodeOrToken::Token(t) => match t.kind() {
                            SyntaxKind::WHITESPACE => {
                                if !content_start {
                                    local_indent += t.text().len();
                                }
                            }
                            SyntaxKind::ListMarker => {
                                marker = t.text().to_string();
                                content_start = true;
                            }
                            _ => {
                                content_start = true;
                            }
                        },
                        _ => {
                            content_start = true;
                        }
                    }
                }

                let total_indent = indent + local_indent;
                let hanging = marker.len() + 1 + total_indent; // +1 for the space after marker
                let available_width = self.config.line_width.saturating_sub(hanging);

                // Build words from the whole node, then drop the leading marker word
                let mut arena: Vec<Box<str>> = Vec::new();
                let mut words = self.build_words(node, &mut arena);
                if let Some(first) = words.first()
                    && first.word == marker
                {
                    // Remove the marker; we will print it ourselves with a following space
                    words.remove(0);
                }

                let algo = WrapAlgorithm::new();
                let line_widths = [available_width];
                let lines = algo.wrap(&words, &line_widths);

                for (i, line) in lines.iter().enumerate() {
                    if i == 0 {
                        self.output.push_str(&" ".repeat(total_indent));
                        self.output.push_str(&marker);
                        self.output.push(' ');
                    } else {
                        // Hanging indent includes marker + one space
                        self.output
                            .push_str(&" ".repeat(total_indent + marker.len() + 1));
                    }
                    for (j, w) in line.iter().enumerate() {
                        self.output.push_str(w.word);
                        if j + 1 < line.len() {
                            self.output.push_str(w.whitespace);
                        } else {
                            self.output.push_str(w.penalty);
                        }
                    }
                    self.output.push('\n');
                }

                // Format nested lists inside this list item aligned to the content column.
                for child in node.children() {
                    if child.kind() == SyntaxKind::List {
                        self.format_node(&child, total_indent + marker.len() + 1);
                    }
                }
            }

            SyntaxKind::FencedDiv => {
                let mut fence_open = None;
                let mut fence_close = None;
                let mut div_content = None;

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::DivFenceOpen => {
                            fence_open = Some(child.text().to_string());
                        }

                        SyntaxKind::DivContent => {
                            div_content = Some(child);
                        }

                        SyntaxKind::DivFenceClose => {
                            fence_close = Some(child.text().to_string());
                        }

                        _ => {}
                    }
                }

                if let Some(open) = fence_open {
                    self.output.push_str(&open);
                }
                if let Some(content_node) = div_content {
                    for grandchild in content_node.children() {
                        if grandchild.kind() == SyntaxKind::DOCUMENT {
                            for doc_child in grandchild.children() {
                                self.format_node(&doc_child, indent);
                            }
                        } else {
                            self.format_node(&grandchild, indent);
                        }
                    }
                }
                if let Some(close) = fence_close {
                    self.output.push_str(&close);
                    if !close.ends_with('\n') {
                        self.output.push('\n');
                    }
                }
            }

            SyntaxKind::InlineMathMarker => {
                // Output inline math as $...$ or $$...$$ (on the same line)
                self.output.push_str(node.text().to_string().trim());
            }

            SyntaxKind::MathBlock => {
                let mut label = None;
                let mut math_content = None;
                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::MathContent => {
                            math_content = Some(child.text().to_string());
                        }
                        SyntaxKind::Attribute => {
                            label = Some(child.text().to_string().trim().to_string());
                        }
                        _ => {}
                    }
                }
                // Opening fence
                self.output.push_str("$$\n");
                // Math content
                if let Some(content) = math_content {
                    let math_indent = self.config.math_indent;
                    for line in content.trim().lines() {
                        self.output.push_str(&" ".repeat(math_indent));
                        self.output.push_str(line.trim_end());
                        self.output.push('\n');
                    }
                }
                // Closing fence (with label if present)
                self.output.push_str("$$");
                if let Some(lbl) = label {
                    self.output.push(' ');
                    self.output.push_str(&lbl);
                }
                self.output.push('\n');
            }

            SyntaxKind::CodeBlock | SyntaxKind::FRONTMATTER => {
                // Preserve these blocks as-is
                let text = node.text().to_string();
                self.output.push_str(&text);
                // Ensure these blocks end with appropriate spacing
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::BlankLine => {
                // Apply blank_lines config to collapse consecutive blank lines
                match self.config.blank_lines {
                    BlankLines::Preserve => {
                        // Always output blank line
                        self.output.push('\n');
                        self.consecutive_blank_lines += 1;
                    }
                    BlankLines::Collapse => {
                        // Only output if we haven't already output one blank line
                        if self.consecutive_blank_lines == 0 {
                            self.output.push('\n');
                            self.consecutive_blank_lines = 1;
                        }
                        // Otherwise skip this blank line (collapsing to one)
                    }
                }
            }

            _ => {
                // Fallback: append node text (should be rare with children_with_tokens above)
                self.output.push_str(&node.text().to_string());
            }
        }
    }
}

pub fn format_tree(tree: &SyntaxNode, config: &Config) -> String {
    Formatter::new(config.clone()).format(tree)
}
